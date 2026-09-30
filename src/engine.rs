use crate::{
    accounting::{self, Detail, Price, Tokens, MAX_VALUE},
    config::{self, Config},
};
use chrono::{DateTime, Local, Utc};
use hmac::{Hmac, Mac};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs, time::Duration};

pub type Result<T> = std::result::Result<T, Fault>;
#[derive(Debug)]
pub struct Fault {
    pub status: u16,
    pub code: &'static str,
    pub message: String,
}
impl Fault {
    pub fn new(status: u16, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
        }
    }
    pub fn json(&self) -> Value {
        json!({"error":{"code": self.code, "message":self.message}})
    }
    fn model_forbidden(model: &str) -> Self {
        Self::new(
            403,
            "model_forbidden",
            format!(
                "请求被拒绝：当前 API 密钥无权调用模型「{model}」。请联系管理员开通该模型权限。"
            ),
        )
    }
}
impl From<String> for Fault {
    fn from(s: String) -> Self {
        Self::new(400, "invalid_config", s)
    }
}
impl From<&str> for Fault {
    fn from(s: &str) -> Self {
        s.to_string().into()
    }
}
impl From<rusqlite::Error> for Fault {
    fn from(_: rusqlite::Error) -> Self {
        Self::new(
            503,
            "storage_unavailable",
            "数据库操作失败，暂时无法校验权限和额度。请联系管理员检查数据目录和磁盘状态。",
        )
    }
}
impl From<serde_json::Error> for Fault {
    fn from(_: serde_json::Error) -> Self {
        Self::new(400, "invalid_json", "数据格式无效")
    }
}

fn default_period() -> String {
    "day".into()
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Quota {
    pub unit: String,
    pub limit: String,
}
impl Quota {
    pub fn amount(&self) -> Result<i64> {
        match self.unit.as_str() {
            "money" => Ok(accounting::micros(&self.limit)?),
            "tokens" => {
                if self.limit.is_empty() || !self.limit.bytes().all(|b| b.is_ascii_digit()) {
                    return Err("Token 额度必须是非负整数".into());
                }
                self.limit
                    .parse::<i64>()
                    .ok()
                    .filter(|n| *n <= MAX_VALUE)
                    .ok_or_else(|| "Token 额度超出范围".into())
            }
            _ => Err("额度单位必须是 tokens 或 money".into()),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    pub access: String,
    #[serde(default)]
    pub quota: Option<Quota>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    #[serde(default)]
    pub revision: i64,
    #[serde(default)]
    pub note: String,
    #[serde(default = "default_period")]
    pub period: String,
    #[serde(default)]
    pub rules: BTreeMap<String, Rule>,
}
impl Default for Policy {
    fn default() -> Self {
        Self {
            revision: 0,
            note: String::new(),
            period: default_period(),
            rules: BTreeMap::new(),
        }
    }
}
#[derive(Clone)]
struct Key {
    id: String,
    raw: String,
}

pub struct Engine {
    pub config: Config,
    pub db: Connection,
    secret: Vec<u8>,
    keys: BTreeMap<String, Key>,
    source_hash: String,
    pub aliases: BTreeMap<String, Vec<String>>,
    pub source_error: Option<String>,
    pub health_error: Option<String>,
    pub price_sync: Option<crate::price_sync::Worker>,
}

impl Engine {
    pub fn open(config: Config) -> Result<Self> {
        config.validate()?;
        fs::create_dir_all(&config.data_dir)
            .map_err(|_| Fault::new(503, "storage_unavailable", "无法创建数据目录"))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&config.data_dir, fs::Permissions::from_mode(0o700))
                .map_err(|_| Fault::new(503, "storage_unavailable", "无法设置数据目录权限"))?;
        }
        let db = Connection::open(config.data_dir.join("manager.sqlite3"))?;
        db.busy_timeout(Duration::from_secs(5))?;
        db.execute_batch(include_str!("schema.sql"))?;
        let existing: Option<String> = db
            .query_row("SELECT value FROM meta WHERE name='secret'", [], |r| {
                r.get(0)
            })
            .optional()?;
        let secret = existing.unwrap_or_else(|| {
            format!(
                "{}{}",
                uuid::Uuid::new_v4().simple(),
                uuid::Uuid::new_v4().simple()
            )
        });
        db.execute("INSERT OR IGNORE INTO meta VALUES('secret',?1)", [&secret])?;
        db.execute(
            "INSERT OR IGNORE INTO meta VALUES('recording_since',?1)",
            [Utc::now().to_rfc3339()],
        )?;
        db.execute(
            "UPDATE requests SET status='review' WHERE status='pending'",
            [],
        )?;
        let mut engine = Self {
            config,
            db,
            secret: secret.into_bytes(),
            keys: BTreeMap::new(),
            aliases: BTreeMap::new(),
            source_hash: String::new(),
            source_error: None,
            health_error: None,
            price_sync: None,
        };
        // Keep the plugin active on a broken source, so interceptions can return 503.
        let _ = engine.sync();
        let previous: Option<String> = engine
            .db
            .query_row(
                "SELECT value FROM meta WHERE name='price_sync_status'",
                [],
                |row| row.get(0),
            )
            .optional()?;
        let status = previous
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        engine.price_sync = Some(crate::price_sync::Worker::start(
            engine.config.clone(),
            status,
        ));
        Ok(engine)
    }
    fn key_id(&self, key: &str) -> String {
        let mut hmac =
            Hmac::<Sha256>::new_from_slice(&self.secret).expect("HMAC accepts any key length");
        hmac.update(key.trim().as_bytes());
        hex::encode(hmac.finalize().into_bytes())
    }
    pub fn timezone(&self) -> Local {
        Local
    }
    pub fn sync(&mut self) -> Result<()> {
        match self.sync_inner() {
            Ok(()) => {
                self.source_error = None;
                Ok(())
            }
            Err(error) => {
                self.source_error = Some(error.message.clone());
                Err(error)
            }
        }
    }
    fn sync_inner(&mut self) -> Result<()> {
        let source = config::read_source(&self.config)
            .map_err(|m| Fault::new(503, "key_source_unavailable", m))?;
        let digest = hex::encode(Sha256::digest(serde_json::to_vec(&(
            &source.keys,
            &source.aliases,
            &source.models,
        ))?));
        if digest == self.source_hash {
            return Ok(());
        }
        let keys: Vec<_> = source
            .keys
            .iter()
            .map(|raw| {
                (
                    config::caller_scope(raw),
                    Key {
                        id: self.key_id(raw),
                        raw: raw.clone(),
                    },
                )
            })
            .collect();
        let tx = self.db.transaction()?;
        tx.execute("UPDATE keys SET active=0", [])?;
        for (_, key) in &keys {
            let chars: Vec<_> = key.raw.chars().collect();
            let masked = if chars.len() > 8 {
                format!(
                    "{}••••{}",
                    chars[..3].iter().collect::<String>(),
                    chars[chars.len() - 4..].iter().collect::<String>()
                )
            } else {
                "••••••••".into()
            };
            tx.execute(
                "INSERT INTO keys(id,masked,active,policy) VALUES(?1,?2,1,?3)
                ON CONFLICT(id) DO UPDATE SET masked=excluded.masked,active=1",
                params![key.id, masked, serde_json::to_string(&Policy::default())?],
            )?;
        }
        for model in source.models {
            tx.execute("INSERT OR IGNORE INTO models(id) VALUES(?1)", [model])?;
        }
        tx.commit()?;
        self.keys = keys.into_iter().collect();
        self.aliases = source.aliases;
        self.source_hash = digest;
        self.schedule_price_sync();
        Ok(())
    }
    pub fn schedule_price_sync(&self) {
        if let Some(worker) = &self.price_sync {
            let _ = worker.request(false);
        }
    }
    pub fn request_price_sync(&self) -> Result<Value> {
        let worker = self.price_sync.as_ref().ok_or_else(|| {
            Fault::new(
                503,
                "price_sync_unavailable",
                "价格同步服务尚未启动，请重启插件",
            )
        })?;
        worker
            .request(true)
            .map_err(|message| Fault::new(503, "price_sync_unavailable", message))?;
        Ok(json!({"queued":true,"price_sync":worker.status()}))
    }
    pub fn policy(&self, key: &str) -> Result<Policy> {
        let text: Option<String> = self
            .db
            .query_row("SELECT policy FROM keys WHERE id=?1", [key], |r| r.get(0))
            .optional()?;
        decode_policy(&text.ok_or_else(|| Fault::new(404, "key_not_found", "密钥不存在"))?)
    }
    pub fn price(&self, model: &str) -> Result<Option<Price>> {
        let text: Option<String> = self
            .db
            .query_row("SELECT price FROM prices WHERE model=?1", [model], |r| {
                r.get(0)
            })
            .optional()?;
        text.map(|t| serde_json::from_str(&t).map_err(Fault::from))
            .transpose()
    }
    fn canonical(&self, model: &str) -> String {
        match self.aliases.get(model) {
            Some(targets) if targets.len() == 1 => targets[0].clone(),
            _ => model.to_string(),
        }
    }
    pub fn totals(&self, key: &str, model: &str, start: i64, end: i64) -> Result<Value> {
        let (tokens, cost, unpriced, count): (i64, i64, i64, i64) = self.db.query_row(
            "SELECT COALESCE(SUM(tokens),0),COALESCE(SUM(cost),0),
             COALESCE(SUM(CASE WHEN cost IS NULL THEN 1 ELSE 0 END),0),COUNT(*)
             FROM usage WHERE key_id=?1 AND model=?2 AND started>=?3 AND started<?4",
            params![key, model, start, end],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )?;
        let pending: i64 = self.db.query_row(
            "SELECT COUNT(*) FROM requests WHERE key_id=?1 AND model=?2 AND started>=?3 AND started<?4 AND status IN ('pending','review')",
            params![key, model, start, end], |r| r.get(0))?;
        let review: i64 = self.db.query_row(
            "SELECT COUNT(*) FROM requests WHERE key_id=?1 AND model=?2 AND status='review'",
            params![key, model],
            |r| r.get(0),
        )?;
        Ok(
            json!({"tokens":tokens.to_string(),"cost":accounting::money(cost),"cost_micros":cost,
            "unpriced":unpriced,"requests":count,"pending":pending,"review":review}),
        )
    }
    pub fn save_policy(
        &mut self,
        key: &str,
        mut policy: Policy,
        reprice: bool,
        now: DateTime<Utc>,
    ) -> Result<Value> {
        let active: bool = self
            .db
            .query_row("SELECT active FROM keys WHERE id=?1", [key], |r| r.get(0))
            .optional()?
            .unwrap_or(false);
        if !active {
            return Err(Fault::new(
                409,
                "key_inactive",
                "该密钥已从宿主删除或不存在",
            ));
        }
        let old = self.policy(key)?;
        if policy.revision != old.revision {
            return Err(Fault::new(
                409,
                "revision_conflict",
                "配置已被其他页面修改，请刷新后重试",
            ));
        }
        if !["day", "week", "month"].contains(&policy.period.as_str())
            || policy.note.chars().count() > 100
        {
            return Err("密钥周期必须是 day、week 或 month，备注不能超过 100 字".into());
        }
        if policy.rules.len() > 1000 {
            return Err("单个密钥最多配置 1000 条模型规则".into());
        }
        let mut canonical_rules = BTreeMap::new();
        let mut updates: Vec<(String, i64)> = Vec::new();
        let mut preview_cost = 0_i64;
        for (name, rule) in &policy.rules {
            validate_model(name)?;
            if !["allow", "deny"].contains(&rule.access.as_str()) {
                return Err("模型权限必须是 allow 或 deny".into());
            }
            let model = self.canonical(name);
            if canonical_rules.contains_key(&model) {
                return Err("同一个实际模型存在重复规则，请合并别名配置".into());
            }
            if let Some(quota) = &rule.quota {
                quota.amount()?;
                if self.aliases.get(name).is_some_and(|v| v.len() > 1) {
                    return Err("该别名对应多个实际模型，请分别配置目标模型的额度".into());
                }
                if quota.unit == "money" {
                    let price = self.price(&model)?.ok_or("请先为该模型设置价格")?;
                    let (start, end) =
                        accounting::period_bounds(now, self.timezone(), &policy.period)?;
                    let unpriced_pending: i64 = self.db.query_row("SELECT COUNT(*) FROM requests
                        WHERE key_id=?1 AND model=?2 AND started>=?3 AND started<?4 AND price IS NULL AND status IN ('pending','review')",
                        params![key,model,start,end], |r| r.get(0))?;
                    if unpriced_pending > 0 {
                        return Err(Fault::new(
                            409,
                            "pending_usage",
                            "仍有未计价的执行中或待核对请求，处理后才能启用金额额度",
                        ));
                    }
                    let mut stmt = self.db.prepare("SELECT id,detail FROM usage WHERE key_id=?1 AND model=?2 AND started>=?3 AND started<?4 AND cost IS NULL")?;
                    let rows = stmt.query_map(params![key, model, start, end], |r| {
                        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
                    })?;
                    for row in rows {
                        let (id, detail) = row?;
                        let tokens: Tokens = serde_json::from_str(&detail)?;
                        let cost = price.cost(&tokens)?;
                        preview_cost =
                            preview_cost.checked_add(cost).ok_or("补计总金额超出范围")?;
                        updates.push((id, cost));
                    }
                }
            }
            canonical_rules.insert(model, rule.clone());
        }
        if !updates.is_empty() && !reprice {
            return Err(Fault::new(
                409,
                "reprice_required",
                format!(
                    "当前周期有 {} 条未计价记录，按当前单价补计 {} USD。确认补计后才能保存。",
                    updates.len(),
                    accounting::money(preview_cost)
                ),
            ));
        }
        policy.rules = canonical_rules;
        policy.revision = old.revision + 1;
        let tx = self.db.transaction()?;
        for (id, cost) in updates {
            tx.execute(
                "UPDATE usage SET cost=?1 WHERE id=?2 AND cost IS NULL",
                params![cost, id],
            )?;
            tx.execute(
                "INSERT INTO audit(created,action,subject,detail) VALUES(?1,'reprice',?2,?3)",
                params![now.timestamp_millis(), id, cost.to_string()],
            )?;
        }
        for model in policy.rules.keys() {
            tx.execute("INSERT OR IGNORE INTO models(id) VALUES(?1)", [model])?;
        }
        tx.execute(
            "UPDATE keys SET policy=?1 WHERE id=?2",
            params![serde_json::to_string(&policy)?, key],
        )?;
        tx.commit()?;
        Ok(serde_json::to_value(policy)?)
    }
    pub fn save_price(&mut self, model: &str, price: Price, revision: i64) -> Result<Value> {
        validate_model(model)?;
        price.validate()?;
        let model = self.canonical(model);
        let tx = self
            .db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let current: i64 = tx
            .query_row(
                "SELECT revision FROM prices WHERE model=?1",
                [&model],
                |r| r.get(0),
            )
            .optional()?
            .unwrap_or(0);
        if current != revision {
            return Err(Fault::new(
                409,
                "revision_conflict",
                "模型单价已变更，请刷新后重试",
            ));
        }
        tx.execute("INSERT OR IGNORE INTO models(id) VALUES(?1)", [&model])?;
        tx.execute("INSERT INTO prices VALUES(?1,?2,?3) ON CONFLICT(model) DO UPDATE SET price=excluded.price,revision=excluded.revision",
            params![model, serde_json::to_string(&price)?, current + 1])?;
        tx.execute("DELETE FROM price_origins WHERE model=?1", [&model])?;
        tx.commit()?;
        Ok(json!({"model":model,"price":price,"revision":current+1}))
    }
    fn caller(&self, request: &Value) -> Result<String> {
        // caller_scope is generated by CPA from its authenticated userApiKey.
        // Do not identify the client from after-auth headers, which contain upstream credentials.
        let scope = request
            .pointer("/Metadata/caller_scope")
            .and_then(Value::as_str)
            .unwrap_or("");
        self.keys.get(scope).map(|k| k.id.clone()).ok_or_else(|| {
            Fault::new(
                403,
                "unknown_api_key",
                "请求被拒绝：当前 API 密钥无法识别或已从宿主配置中移除。请检查所用密钥，或联系管理员确认配置。",
            )
        })
    }
    fn permitted(policy: &Policy, requested: &str, actual: &str) -> bool {
        let rules: Vec<_> = [requested, actual]
            .iter()
            .filter_map(|m| policy.rules.get(*m))
            .collect();
        if rules.iter().any(|r| r.access == "deny") {
            return false;
        }
        true
    }
    pub fn intercept(&mut self, req: &Value, after: bool, now: DateTime<Utc>) -> Result<Value> {
        self.sync()?;
        if self.health_error.is_some() {
            return Err(Fault::new(
                503,
                "accounting_unavailable",
                "请求被拒绝：用量记账服务异常，暂时无法核验额度。请联系管理员检查数据库和磁盘状态，修复后重启插件。",
            ));
        }
        let key = self.caller(req)?;
        let requested = string(req, "RequestedModel")
            .or_else(|| string(req, "Model"))
            .ok_or_else(|| {
                Fault::new(
                    400,
                    "missing_model",
                    "请求被拒绝：未指定模型。请在请求中填写 model 字段。",
                )
            })?;
        if req
            .pointer("/Metadata/request_path")
            .and_then(Value::as_str)
            .is_some_and(|p| p.ends_with("/count_tokens"))
        {
            // Token counting has no generation cost, but still enforces model permissions.
            let policy = self.policy(&key)?;
            if !Self::permitted(&policy, requested, &self.canonical(requested)) {
                return Err(Fault::model_forbidden(&self.canonical(requested)));
            }
            return Ok(json!({}));
        }
        let actual = if after {
            string(req, "Model").unwrap_or(requested).to_string()
        } else {
            self.canonical(requested)
        };
        let policy = self.policy(&key)?;
        if !Self::permitted(&policy, requested, &actual) {
            return Err(Fault::model_forbidden(&actual));
        }
        if !after {
            return Ok(json!({}));
        }
        let added = self
            .db
            .execute("INSERT OR IGNORE INTO models(id) VALUES(?1)", [&actual])?;
        if added == 1 {
            self.schedule_price_sync();
        }
        // An unconfigured model passes through and creates no pending accounting obligation.
        if policy
            .rules
            .get(&actual)
            .and_then(|rule| rule.quota.as_ref())
            .is_none()
        {
            return Ok(json!({}));
        }
        self.expire_pending(now)?;
        let review: i64 = self.db.query_row(
            "SELECT COUNT(*) FROM requests WHERE key_id=?1 AND model=?2 AND status='review'",
            params![key, actual],
            |r| r.get(0),
        )?;
        if review > 0 {
            return Err(Fault::new(
                503,
                "usage_needs_review",
                format!("请求被拒绝：模型「{actual}」有 {review} 个请求的用量待核对，已暂停当前密钥对该模型的调用。请联系管理员在「API 密钥 → 对应模型 → 处理待核对」中完成核对后重试。"),
            ));
        }
        if let Some(quota) = policy.rules.get(&actual).and_then(|r| r.quota.as_ref()) {
            let timezone = self.timezone();
            let timezone_label = server_timezone_name();
            let (start, end) = accounting::period_bounds(now, timezone, &policy.period)?;
            let totals = self.totals(&key, &actual, start, end)?;
            let used = if quota.unit == "money" {
                if self.price(&actual)?.is_none() {
                    return Err(Fault::new(
                        503,
                        "unpriced_usage",
                        format!("请求被拒绝：模型「{actual}」尚未设置价格，无法核算金额额度。请联系管理员在「模型价格」中设置价格后重试。"),
                    ));
                }
                let unpriced = totals["unpriced"].as_i64().unwrap_or(0);
                if unpriced > 0 {
                    return Err(Fault::new(
                        503,
                        "unpriced_usage",
                        format!("请求被拒绝：模型「{actual}」当前周期有 {unpriced} 条用量记录尚未计价。请联系管理员确认补计费用后重试。"),
                    ));
                }
                totals["cost_micros"].as_i64().unwrap_or(0)
            } else {
                totals["tokens"]
                    .as_str()
                    .unwrap_or("0")
                    .parse::<i64>()
                    .unwrap_or(i64::MAX)
            };
            let limit = quota.amount()?;
            if used >= limit {
                let reset = DateTime::from_timestamp_millis(end).ok_or("额度重置时间无效")?;
                let period = match policy.period.as_str() {
                    "day" => "今日",
                    "week" => "本周",
                    _ => "本月",
                };
                let (unit, label, used_text, limit_text) = if quota.unit == "money" {
                    (
                        "美元",
                        "金额",
                        accounting::money(used),
                        accounting::money(limit),
                    )
                } else {
                    ("Token", "Token", used.to_string(), limit.to_string())
                };
                let message = format!(
                    "请求被拒绝：模型「{actual}」{period}的{label}额度已耗尽（已用 {used_text} {unit}，上限 {limit_text} {unit}）。额度将于 {}（服务器时区：{timezone_label}）重置，请等待重置或联系管理员提高额度。",
                    reset.with_timezone(&timezone).format("%Y-%m-%d %H:%M:%S")
                );
                let body = json!({"error":{"code":"quota_exceeded","message":message,
                    "model":actual,"unit":quota.unit,"period":policy.period,
                    "used":used_text,"limit":limit_text,"reset_at":reset.to_rfc3339(),
                    "timezone":timezone_label}});
                let mut response = crate::web::rejection(429, &body);
                response["ResponseHeaders"]["Retry-After"] =
                    json!([((end - now.timestamp_millis() + 999) / 1000)
                        .max(1)
                        .to_string()]);
                return Ok(response);
            }
        }
        let rid = string(req, "RequestID").ok_or_else(|| {
            Fault::new(
                503,
                "request_context_unavailable",
                "请求被拒绝：宿主未提供请求标识，暂时无法记录用量。请联系管理员检查宿主与插件的兼容性。",
            )
        })?;
        let price = self
            .price(&actual)?
            .map(|p| serde_json::to_string(&p))
            .transpose()?;
        self.db.execute("INSERT OR IGNORE INTO requests(request_id,model,key_id,requested,trace_id,started,status,price)
            VALUES(?1,?2,?3,?4,?5,?6,'pending',?7)",
            params![rid,actual,key,requested,string(req,"TraceID").unwrap_or(""),now.timestamp_millis(),price])?;
        Ok(json!({}))
    }
    pub fn complete(&mut self, req: &Value, now: DateTime<Utc>) -> Result<()> {
        let rid = string(req, "RequestID").ok_or("完成事件缺少 RequestID")?;
        let outcome = string(req, "Outcome").unwrap_or("unknown");
        self.db.execute(
            "UPDATE requests SET completed=?1,outcome=?2 WHERE request_id=?3",
            params![now.timestamp_millis(), outcome, rid],
        )?;
        Ok(())
    }
    pub fn expire_pending(&mut self, now: DateTime<Utc>) -> Result<()> {
        self.db.execute("UPDATE requests SET status='review' WHERE status='pending' AND completed IS NOT NULL AND completed<?1",
            [now.timestamp_millis()-30_000])?;
        Ok(())
    }
    pub fn usage(&mut self, record: &Value, now: DateTime<Utc>) -> Result<()> {
        if record.get("Generate") == Some(&Value::Bool(false)) {
            return Ok(());
        }
        let rid = string(record, "RequestID").ok_or("用量事件缺少 RequestID")?;
        let model = string(record, "Model").ok_or("用量事件缺少 Model")?;
        let trace = string(record, "TraceID").unwrap_or("");
        let raw_key = string(record, "APIKey").unwrap_or("");
        let fallback_key = if raw_key.is_empty() {
            None
        } else {
            Some(self.key_id(raw_key))
        };
        let requested_at = string(record, "RequestedAt")
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|t| t.timestamp_millis())
            .unwrap_or(now.timestamp_millis());
        let exact: Option<(String,i64,Option<String>,String)> = self.db.query_row(
            "SELECT key_id,started,price,request_id FROM requests WHERE request_id=?1 AND model=?2",
            params![rid,model], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?;
        let pending = match exact {
            Some(value) => Some(value),
            None if !trace.is_empty() && fallback_key.is_some() => self.db.query_row(
                "SELECT key_id,started,price,request_id FROM requests WHERE trace_id=?1 AND model=?2 AND key_id=?3
                 AND ABS(started-?4)<86400000 ORDER BY started DESC LIMIT 1",
                params![trace,model,fallback_key,requested_at], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?,
            _ => None,
        };
        let (key, started, price, parent_id) = match pending {
            Some(pending) => pending,
            None => {
                let Some(key) = fallback_key else {
                    return Ok(());
                };
                let exists: bool = self.db.query_row(
                    "SELECT EXISTS(SELECT 1 FROM keys WHERE id=?1)",
                    [&key],
                    |r| r.get(0),
                )?;
                // Usage for requests admitted before plugin installation is not ours to charge.
                if !exists
                    || !self
                        .policy(&key)?
                        .rules
                        .get(model)
                        .is_some_and(|rule| rule.access == "allow" && rule.quota.is_some())
                {
                    return Ok(());
                }
                (key, requested_at, None, rid.to_string())
            }
        };
        let detail: Detail =
            serde_json::from_value(record.get("Detail").cloned().unwrap_or(json!({})))?;
        let mut tokens = accounting::normalize(
            &detail,
            string(record, "Provider").unwrap_or(""),
            string(record, "ExecutorType").unwrap_or(""),
        )?;
        // CPA may emit an empty detail when upstream never supplied usage. Do not
        // silently interpret an unmeasured generation as free.
        if tokens.total == 0 && !record["Failed"].as_bool().unwrap_or(false) {
            tokens.complete = false;
        }
        let price: Option<Price> = price.map(|p| serde_json::from_str(&p)).transpose()?;
        let cost = price
            .as_ref()
            .map(|p| p.cost(&tokens))
            .transpose()
            .ok()
            .flatten();
        let id = hex::encode(Sha256::digest(
            format!(
                "{rid}\0{model}\0{}\0{}\0{}",
                string(record, "Provider").unwrap_or(""),
                string(record, "AuthID").unwrap_or(""),
                string(record, "RequestedAt").unwrap_or("")
            )
            .as_bytes(),
        ));
        let tx = self.db.transaction()?;
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM usage WHERE id=?1)",
            [&id],
            |r| r.get(0),
        )?;
        if exists {
            return Ok(());
        }
        // A late authoritative record replaces a manual settlement, not a second debit.
        tx.execute(
            "DELETE FROM usage WHERE request_id=?1 AND model=?2 AND manual=1",
            params![parent_id, model],
        )?;
        tx.execute("INSERT INTO usage(id,request_id,key_id,model,started,tokens,cost,detail,failed,stream,manual)
            VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,0)", params![id,parent_id,key,model,started,tokens.total,cost,
                serde_json::to_string(&tokens)?,record["Failed"].as_bool().unwrap_or(false),record["Stream"].as_bool().unwrap_or(false)])?;
        tx.execute(
            "UPDATE requests SET status=?1 WHERE request_id=?2 AND model=?3",
            params![
                if tokens.complete { "settled" } else { "review" },
                parent_id,
                model
            ],
        )?;
        tx.execute("INSERT OR IGNORE INTO models(id) VALUES(?1)", [model])?;
        tx.commit()?;
        Ok(())
    }
    pub fn settle(
        &mut self,
        rid: &str,
        model: &str,
        tokens: Tokens,
        reason: &str,
        now: DateTime<Utc>,
    ) -> Result<()> {
        if reason.trim().chars().count() < 4 || reason.chars().count() > 500 {
            return Err("核对原因需填写 4～500 字".into());
        }
        if !tokens.complete
            || [
                tokens.input,
                tokens.output,
                tokens.cache_read,
                tokens.cache_write,
                tokens.total,
            ]
            .iter()
            .any(|n| *n < 0 || *n > MAX_VALUE)
            || tokens.input + tokens.output + tokens.cache_read + tokens.cache_write != tokens.total
        {
            return Err("Token 明细必须非负且合计一致".into());
        }
        let row: Option<(String, i64, Option<String>)> = self
            .db
            .query_row(
                "SELECT key_id,started,price FROM requests
            WHERE request_id=?1 AND model=?2 AND status='review'",
                params![rid, model],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        let (key, started, price) = row.ok_or("该请求不在待核对状态")?;
        let price = price
            .map(|p| serde_json::from_str::<Price>(&p))
            .transpose()?
            .or(self.price(model)?);
        let cost = price.map(|p| p.cost(&tokens)).transpose()?;
        let tx = self.db.transaction()?;
        // Reconciliation explicitly replaces this request's existing incomplete totals.
        tx.execute(
            "DELETE FROM usage WHERE request_id=?1 AND model=?2",
            params![rid, model],
        )?;
        tx.execute("INSERT INTO usage(id,request_id,key_id,model,started,tokens,cost,detail,failed,stream,manual)
            VALUES(?1,?2,?3,?4,?5,?6,?7,?8,0,0,1)",
            params![format!("manual:{rid}:{model}"),rid,key,model,started,tokens.total,cost,serde_json::to_string(&tokens)?])?;
        tx.execute(
            "UPDATE requests SET status='settled' WHERE request_id=?1 AND model=?2",
            params![rid, model],
        )?;
        tx.execute(
            "INSERT INTO audit(created,action,subject,detail) VALUES(?1,'settle',?2,?3)",
            params![now.timestamp_millis(), rid, reason],
        )?;
        tx.commit()?;
        Ok(())
    }
    pub fn snapshot(&mut self, now: DateTime<Utc>) -> Result<Value> {
        let _ = self.sync();
        self.expire_pending(now)?;
        let zone = self.timezone();
        let mut keys = Vec::new();
        let mut stmt = self
            .db
            .prepare("SELECT id,masked,active,policy FROM keys ORDER BY rowid")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, bool>(2)?,
                r.get::<_, String>(3)?,
            ))
        })?;
        for row in rows {
            let (id, masked, active, p) = row?;
            let policy = decode_policy(&p)?;
            let (_, period_end) = accounting::period_bounds(now, zone, &policy.period)?;
            let mut quotas = serde_json::Map::new();
            for (model, rule) in &policy.rules {
                if rule.quota.is_some() {
                    let (start, end) = accounting::period_bounds(now, zone, &policy.period)?;
                    let mut totals = self.totals(&id, model, start, end)?;
                    totals["reset_at"] = json!(DateTime::from_timestamp_millis(end)
                        .unwrap()
                        .with_timezone(&zone)
                        .to_rfc3339());
                    quotas.insert(model.clone(), totals);
                }
            }
            keys.push(
                json!({"id":id,"masked":masked,"active":active,"policy":policy,"quotas":quotas,
                    "period_reset_at":DateTime::from_timestamp_millis(period_end).map(|t| t.with_timezone(&zone).to_rfc3339())}),
            );
        }
        let mut saved_prices = BTreeMap::new();
        let mut stmt = self.db.prepare(
            "SELECT p.model,p.price,p.revision,o.source,o.source_model,o.synced_at
             FROM prices p LEFT JOIN price_origins o ON o.model=p.model",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, Option<String>>(5)?,
            ))
        })?;
        for row in rows {
            let (id, price, revision, source, source_model, synced_at) = row?;
            let price: Price = serde_json::from_str(&price)?;
            saved_prices.insert(id, (price, revision, source, source_model, synced_at));
        }
        let mut models = Vec::new();
        let mut stmt = self.db.prepare("SELECT id FROM models ORDER BY id")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        for row in rows {
            let id = row?;
            let price_model = self.canonical(&id);
            let entry = saved_prices.get(&price_model);
            models.push(json!({
                "id":id,"price_model":price_model,
                "price":entry.map(|e| &e.0),"revision":entry.map(|e| e.1).unwrap_or(0),
                "price_source":entry.map(|e| e.2.as_deref().unwrap_or("manual")),
                "price_source_model":entry.and_then(|e| e.3.as_deref()),
                "price_synced_at":entry.and_then(|e| e.4.as_deref()),
                "targets":self.aliases.get(&id)
            }));
        }
        let recording_since: String = self.db.query_row(
            "SELECT value FROM meta WHERE name='recording_since'",
            [],
            |r| r.get(0),
        )?;
        let reviews: i64 = self.db.query_row(
            "SELECT COUNT(*) FROM requests WHERE status='review'",
            [],
            |r| r.get(0),
        )?;
        Ok(
            json!({"version":env!("CARGO_PKG_VERSION"),"timezone":server_timezone_name(),
            "server_offset_seconds":now.with_timezone(&Local).offset().local_minus_utc(),
            "timezone_source":"server","currency":"USD",
            "source_error":self.source_error,"health_error":self.health_error,"recording_since":recording_since,
            "review_count":reviews,"keys":keys,"models":models,
            "price_sync":self.price_sync.as_ref().map(|worker| worker.status()),
            "quota_mode":"settled_usage","server_time":now.to_rfc3339()}),
        )
    }
    pub fn ledger(
        &mut self,
        key: &str,
        model: &str,
        offset: i64,
        reviews: bool,
        now: DateTime<Utc>,
    ) -> Result<Value> {
        self.expire_pending(now)?;
        let mut items = Vec::new();
        if reviews {
            let mut stmt = self.db.prepare("SELECT request_id,model,key_id,started,status,outcome FROM requests
                WHERE status='review' AND (?1='' OR key_id=?1) AND (?2='' OR model=?2) ORDER BY started DESC LIMIT 100 OFFSET ?3")?;
            let rows = stmt.query_map(params![key,model,offset.max(0)], |r| Ok(json!({"request_id":r.get::<_,String>(0)?,
                "model":r.get::<_,String>(1)?,"key_id":r.get::<_,String>(2)?,"started":r.get::<_,i64>(3)?,
                "status":r.get::<_,String>(4)?,"outcome":r.get::<_,Option<String>>(5)?})))?;
            for row in rows {
                items.push(row?);
            }
        } else {
            let mut stmt = self.db.prepare("SELECT request_id,model,key_id,started,tokens,cost,detail,failed,stream,manual FROM usage
                WHERE (?1='' OR key_id=?1) AND (?2='' OR model=?2) ORDER BY started DESC LIMIT 100 OFFSET ?3")?;
            let rows = stmt.query_map(params![key, model, offset.max(0)], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, i64>(4)?,
                    r.get::<_, Option<i64>>(5)?,
                    r.get::<_, String>(6)?,
                    r.get::<_, bool>(7)?,
                    r.get::<_, bool>(8)?,
                    r.get::<_, bool>(9)?,
                ))
            })?;
            for row in rows {
                let (rid, model, key, started, tokens, cost, detail, failed, stream, manual) = row?;
                items.push(json!({"request_id":rid,"model":model,"key_id":key,"started":started,"tokens":tokens.to_string(),
                    "cost":cost.map(accounting::money),"detail":serde_json::from_str::<Value>(&detail)?,
                    "failed":failed,"stream":stream,"manual":manual}));
            }
        }
        Ok(json!({"items":items,"offset":offset.max(0),"limit":100}))
    }
    pub fn sync_models(&mut self, key_id: &str) -> Result<usize> {
        self.sync()?;
        let key = self
            .keys
            .values()
            .find(|k| k.id == key_id)
            .ok_or("请选择有效的客户端密钥")?;
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(8))
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .build()
            .map_err(|_| Fault::new(502, "model_sync_failed", "无法建立宿主连接"))?;
        let response = client
            .get(format!(
                "{}/v1/models",
                self.config.cpa_base_url.trim_end_matches('/')
            ))
            .bearer_auth(&key.raw)
            .send()
            .and_then(|r| r.error_for_status())
            .map_err(|_| {
                Fault::new(
                    502,
                    "model_sync_failed",
                    "读取宿主模型列表失败，请检查地址和密钥",
                )
            })?;
        // Bound responses before JSON parsing.
        use std::io::Read;
        let mut body = Vec::new();
        response
            .take(4 * 1024 * 1024 + 1)
            .read_to_end(&mut body)
            .map_err(|_| "读取模型列表失败")?;
        if body.len() > 4 * 1024 * 1024 {
            return Err("模型列表超过大小限制".into());
        }
        let doc: Value = serde_json::from_slice(&body)?;
        let models = doc["data"].as_array().ok_or("宿主模型列表格式无效")?;
        let tx = self.db.transaction()?;
        let mut count = 0;
        for model in models {
            if let Some(id) = model["id"].as_str() {
                validate_model(id)?;
                tx.execute("INSERT OR IGNORE INTO models(id) VALUES(?1)", [id])?;
                count += 1;
            }
        }
        tx.commit()?;
        self.schedule_price_sync();
        Ok(count)
    }
}
pub fn string<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
}
pub fn server_timezone_name() -> String {
    iana_time_zone::get_timezone().unwrap_or_else(|_| format!("UTC{}", Local::now().offset()))
}

fn decode_policy(text: &str) -> Result<Policy> {
    let mut value: Value = serde_json::from_str(text)?;
    let object = value.as_object_mut().ok_or("已保存的密钥规则格式无效")?;
    object.remove("default_access");
    let mut legacy_periods = std::collections::BTreeSet::new();
    if let Some(rules) = object.get_mut("rules").and_then(Value::as_object_mut) {
        for rule in rules.values_mut() {
            if let Some(quota) = rule.get_mut("quota").and_then(Value::as_object_mut) {
                if let Some(Value::String(period)) = quota.remove("period") {
                    if ["day", "week", "month"].contains(&period.as_str()) {
                        legacy_periods.insert(period);
                    }
                }
            }
        }
    }
    if !object.contains_key("period") {
        // Preserve a uniform legacy cycle. A mixed legacy configuration chooses the
        // broadest window rather than silently granting a fresh shorter allowance.
        let period = if legacy_periods.len() == 1 {
            legacy_periods
                .into_iter()
                .next()
                .unwrap_or_else(default_period)
        } else if legacy_periods.is_empty() {
            default_period()
        } else {
            "month".into()
        };
        object.insert("period".into(), json!(period));
    }
    Ok(serde_json::from_value(value)?)
}
pub fn validate_model(model: &str) -> Result<()> {
    if model.is_empty()
        || model.len() > 256
        || model.trim() != model
        || model.chars().any(char::is_control)
    {
        return Err("模型 ID 必须为 1～256 字节，且不含首尾空白或控制字符".into());
    }
    Ok(())
}
