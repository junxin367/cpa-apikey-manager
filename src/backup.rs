use crate::{
    accounting::{Price, Tokens, MAX_VALUE},
    engine::{Fault, Result},
};
use chrono::Utc;
use rusqlite::{params_from_iter, types::Value as SqlValue, Connection};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs::OpenOptions, io::Write, path::Path};

pub const MAX_BYTES: usize = 128 * 1024 * 1024;
const TABLES: [&str; 8] = [
    "meta",
    "keys",
    "models",
    "prices",
    "price_origins",
    "requests",
    "usage",
    "audit",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportFilter {
    ConfigOnly,
    Since(i64),
    All,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Table {
    columns: Vec<String>,
    rows: Vec<Vec<Option<String>>>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Data {
    format: String,
    format_version: u32,
    plugin_version: String,
    exported_at: String,
    schema_version: i64,
    tables: BTreeMap<String, Table>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Archive {
    data: Data,
    sha256: String,
}

struct Column {
    name: String,
    integer: bool,
    required: bool,
}

fn invalid(message: impl Into<String>) -> Fault {
    Fault::new(400, "invalid_backup", message)
}

fn columns(db: &Connection, table: &str) -> Result<Vec<Column>> {
    let mut statement = db.prepare(&format!("PRAGMA table_info({table})"))?;
    let rows = statement.query_map([], |row| {
        Ok(Column {
            name: row.get(1)?,
            integer: row.get::<_, String>(2)? == "INTEGER",
            required: row.get::<_, bool>(3)? || row.get::<_, i64>(5)? > 0,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

fn read_tables(db: &Connection, filter: ExportFilter) -> Result<BTreeMap<String, Table>> {
    let mut tables = BTreeMap::new();
    for name in TABLES {
        let names: Vec<_> = columns(db, name)?.into_iter().map(|c| c.name).collect();
        if filter == ExportFilter::ConfigOnly && matches!(name, "requests" | "usage" | "audit") {
            tables.insert(
                name.into(),
                Table {
                    columns: names,
                    rows: Vec::new(),
                },
            );
            continue;
        }
        let query_sql = match (name, filter) {
            ("usage", ExportFilter::Since(_)) => format!(
                "SELECT {} FROM usage WHERE started >= ?1 ORDER BY rowid",
                names.join(",")
            ),
            ("requests", ExportFilter::Since(_)) => format!(
                "SELECT {} FROM requests WHERE started >= ?1 OR (request_id, model) IN (SELECT request_id, model FROM usage WHERE started >= ?1) ORDER BY rowid",
                names.join(",")
            ),
            ("audit", ExportFilter::Since(_)) => format!(
                "SELECT {} FROM audit WHERE created >= ?1 ORDER BY rowid",
                names.join(",")
            ),
            _ => format!("SELECT {} FROM {name} ORDER BY rowid", names.join(",")),
        };
        let mut statement = db.prepare(&query_sql)?;
        let len = names.len();
        let map_row = |row: &rusqlite::Row| -> rusqlite::Result<Vec<Option<String>>> {
            (0..len)
                .map(|index| match row.get_ref(index)? {
                    rusqlite::types::ValueRef::Null => Ok(None),
                    rusqlite::types::ValueRef::Integer(value) => Ok(Some(value.to_string())),
                    rusqlite::types::ValueRef::Text(value) => {
                        Ok(Some(String::from_utf8_lossy(value).into_owned()))
                    }
                    _ => Err(rusqlite::Error::InvalidQuery),
                })
                .collect()
        };
        let rows: Vec<Vec<Option<String>>> = match filter {
            ExportFilter::Since(start_ms) if matches!(name, "usage" | "requests" | "audit") => {
                statement
                    .query_map([start_ms], map_row)?
                    .collect::<rusqlite::Result<_>>()?
            }
            _ => statement
                .query_map([], map_row)?
                .collect::<rusqlite::Result<_>>()?,
        };
        tables.insert(
            name.into(),
            Table {
                columns: names,
                rows,
            },
        );
    }
    Ok(tables)
}

pub fn export(db: &mut Connection, filter: ExportFilter) -> Result<Value> {
    let tx = db.transaction()?;
    let data = Data {
        format: "cpa-apikey-manager-backup".into(),
        format_version: 1,
        plugin_version: env!("CARGO_PKG_VERSION").into(),
        exported_at: Utc::now().to_rfc3339(),
        schema_version: tx.pragma_query_value(None, "user_version", |row| row.get(0))?,
        tables: read_tables(&tx, filter)?,
    };
    let sha256 = hex::encode(Sha256::digest(serde_json::to_vec(&data)?));
    let archive = serde_json::to_value(Archive { data, sha256 })?;
    if serde_json::to_vec(&archive)?.len() > MAX_BYTES {
        return Err(Fault::new(
            413,
            "backup_too_large",
            "备份超过 128 MB，请停止宿主后备份整个数据目录",
        ));
    }
    tx.commit()?;
    Ok(archive)
}

fn insert_tables(db: &Connection, tables: &BTreeMap<String, Table>) -> Result<()> {
    if tables.len() != TABLES.len() || TABLES.iter().any(|name| !tables.contains_key(*name)) {
        return Err(invalid("备份缺少数据表或包含未知数据表"));
    }
    for name in TABLES {
        let table = &tables[name];
        let schema = columns(db, name)?;
        if table.columns != schema.iter().map(|c| c.name.clone()).collect::<Vec<_>>() {
            return Err(invalid(format!("备份数据表 {name} 的字段与当前版本不一致")));
        }
        let placeholders = vec!["?"; schema.len()].join(",");
        let mut statement = db.prepare(&format!(
            "INSERT INTO {name}({}) VALUES({placeholders})",
            table.columns.join(",")
        ))?;
        for row in &table.rows {
            if row.len() != schema.len() {
                return Err(invalid(format!("备份数据表 {name} 的记录不完整")));
            }
            let values: Vec<_> = row
                .iter()
                .zip(&schema)
                .map(|(value, column)| match value {
                    None if column.required => Err(invalid("备份的必填字段为空")),
                    None => Ok(SqlValue::Null),
                    Some(value) if column.integer => value
                        .parse::<i64>()
                        .map(SqlValue::Integer)
                        .map_err(|_| invalid("备份包含无效整数")),
                    Some(value) => Ok(SqlValue::Text(value.clone())),
                })
                .collect::<Result<_>>()?;
            statement
                .execute(params_from_iter(values))
                .map_err(|_| invalid(format!("备份数据表 {name} 包含重复或无效记录")))?;
        }
    }
    Ok(())
}

pub fn prepare(value: Value) -> Result<Connection> {
    let archive: Archive =
        serde_json::from_value(value).map_err(|_| invalid("文件不是有效的插件备份"))?;
    if archive.data.format != "cpa-apikey-manager-backup" || archive.data.format_version != 1 {
        return Err(invalid("不支持此备份格式"));
    }
    if chrono::DateTime::parse_from_rfc3339(&archive.data.exported_at).is_err() {
        return Err(invalid("备份时间无效"));
    }
    let checksum = hex::encode(Sha256::digest(serde_json::to_vec(&archive.data)?));
    if checksum != archive.sha256 {
        return Err(invalid("备份校验失败，文件可能已损坏或被修改"));
    }
    let mut db = Connection::open_in_memory()?;
    crate::migrations::initialize(&mut db)?;
    let version: i64 = db.pragma_query_value(None, "user_version", |row| row.get(0))?;
    // v5 and v6 have identical columns; v6 adds a terminal request status.
    if archive.data.schema_version != version && archive.data.schema_version != 5 {
        return Err(invalid("备份数据库版本不兼容，请使用匹配的插件版本恢复"));
    }
    let tx = db.transaction()?;
    for name in TABLES.into_iter().rev() {
        tx.execute(&format!("DELETE FROM {name}"), [])?;
    }
    insert_tables(&tx, &archive.data.tables)?;
    tx.commit()?;
    let secret: String = db
        .query_row("SELECT value FROM meta WHERE name='secret'", [], |r| {
            r.get(0)
        })
        .map_err(|_| invalid("备份缺少密钥标识数据"))?;
    if secret.len() != 64 || !secret.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err(invalid("备份中的密钥标识数据无效"));
    }
    let enabled: String = db
        .query_row(
            "SELECT value FROM meta WHERE name='enforcement_enabled'",
            [],
            |r| r.get(0),
        )
        .map_err(|_| invalid("备份缺少拦截设置"))?;
    if !["true", "false"].contains(&enabled.as_str()) {
        return Err(invalid("备份中的拦截设置无效"));
    }
    for name in ["recording_since", "continuous_recording_since"] {
        let timestamp: String = db
            .query_row("SELECT value FROM meta WHERE name=?1", [name], |r| r.get(0))
            .map_err(|_| invalid("备份缺少账本开始时间"))?;
        if chrono::DateTime::parse_from_rfc3339(&timestamp).is_err() {
            return Err(invalid("备份中的账本时间无效"));
        }
    }
    let mut statement = db.prepare("SELECT policy FROM keys")?;
    for row in statement.query_map([], |r| r.get::<_, String>(0))? {
        let policy =
            crate::engine::decode_policy(&row?).map_err(|_| invalid("备份中的密钥规则无效"))?;
        if !["day", "week", "month"].contains(&policy.period.as_str())
            || !["model", "channel"].contains(&policy.rule_mode.as_str())
            || !["allow", "deny"].contains(&policy.total_access.as_str())
            || policy.revision < 0
            || policy.note.chars().count() > 100
            || policy.rules.len() > 1000
            || (policy.total_quota_enabled
                && policy.total_access == "allow"
                && policy.total_quota.is_none())
        {
            return Err(invalid("备份中的密钥权限或额度配置无效"));
        }
        if let Some(quota) = &policy.total_quota {
            quota.amount().map_err(|_| invalid("备份中的总额度无效"))?;
        }
        for (name, rule) in &policy.rules {
            crate::engine::validate_model(name).map_err(|_| invalid("备份中的模型规则无效"))?;
            if !["allow", "deny"].contains(&rule.access.as_str()) {
                return Err(invalid("备份中的模型权限无效"));
            }
        }
        for (name, rule) in &policy.channel_rules {
            if !crate::channel::valid(name) || !["allow", "deny"].contains(&rule.access.as_str()) {
                return Err(invalid("备份中的渠道权限无效"));
            }
        }
        for rule in policy.rules.values().chain(policy.channel_rules.values()) {
            if let Some(quota) = &rule.quota {
                quota
                    .amount()
                    .map_err(|_| invalid("备份中的分项额度无效"))?;
            }
        }
    }
    drop(statement);
    let mut statement = db.prepare(
        "SELECT price FROM prices UNION ALL SELECT price FROM requests WHERE price IS NOT NULL",
    )?;
    for row in statement.query_map([], |r| r.get::<_, String>(0))? {
        let price: Price =
            serde_json::from_str(&row?).map_err(|_| invalid("备份中的模型价格无效"))?;
        price
            .validate()
            .map_err(|_| invalid("备份中的模型价格无效"))?;
    }
    drop(statement);
    let mut statement = db.prepare("SELECT detail,tokens,cost FROM usage")?;
    for row in statement.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, Option<i64>>(2)?,
        ))
    })? {
        let (detail, count, cost) = row?;
        let tokens: Tokens =
            serde_json::from_str(&detail).map_err(|_| invalid("备份中的用量明细无效"))?;
        let counts = [
            tokens.input,
            tokens.output,
            tokens.cache_read,
            tokens.cache_write,
        ];
        if counts
            .iter()
            .chain([&tokens.total, &count])
            .any(|n| !(0..=MAX_VALUE).contains(n))
            || count != tokens.total
            || cost.is_some_and(|c| !(0..=MAX_VALUE).contains(&c))
            || (tokens.complete && counts.iter().sum::<i64>() != tokens.total)
        {
            return Err(invalid("备份中的 Token 或费用无效"));
        }
    }
    drop(statement);
    let invalid_state: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM requests WHERE status NOT IN ('pending','review','settled','unmeasured'))
         OR EXISTS(SELECT 1 FROM keys WHERE active NOT IN (0,1))
         OR EXISTS(SELECT 1 FROM usage WHERE failed NOT IN (0,1) OR stream NOT IN (0,1) OR manual NOT IN (0,1))
         OR EXISTS(SELECT 1 FROM requests r WHERE r.status='unmeasured'
             AND (r.completed IS NULL OR r.outcome IS NULL
                 OR r.outcome NOT IN ('failed','canceled','rejected')
                 OR EXISTS(SELECT 1 FROM usage u WHERE u.request_id=r.request_id AND u.model=r.model
                     AND (u.manual=1 OR u.tokens<>0 OR COALESCE(u.cost,0)<>0
                         OR COALESCE(json_extract(u.detail,'$.input'),0)<>0
                         OR COALESCE(json_extract(u.detail,'$.output'),0)<>0
                         OR COALESCE(json_extract(u.detail,'$.cache_read'),0)<>0
                         OR COALESCE(json_extract(u.detail,'$.cache_write'),0)<>0))))",
        [],
        |r| r.get(0),
    )?;
    let orphan: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM usage WHERE key_id NOT IN (SELECT id FROM keys))
         OR EXISTS(SELECT 1 FROM requests WHERE key_id NOT IN (SELECT id FROM keys))
         OR EXISTS(SELECT 1 FROM usage u LEFT JOIN requests r
                   ON r.request_id=u.request_id AND r.model=u.model
                   WHERE r.request_id IS NULL OR r.key_id<>u.key_id)",
        [],
        |r| r.get(0),
    )?;
    if invalid_state || orphan {
        return Err(invalid("备份中的请求状态或密钥关联无效"));
    }
    Ok(db)
}

pub fn retain_pending(
    source: &Connection,
    target: &Connection,
    key_ids: &BTreeMap<String, String>,
) -> Result<()> {
    let names: Vec<_> = columns(source, "requests")?
        .into_iter()
        .map(|c| c.name)
        .collect();
    let key_index = names
        .iter()
        .position(|name| name == "key_id")
        .ok_or("请求字段无效")?;
    let status_index = names
        .iter()
        .position(|name| name == "status")
        .ok_or("请求字段无效")?;
    let mut query = source.prepare(&format!(
        "SELECT {} FROM requests WHERE status='pending'",
        names.join(",")
    ))?;
    let mut insert = target.prepare(&format!(
        "INSERT OR IGNORE INTO requests({}) VALUES({})",
        names.join(","),
        vec!["?"; names.len()].join(",")
    ))?;
    let rows = query.query_map([], |row| {
        (0..names.len())
            .map(|index| row.get::<_, SqlValue>(index))
            .collect::<rusqlite::Result<Vec<_>>>()
    })?;
    for row in rows {
        let mut values = row?;
        let SqlValue::Text(key) = &values[key_index] else {
            continue;
        };
        let Some(restored_key) = key_ids.get(key) else {
            continue;
        };
        values[key_index] = SqlValue::Text(restored_key.clone());
        values[status_index] = SqlValue::Text("review".into());
        insert.execute(params_from_iter(values))?;
    }
    Ok(())
}

pub fn summary(db: &Connection) -> Result<Value> {
    let mut result = serde_json::Map::new();
    for name in ["keys", "models", "prices", "requests", "usage"] {
        let count: i64 = db.query_row(&format!("SELECT COUNT(*) FROM {name}"), [], |r| r.get(0))?;
        result.insert(name.into(), json!(count));
    }
    Ok(Value::Object(result))
}

pub fn replace(db: &mut Connection, source: &Connection) -> Result<()> {
    let tables = read_tables(source, ExportFilter::All)?;
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    for name in TABLES.into_iter().rev() {
        tx.execute(&format!("DELETE FROM {name}"), [])?;
    }
    insert_tables(&tx, &tables)?;
    tx.commit()?;
    Ok(())
}

pub fn save_rollback(directory: &Path, archive: &Value) -> Result<String> {
    let path = directory.join(format!(
        "backup-before-import-{}-{}.json",
        Utc::now().format("%Y%m%d-%H%M%S"),
        uuid::Uuid::new_v4().simple()
    ));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let failure = || {
        Fault::new(
            503,
            "backup_save_failed",
            "无法保存导入前的回退备份，当前数据未修改；请检查数据目录权限和磁盘空间",
        )
    };
    let mut file = options.open(&path).map_err(|_| failure())?;
    let result = (|| -> std::io::Result<()> {
        file.write_all(serde_json::to_vec(archive)?.as_slice())?;
        file.sync_all()
    })();
    if result.is_err() {
        drop(file);
        let _ = std::fs::remove_file(&path);
        return Err(failure());
    }
    Ok(path.file_name().unwrap().to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config::Config,
        engine::{Engine, Policy, Quota},
    };
    use rusqlite::params;
    use std::fs;

    fn engine(root: &Path, raw_key: &str) -> Engine {
        fs::create_dir_all(root).unwrap();
        let config_path = root.join("config.yaml");
        fs::write(
            &config_path,
            format!("access:\n  api-keys: [\"{raw_key}\"]\n"),
        )
        .unwrap();
        let mut config = Config {
            cpa_config_path: config_path,
            data_dir: root.to_path_buf(),
            ..Config::default()
        };
        config.price_sync.enabled = false;
        Engine::open(config).unwrap()
    }

    fn root() -> std::path::PathBuf {
        std::env::current_dir()
            .unwrap()
            .join("temp")
            .join(format!("backup-test-{}", uuid::Uuid::new_v4()))
    }

    fn resign(value: &mut Value) {
        let archive: Archive = serde_json::from_value(value.clone()).unwrap();
        value["sha256"] = json!(hex::encode(Sha256::digest(
            serde_json::to_vec(&archive.data).unwrap()
        )));
    }

    #[test]
    fn unmeasured_requests_round_trip_and_v5_backups_still_import() {
        let root = root();
        let raw = "sk-unmeasured-backup";
        let now = Utc::now();
        let mut source = engine(&root.join("source"), raw);
        let key = source.snapshot(now).unwrap()["keys"][0]["id"]
            .as_str()
            .unwrap()
            .to_string();
        source
            .save_policy(
                &key,
                Policy {
                    total_quota_enabled: true,
                    total_quota: Some(Quota {
                        unit: "tokens".into(),
                        limit: "100000".into(),
                    }),
                    ..Default::default()
                },
                false,
                now,
            )
            .unwrap();
        let request = json!({"RequestID":"failed","Model":"gpt-6.1-sol","Outcome":"failed",
            "Metadata":{"caller_scope":crate::config::caller_scope(raw)}});
        source.intercept(&request, true, now).unwrap();
        source
            .usage(
                &json!({"RequestID":"failed","Model":"gpt-6.1-sol","APIKey":raw,
            "Provider":"codex","Failed":false,"Detail":{}}),
                now,
            )
            .unwrap();
        source.complete(&request, now).unwrap();
        let archive = source.export_backup(None).unwrap();
        let mut target = engine(&root.join("target"), raw);
        target.import_backup(archive.clone(), true).unwrap();
        assert_eq!(target.snapshot(now).unwrap()["review_count"], 0);
        assert_eq!(
            target
                .db
                .query_row("SELECT status FROM requests", [], |r| r.get::<_, String>(0))
                .unwrap(),
            "unmeasured"
        );
        let mut legacy = archive.clone();
        legacy["data"]["schema_version"] = json!(5);
        let status_index = legacy["data"]["tables"]["requests"]["columns"]
            .as_array()
            .unwrap()
            .iter()
            .position(|c| c == "status")
            .unwrap();
        legacy["data"]["tables"]["requests"]["rows"][0][status_index] = json!("review");
        resign(&mut legacy);
        target.import_backup(legacy, true).unwrap();
        assert_eq!(target.snapshot(now).unwrap()["review_count"], 0);
        assert_eq!(
            target.export_backup(None).unwrap()["data"]["tables"]["usage"],
            archive["data"]["tables"]["usage"]
        );
        let mut invalid = archive;
        let outcome_index = invalid["data"]["tables"]["requests"]["columns"]
            .as_array()
            .unwrap()
            .iter()
            .position(|c| c == "outcome")
            .unwrap();
        invalid["data"]["tables"]["requests"]["rows"][0][outcome_index] = json!("succeeded");
        resign(&mut invalid);
        assert!(target.validate_backup(invalid).is_err());
        drop(target);
        drop(source);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn import_preserves_inflight_admissions_without_merging_settled_usage() {
        let root = root();
        let raw = "sk-import-ongoing-probe";
        let mut target = engine(&root, raw);
        let key = target.snapshot(Utc::now()).unwrap()["keys"][0]["id"]
            .as_str()
            .unwrap()
            .to_string();
        target
            .save_policy(
                &key,
                Policy {
                    total_quota_enabled: true,
                    total_quota: Some(Quota {
                        unit: "tokens".into(),
                        limit: "1000".into(),
                    }),
                    ..Policy::default()
                },
                false,
                Utc::now(),
            )
            .unwrap();
        target
            .save_price(
                "gpt-5.5",
                serde_json::from_value(json!({"input":"1","output":"2"})).unwrap(),
                0,
            )
            .unwrap();
        target.set_enforcement_enabled(true).unwrap();
        let archive = target.export_backup(None).unwrap();
        let request = json!({"RequestID":"ongoing-across-import","Model":"gpt-5.5","RequestedModel":"gpt-5.5",
            "Metadata":{"caller_scope":crate::config::caller_scope(raw)}});
        target.intercept(&request, true, Utc::now()).unwrap();
        target.import_backup(archive, true).unwrap();
        target.usage(&json!({"RequestID":"ongoing-across-import","APIKey":raw,"Model":"gpt-5.5","Provider":"openai",
            "Detail":{"InputTokens":9,"OutputTokens":1,"TotalTokens":10}}), Utc::now()).unwrap();
        let total =
            target.snapshot(Utc::now()).unwrap()["keys"][0]["total_usage"]["tokens"].clone();
        let mut source = engine(&root.join("source"), raw);
        let foreign = source.export_backup(None).unwrap();
        let request = json!({"RequestID":"ongoing-new-secret","Model":"gpt-5.5","RequestedModel":"gpt-5.5",
            "Metadata":{"caller_scope":crate::config::caller_scope(raw)}});
        target.intercept(&request, true, Utc::now()).unwrap();
        target.import_backup(foreign, true).unwrap();
        target.usage(&json!({"RequestID":"ongoing-new-secret","APIKey":raw,"Model":"gpt-5.5","Provider":"openai",
            "Detail":{"InputTokens":9,"OutputTokens":1,"TotalTokens":10}}), Utc::now()).unwrap();
        let state = target.snapshot(Utc::now()).unwrap();
        let rebound = state["keys"][0]["id"] != key;
        let resumed = state["keys"][0]["total_usage"]["tokens"].clone();
        let usage_count: i64 = target
            .db
            .query_row("SELECT COUNT(*) FROM usage", [], |row| row.get(0))
            .unwrap();
        let good = target.export_backup(None).unwrap();
        let mut orphan = good.clone();
        orphan["data"]["tables"]["requests"]["rows"] = json!([]);
        resign(&mut orphan);
        let orphan_rejected = target.validate_backup(orphan).is_err();
        let mut flag = good;
        let index = flag["data"]["tables"]["usage"]["columns"]
            .as_array()
            .unwrap()
            .iter()
            .position(|name| name == "manual")
            .unwrap();
        flag["data"]["tables"]["usage"]["rows"][0][index] = json!("2");
        resign(&mut flag);
        let flag_rejected = target.validate_backup(flag).is_err();
        drop(source);
        drop(target);
        fs::remove_dir_all(root).unwrap();
        assert_eq!(total, "10");
        assert!(rebound);
        assert_eq!(resumed, "10");
        assert_eq!(usage_count, 1);
        assert!(orphan_rejected && flag_rejected);
    }

    #[test]
    fn backup_rejects_invalid_permission_fields() {
        let root = root();
        let mut target = engine(&root, "sk-import-policy-probe");
        let mut archive = target.export_backup(None).unwrap();
        let columns = archive["data"]["tables"]["keys"]["columns"]
            .as_array()
            .unwrap();
        let index = columns.iter().position(|name| name == "policy").unwrap();
        let mut policy: Value = serde_json::from_str(
            archive["data"]["tables"]["keys"]["rows"][0][index]
                .as_str()
                .unwrap(),
        )
        .unwrap();
        policy["rule_mode"] = json!("unknown");
        policy["total_access"] = json!("unknown");
        archive["data"]["tables"]["keys"]["rows"][0][index] = json!(policy.to_string());
        resign(&mut archive);
        let result = target.validate_backup(archive);
        drop(target);
        fs::remove_dir_all(root).unwrap();
        assert!(result.is_err());
    }

    #[test]
    fn full_backup_restores_key_identity_quotas_ledger_and_integer_precision() {
        let root = root();
        let raw = "sk-backup-test-original";
        let mut source = engine(&root.join("source"), raw);
        let key = source.snapshot(Utc::now()).unwrap()["keys"][0]["id"]
            .as_str()
            .unwrap()
            .to_string();
        source
            .save_policy(
                &key,
                Policy {
                    note: "恢复验证".into(),
                    total_quota_enabled: true,
                    total_quota: Some(Quota {
                        unit: "tokens".into(),
                        limit: "100000".into(),
                    }),
                    ..Policy::default()
                },
                false,
                Utc::now(),
            )
            .unwrap();
        source
            .save_price(
                "gpt-5.5",
                serde_json::from_value(json!({"input":"1","output":"2"})).unwrap(),
                0,
            )
            .unwrap();
        source.set_timezone("Asia/Shanghai").unwrap();
        source.set_enforcement_enabled(true).unwrap();
        let request = |id: &str| {
            json!({
                "RequestID":id,"Model":"gpt-5.5","RequestedModel":"gpt-5.5",
                "Metadata":{"caller_scope":crate::config::caller_scope(raw)}
            })
        };
        source
            .intercept(&request("settled"), true, Utc::now())
            .unwrap();
        source
            .usage(
                &json!({
                    "RequestID":"settled","APIKey":raw,"Model":"gpt-5.5","Provider":"openai",
                    "Detail":{"InputTokens":9,"OutputTokens":1,"TotalTokens":10}
                }),
                Utc::now(),
            )
            .unwrap();
        source
            .intercept(&request("pending"), true, Utc::now())
            .unwrap();
        source.db.execute(
            "INSERT INTO audit(created,action,subject,detail) VALUES(?1,'large_integer','test','')",
            [9_007_199_254_740_993_i64],
        ).unwrap();
        let archive = source.export_backup(None).unwrap();
        assert!(!archive.to_string().contains(raw));
        assert!(archive.to_string().contains("\"9007199254740993\""));
        let mut target = engine(&root.join("target"), raw);
        let previous_key = target.snapshot(Utc::now()).unwrap()["keys"][0]["id"].clone();
        assert_ne!(previous_key, key);
        target.validate_backup(archive.clone()).unwrap();
        let result = target.import_backup(archive.clone(), true).unwrap();
        let snapshot = target.snapshot(Utc::now()).unwrap();
        assert_eq!(snapshot["enforcement_enabled"], false);
        assert_eq!(snapshot["timezone"], "Asia/Shanghai");
        assert_eq!(snapshot["keys"][0]["id"], key);
        assert_eq!(snapshot["keys"][0]["policy"]["note"], "恢复验证");
        assert_eq!(snapshot["keys"][0]["total_usage"]["tokens"], "10");
        assert_eq!(snapshot["keys"][0]["total_usage"]["cost_micros"], 11);
        assert_eq!(snapshot["keys"][0]["total_usage"]["review"], 1);
        assert_eq!(target.reveal_key(&key).unwrap(), raw);
        let large: i64 = target
            .db
            .query_row(
                "SELECT created FROM audit WHERE action='large_integer'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(large, 9_007_199_254_740_993);
        let rollback_path = target
            .config
            .data_dir
            .join(result["rollback_file"].as_str().unwrap());
        let rollback: Value = serde_json::from_slice(&fs::read(rollback_path).unwrap()).unwrap();
        let rollback_db = prepare(rollback).unwrap();
        assert_eq!(summary(&rollback_db).unwrap()["usage"], 0);
        drop(rollback_db);
        target.set_enforcement_enabled(true).unwrap();
        target.import_backup(archive, true).unwrap();
        assert!(target.enforcement_enabled().unwrap());
        drop(source);
        drop(target);
        let mut reopened = engine(&root.join("target"), raw);
        assert_eq!(reopened.snapshot(Utc::now()).unwrap()["keys"][0]["id"], key);
        drop(reopened);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn invalid_and_unconfirmed_backups_never_change_current_data() {
        let root = root();
        let mut target = engine(&root, "sk-backup-test-current");
        let archive = target.export_backup(None).unwrap();
        let before = archive["data"]["tables"].clone();
        assert_eq!(
            target
                .import_backup(archive.clone(), false)
                .unwrap_err()
                .code,
            "backup_confirmation_required"
        );
        target.set_enforcement_enabled(true).unwrap();
        let mut damaged = archive.clone();
        damaged["sha256"] = json!("invalid");
        assert_eq!(
            target.import_backup(damaged, true).unwrap_err().code,
            "invalid_backup"
        );
        let mut missing = archive.clone();
        missing["data"]["tables"]
            .as_object_mut()
            .unwrap()
            .remove("usage");
        resign(&mut missing);
        assert_eq!(
            target.import_backup(missing, true).unwrap_err().code,
            "invalid_backup"
        );
        let mut future = archive.clone();
        future["data"]["schema_version"] = json!(99);
        resign(&mut future);
        assert_eq!(
            target.import_backup(future, true).unwrap_err().code,
            "invalid_backup"
        );
        let mut duplicate = archive.clone();
        let row = duplicate["data"]["tables"]["keys"]["rows"][0].clone();
        duplicate["data"]["tables"]["keys"]["rows"]
            .as_array_mut()
            .unwrap()
            .push(row);
        resign(&mut duplicate);
        assert_eq!(
            target.import_backup(duplicate, true).unwrap_err().code,
            "invalid_backup"
        );
        let after = target.export_backup(None).unwrap();
        assert_eq!(before["keys"], after["data"]["tables"]["keys"]);
        assert_eq!(before["usage"], after["data"]["tables"]["usage"]);
        assert_eq!(before["prices"], after["data"]["tables"]["prices"]);
        assert!(target.enforcement_enabled().unwrap());
        assert!(!fs::read_dir(&root).unwrap().any(|entry| entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with("backup-before-import")));
        drop(target);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failed_database_restore_rolls_back_and_keeps_recovery_backup() {
        let root = root();
        let mut source = engine(&root.join("source"), "sk-backup-test");
        source
            .save_price(
                "gpt-5.5",
                serde_json::from_value(json!({"input":"1","output":"2"})).unwrap(),
                0,
            )
            .unwrap();
        let archive = source.export_backup(None).unwrap();
        let mut target = engine(&root.join("target"), "sk-backup-test");
        let original = target.export_backup(None).unwrap()["data"]["tables"].clone();
        target.db.execute_batch(
            "CREATE TEMP TRIGGER fail_import BEFORE INSERT ON prices BEGIN SELECT RAISE(ABORT,'test failure'); END;",
        ).unwrap();
        assert!(target.import_backup(archive, true).is_err());
        let after = target.export_backup(None).unwrap()["data"]["tables"].clone();
        assert_eq!(after, original);
        assert_eq!(
            target.price_sync.as_ref().unwrap().status()["enabled"],
            false
        );
        target.db.execute_batch("DROP TRIGGER fail_import").unwrap();
        let original_directory = target.config.data_dir.clone();
        target.config.data_dir = target.config.cpa_config_path.clone();
        let archive = source.export_backup(None).unwrap();
        assert_eq!(
            target.import_backup(archive, true).unwrap_err().code,
            "backup_save_failed"
        );
        assert_eq!(
            target.export_backup(None).unwrap()["data"]["tables"],
            original
        );
        target.config.data_dir = original_directory;
        assert!(fs::read_dir(root.join("target")).unwrap().any(|entry| entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with("backup-before-import")));
        drop(source);
        drop(target);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn backup_import_does_not_restore_or_create_host_api_keys() {
        let root = root();
        let mut source = engine(&root.join("source"), "sk-backup-old");
        let old_key = source.snapshot(Utc::now()).unwrap()["keys"][0]["id"]
            .as_str()
            .unwrap()
            .to_string();
        let mut target = engine(&root.join("target"), "sk-backup-current");
        let config_before = fs::read(&target.config.cpa_config_path).unwrap();
        target
            .import_backup(source.export_backup(None).unwrap(), true)
            .unwrap();
        assert_eq!(
            fs::read(&target.config.cpa_config_path).unwrap(),
            config_before
        );
        assert!(target.reveal_key(&old_key).is_err());
        let active: bool = target
            .db
            .query_row(
                "SELECT active FROM keys WHERE id=?1",
                params![old_key],
                |r| r.get(0),
            )
            .unwrap();
        assert!(!active);
        assert_eq!(
            target.snapshot(Utc::now()).unwrap()["keys"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        drop(source);
        drop(target);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn export_range_filters_appropriately() {
        let root = root();
        let mut target = engine(&root, "sk-export-range-test");
        let key = target.snapshot(Utc::now()).unwrap()["keys"][0]["id"]
            .as_str()
            .unwrap()
            .to_string();
        target
            .save_policy(
                &key,
                Policy {
                    total_quota_enabled: true,
                    total_quota: Some(Quota {
                        unit: "tokens".into(),
                        limit: "100000".into(),
                    }),
                    ..Policy::default()
                },
                false,
                Utc::now(),
            )
            .unwrap();
        target
            .save_price(
                "gpt-5.5",
                serde_json::from_value(json!({"input":"1","output":"2"})).unwrap(),
                0,
            )
            .unwrap();
        let now = Utc::now();
        let req = json!({"RequestID":"range-test-req","Model":"gpt-5.5","RequestedModel":"gpt-5.5",
            "Metadata":{"caller_scope":crate::config::caller_scope("sk-export-range-test")}});
        target.intercept(&req, true, now).unwrap();
        target.usage(&json!({"RequestID":"range-test-req","APIKey":"sk-export-range-test","Model":"gpt-5.5","Provider":"openai",
            "Detail":{"InputTokens":10,"OutputTokens":5,"TotalTokens":15}}), now).unwrap();

        // 1. 导出 config 范围：无流水明细，但校验和导入必须通过
        let config_archive = target.export_backup(Some("config")).unwrap();
        assert_eq!(
            config_archive["data"]["tables"]["requests"]["rows"]
                .as_array()
                .unwrap()
                .len(),
            0
        );
        assert_eq!(
            config_archive["data"]["tables"]["usage"]["rows"]
                .as_array()
                .unwrap()
                .len(),
            0
        );
        assert_eq!(
            config_archive["data"]["tables"]["audit"]["rows"]
                .as_array()
                .unwrap()
                .len(),
            0
        );
        assert!(
            config_archive["data"]["tables"]["keys"]["rows"]
                .as_array()
                .unwrap()
                .len()
                > 0
        );
        assert!(
            config_archive["data"]["tables"]["prices"]["rows"]
                .as_array()
                .unwrap()
                .len()
                > 0
        );
        target.validate_backup(config_archive.clone()).unwrap();

        // 2. 导出 week 范围：包含当前周数据
        let week_archive = target.export_backup(Some("week")).unwrap();
        assert_eq!(
            week_archive["data"]["tables"]["usage"]["rows"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            week_archive["data"]["tables"]["requests"]["rows"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        target.validate_backup(week_archive).unwrap();

        // 3. 导出 month 范围：包含当前月数据
        let month_archive = target.export_backup(Some("month")).unwrap();
        assert_eq!(
            month_archive["data"]["tables"]["usage"]["rows"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        target.validate_backup(month_archive).unwrap();

        // 4. 非法范围报错
        assert_eq!(
            target.export_backup(Some("invalid")).unwrap_err().code,
            "invalid_range"
        );

        drop(target);
        fs::remove_dir_all(root).unwrap();
    }
}
