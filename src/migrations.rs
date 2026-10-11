use crate::{
    channel,
    engine::{Fault, Result},
};
use chrono::Utc;
use rusqlite::{params, Connection};
use serde_json::Value;
use std::collections::BTreeSet;

fn v3_channel(model: &str) -> &'static str {
    let name = model
        .rsplit('/')
        .find(|part| !part.is_empty())
        .unwrap_or("")
        .to_ascii_lowercase();
    let root = name.split(['-', '_', '.', ':']).next().unwrap_or("");
    match root {
        "gpt" | "chatgpt" | "codex" => "gpt",
        "claude" => "claude",
        "deepseek" => "deepseek",
        "glm" => "glm",
        "kimi" | "moonshot" => "kimi",
        "gemini" => "gemini",
        _ if channel::kimi_short(root) => "kimi",
        _ if root.strip_prefix('o').is_some_and(|n| {
            n.starts_with(|c: char| ('1'..='9').contains(&c))
                && n.bytes().all(|c| c.is_ascii_digit())
        }) =>
        {
            "gpt"
        }
        _ => "other",
    }
}

pub fn initialize(db: &mut Connection) -> Result<()> {
    let version: i64 = db.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version > 5 {
        return Err(Fault::new(
            503,
            "database_version_unsupported",
            "数据库来自更新的插件版本，请使用匹配版本或恢复备份",
        ));
    }
    db.execute_batch(include_str!("schema.sql"))?;
    if version < 2 {
        let tx = db.transaction()?;
        tx.execute_batch(
            "ALTER TABLE keys ADD COLUMN recording_since TEXT NOT NULL DEFAULT '';
             ALTER TABLE requests ADD COLUMN channel TEXT NOT NULL DEFAULT 'other';
             ALTER TABLE usage ADD COLUMN channel TEXT NOT NULL DEFAULT 'other';",
        )?;
        let since = Utc::now().to_rfc3339();
        tx.execute(
            "INSERT OR IGNORE INTO meta VALUES('continuous_recording_since',?1)",
            [&since],
        )?;
        tx.execute(
            "UPDATE keys SET recording_since=?1 WHERE recording_since=''",
            [&since],
        )?;
        for table in ["requests", "usage"] {
            let models = {
                let mut stmt = tx.prepare(&format!("SELECT DISTINCT model FROM {table}"))?;
                let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
                rows.collect::<std::result::Result<Vec<_>, _>>()?
            };
            for model in models {
                tx.execute(
                    &format!("UPDATE {table} SET channel=?1 WHERE model=?2"),
                    params![v3_channel(&model), model],
                )?;
            }
        }
        tx.execute_batch(
            "CREATE INDEX usage_channel_period ON usage(key_id,channel,started);
             CREATE INDEX requests_channel_review ON requests(key_id,channel,status);
             PRAGMA user_version=2;",
        )?;
        tx.commit()?;
    }
    if version < 3 {
        // v3 added the Gemini channel and short Kimi names. Only rows that were classified as
        // "other" by model name move; upstream-based channels such as Antigravity stay as recorded.
        let tx = db.transaction()?;
        for table in ["requests", "usage"] {
            let models = {
                let mut stmt = tx.prepare(&format!(
                    "SELECT DISTINCT model FROM {table} WHERE channel='other'"
                ))?;
                let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
                rows.collect::<std::result::Result<Vec<_>, _>>()?
            };
            for model in models {
                let channel = v3_channel(&model);
                if channel != "other" {
                    tx.execute(
                        &format!(
                            "UPDATE {table} SET channel=?1 WHERE model=?2 AND channel='other'"
                        ),
                        params![channel, model],
                    )?;
                }
            }
        }
        tx.execute_batch("PRAGMA user_version=3;")?;
        tx.commit()?;
    }
    if version < 4 {
        // v4 identifies channels from model names, so families that fell into "other" get their
        // own channel. Each new channel inherits a key's "other" rule, keeping existing denials
        // and limits on those models.
        let tx = db.transaction()?;
        let mut split = BTreeSet::new();
        for table in ["requests", "usage"] {
            let models = {
                let mut stmt = tx.prepare(&format!(
                    "SELECT DISTINCT model FROM {table} WHERE channel='other'"
                ))?;
                let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
                rows.collect::<std::result::Result<Vec<_>, _>>()?
            };
            for model in models {
                let channel = channel::classify(&model);
                if channel != "other" {
                    tx.execute(
                        &format!(
                            "UPDATE {table} SET channel=?1 WHERE model=?2 AND channel='other'"
                        ),
                        params![channel, model],
                    )?;
                    split.insert(channel);
                }
            }
        }
        let models = {
            let mut stmt = tx.prepare("SELECT id FROM models")?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
            rows.collect::<std::result::Result<Vec<_>, _>>()?
        };
        split.extend(
            models
                .iter()
                .filter(|model| v3_channel(model) == "other")
                .map(|model| channel::classify(model))
                .filter(|channel| channel != "other"),
        );
        let keys = {
            let mut stmt = tx.prepare("SELECT id,policy FROM keys")?;
            let rows = stmt.query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?;
            rows.collect::<std::result::Result<Vec<_>, _>>()?
        };
        for (id, text) in keys {
            let mut policy: Value = serde_json::from_str(&text)?;
            let Some(rules) = policy
                .get_mut("channel_rules")
                .and_then(Value::as_object_mut)
            else {
                continue;
            };
            let Some(other) = rules.get("other").cloned() else {
                continue;
            };
            for channel in &split {
                rules
                    .entry(channel.clone())
                    .or_insert_with(|| other.clone());
            }
            tx.execute(
                "UPDATE keys SET policy=?1 WHERE id=?2",
                params![policy.to_string(), id],
            )?;
        }
        tx.execute_batch("PRAGMA user_version=4;")?;
        tx.commit()?;
    }
    if version < 5 {
        let tx = db.transaction()?;
        tx.execute_batch(
            "ALTER TABLE requests ADD COLUMN legacy_channel TEXT NOT NULL DEFAULT '';
             ALTER TABLE usage ADD COLUMN legacy_channel TEXT NOT NULL DEFAULT '';",
        )?;
        for table in ["requests", "usage"] {
            let models = {
                let mut stmt = tx.prepare(&format!(
                    "SELECT DISTINCT model FROM {table} WHERE channel='antigravity'"
                ))?;
                let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
                rows.collect::<std::result::Result<Vec<_>, _>>()?
            };
            for model in models {
                tx.execute(
                    &format!(
                        "UPDATE {table} SET channel=?1,legacy_channel='antigravity'
                         WHERE model=?2 AND channel='antigravity'"
                    ),
                    params![channel::classify(&model), model],
                )?;
            }
        }
        tx.execute_batch(
            "CREATE INDEX usage_legacy_channel_period ON usage(key_id,legacy_channel,started)
                 WHERE legacy_channel!='';
             CREATE INDEX requests_legacy_channel_review ON requests(key_id,legacy_channel,status)
                 WHERE legacy_channel!='';
             PRAGMA user_version=5;",
        )?;
        tx.commit()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::initialize;
    use rusqlite::Connection;

    #[test]
    fn upgrade_preserves_rules_and_reclassifies_historical_channels() {
        let mut db = Connection::open_in_memory().unwrap();
        initialize(&mut db).unwrap();
        let row = |rid: &str, model: &str, channel: &str| {
            format!("INSERT INTO requests(request_id,model,key_id,requested,trace_id,started,status,channel)
                VALUES('{rid}','{model}','key','{model}','',0,'settled','{channel}');")
        };
        db.execute_batch(&format!(
            "{}{}{}{}{}INSERT INTO models(id) VALUES('qwen3-coder-plus');
            INSERT INTO keys(id,masked,active,policy,recording_since) VALUES
                ('limited','',1,'{{\"rule_mode\":\"channel\",\"channel_rules\":{{\"other\":{{\"access\":\"deny\"}},\"grok\":{{\"access\":\"allow\"}},\"antigravity\":{{\"access\":\"allow\",\"quota\":{{\"unit\":\"tokens\",\"limit\":\"100\"}}}}}}}}',''),
                ('open','',1,'{{\"rule_mode\":\"channel\",\"channel_rules\":{{\"gpt\":{{\"access\":\"deny\"}}}}}}','');
            INSERT INTO usage(id,request_id,key_id,model,started,tokens,cost,detail,failed,stream,channel)
                VALUES('u','c','limited','gemini-3.6-flash-high',123,20,456,'{{}}',0,0,'antigravity');
            DROP INDEX usage_legacy_channel_period;
            DROP INDEX requests_legacy_channel_review;
            ALTER TABLE requests DROP COLUMN legacy_channel;
            ALTER TABLE usage DROP COLUMN legacy_channel;
            PRAGMA user_version=2;",
            row("a", "gemini-3.6-flash-high", "other"),
            row("b", "k3", "other"),
            row("c", "gemini-3.6-flash-high", "antigravity"),
            row("d", "grok-4", "other"),
            row("e", "devin/swe-2", "other"),
        ))
        .unwrap();
        initialize(&mut db).unwrap();
        let channel = |rid: &str| -> String {
            db.query_row(
                "SELECT channel FROM requests WHERE request_id=?1",
                [rid],
                |r| r.get(0),
            )
            .unwrap()
        };
        assert_eq!(channel("a"), "gemini");
        assert_eq!(channel("b"), "kimi");
        assert_eq!(channel("c"), "gemini");
        let legacy: String = db
            .query_row(
                "SELECT legacy_channel FROM requests WHERE request_id='c'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(legacy, "antigravity");
        let usage: (String, String, i64, i64, i64) = db
            .query_row(
                "SELECT channel,legacy_channel,started,tokens,cost FROM usage WHERE id='u'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .unwrap();
        assert_eq!(usage, ("gemini".into(), "antigravity".into(), 123, 20, 456));
        assert_eq!(channel("d"), "grok");
        assert_eq!(channel("e"), "swe");
        let rules = |key: &str| -> serde_json::Value {
            let text: String = db
                .query_row("SELECT policy FROM keys WHERE id=?1", [key], |r| r.get(0))
                .unwrap();
            serde_json::from_str::<serde_json::Value>(&text).unwrap()["channel_rules"].clone()
        };
        let limited = rules("limited");
        assert_eq!(limited["swe"]["access"], "deny");
        assert_eq!(limited["qwen"]["access"], "deny");
        assert_eq!(limited["antigravity"]["quota"]["limit"], "100");
        assert_eq!(
            limited["grok"]["access"], "allow",
            "existing rules are kept"
        );
        assert!(limited.get("gemini").is_none() && limited.get("kimi").is_none());
        assert_eq!(rules("open").as_object().unwrap().len(), 1);
        let version: i64 = db
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(version, 5);
        initialize(&mut db).unwrap();
        let channel: String = db
            .query_row(
                "SELECT channel FROM requests WHERE request_id='c'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(channel, "gemini");
    }
}
