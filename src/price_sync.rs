//! A joinable background worker keeps public-price network I/O off request hooks.
use crate::{
    accounting::Price,
    config::{self, Config},
    price_catalog::{Catalog, Entry, Source, RULES_VERSION},
};
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Read,
    sync::{Arc, Condvar, Mutex, MutexGuard},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const MAX_CATALOG_BYTES: usize = 32 * 1024 * 1024;
/// How often requests for one model without a price may look it up again.
const MISSING_RETRY: Duration = Duration::from_secs(3600);
/// Such a lookup downloads catalogs again once they are older than this.
const MISSING_REFRESH: Duration = Duration::from_secs(600);

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct SourceStatus {
    pub source: String,
    pub matched: usize,
    pub error: Option<String>,
}

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Status {
    pub last_attempt: Option<String>,
    pub last_success: Option<String>,
    pub added: usize,
    pub updated: usize,
    pub unmatched: Vec<String>,
    pub sources: Vec<SourceStatus>,
    pub error: Option<String>,
}

struct Control {
    config: Config,
    generation: u64,
    pending: bool,
    force: bool,
    refresh: bool,
    running: bool,
    stopped: bool,
    status: Status,
    missing: BTreeMap<String, Instant>,
}
type Shared = Arc<(Mutex<Control>, Condvar, Mutex<()>)>;
struct Cached {
    url: String,
    fetched: Instant,
    catalog: Result<Catalog, String>,
}
type Cache = BTreeMap<Source, Cached>;

pub struct Worker {
    shared: Shared,
    handle: Option<JoinHandle<()>>,
}
pub struct Pause<'a> {
    control: MutexGuard<'a, Control>,
    _write: MutexGuard<'a, ()>,
    wake: &'a Condvar,
    resume: bool,
}
impl Drop for Pause<'_> {
    fn drop(&mut self) {
        self.control.pending = !self.control.stopped && self.resume;
        self.control.force |= self.control.pending;
        self.wake.notify_one();
    }
}
impl Worker {
    pub fn start(config: Config, status: Status) -> Self {
        let shared = Arc::new((
            Mutex::new(Control {
                pending: config.price_sync.enabled,
                config,
                generation: 0,
                force: false,
                refresh: false,
                running: false,
                stopped: false,
                status,
                missing: BTreeMap::new(),
            }),
            Condvar::new(),
            Mutex::new(()),
        ));
        let inner = Arc::clone(&shared);
        let handle = thread::Builder::new()
            .name("cpa-price-sync".into())
            .spawn(move || {
                if std::panic::catch_unwind(|| run(&inner)).is_err() {
                    let mut control = inner.0.lock().unwrap_or_else(|e| e.into_inner());
                    control.running = false;
                    control.pending = false;
                    control.stopped = true;
                    control.status.error = Some("价格同步线程异常，请重启插件后重试".into());
                }
            })
            .ok();
        if handle.is_none() {
            let mut control = shared.0.lock().unwrap_or_else(|e| e.into_inner());
            control.pending = false;
            control.stopped = true;
            control.status.error = Some("无法启动价格同步线程，请检查宿主资源并重启插件".into());
        }
        Self { shared, handle }
    }
    pub fn pause(&self) -> Pause<'_> {
        let write = self.shared.2.lock().unwrap_or_else(|e| e.into_inner());
        let mut control = self.shared.0.lock().unwrap_or_else(|e| e.into_inner());
        let resume = control.pending || control.running || control.config.price_sync.enabled;
        control.generation += 1;
        control.pending = false;
        control.missing.clear();
        Pause {
            control,
            _write: write,
            wake: &self.shared.1,
            resume,
        }
    }
    pub fn reconfigure(&self, config: Config) {
        let mut control = self.shared.0.lock().unwrap_or_else(|e| e.into_inner());
        if control.config != config {
            control.generation += 1;
            control.pending = config.price_sync.enabled;
            control.force = true;
            control.config = config;
            self.shared.1.notify_one();
        }
    }
    pub fn request(&self, manual: bool) -> Result<(), String> {
        let mut control = self.shared.0.lock().unwrap_or_else(|e| e.into_inner());
        if control.stopped {
            return Err("价格同步线程不可用，请重启插件".into());
        }
        if manual || control.config.price_sync.enabled {
            // Repeated manual clicks do not create a queue of concurrent downloads.
            if manual && (control.running || control.pending) {
                return Ok(());
            }
            control.pending = true;
            control.force |= manual;
            self.shared.1.notify_one();
        }
        Ok(())
    }
    /// A request met a model without a price: look it up now instead of at the next interval.
    pub fn request_missing(&self, model: &str) {
        let mut control = self.shared.0.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        if control.stopped
            || !control.config.price_sync.enabled
            || control
                .missing
                .get(model)
                .is_some_and(|asked| now.duration_since(*asked) < MISSING_RETRY)
        {
            return;
        }
        control
            .missing
            .retain(|_, asked| now.duration_since(*asked) < MISSING_RETRY);
        control.missing.insert(model.to_string(), now);
        control.pending = true;
        control.refresh = true;
        self.shared.1.notify_one();
    }
    pub fn status(&self) -> Value {
        let control = self.shared.0.lock().unwrap_or_else(|e| e.into_inner());
        let mut value = serde_json::to_value(&control.status).unwrap_or_else(|_| json!({}));
        value["enabled"] = json!(control.config.price_sync.enabled);
        value["interval_hours"] = json!(control.config.price_sync.interval_hours);
        value["running"] = json!(control.running || control.pending);
        value
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        {
            let mut control = self.shared.0.lock().unwrap_or_else(|e| e.into_inner());
            control.stopped = true;
            self.shared.1.notify_one();
        }
        // The host may unload the DLL immediately after shutdown returns.
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn run(shared: &Shared) {
    let mut cache = Cache::new();
    let mut next = Instant::now();
    loop {
        let (config, generation, force, refresh, mut status) = {
            let mut control = shared.0.lock().unwrap_or_else(|e| e.into_inner());
            while !control.stopped && !control.pending {
                let wait = if control.config.price_sync.enabled {
                    next.saturating_duration_since(Instant::now())
                } else {
                    Duration::from_secs(24 * 3600)
                };
                let (guard, timeout) = shared
                    .1
                    .wait_timeout(control, wait)
                    .unwrap_or_else(|e| e.into_inner());
                control = guard;
                if timeout.timed_out() && control.config.price_sync.enabled {
                    control.pending = true;
                }
            }
            if control.stopped {
                return;
            }
            control.pending = false;
            control.running = true;
            control.status.last_attempt = Some(Utc::now().to_rfc3339());
            (
                control.config.clone(),
                control.generation,
                std::mem::take(&mut control.force),
                std::mem::take(&mut control.refresh),
                control.status.clone(),
            )
        };
        if force {
            cache.clear();
        }
        let interval = Duration::from_secs(config.price_sync.interval_hours * 3600);
        let fresh = if refresh {
            interval.min(MISSING_REFRESH)
        } else {
            interval
        };
        let cancelled = || {
            let control = shared.0.lock().unwrap_or_else(|e| e.into_inner());
            control.stopped || control.generation != generation
        };
        status.added = 0;
        status.updated = 0;
        status.sources.clear();
        status.error = None;
        let result = synchronize(
            &config,
            &mut cache,
            &mut status,
            &cancelled,
            fresh,
            &shared.2,
        );
        if let Err(error) = result {
            status.error = Some(error);
        }
        let mut control = shared.0.lock().unwrap_or_else(|e| e.into_inner());
        control.running = false;
        let persist = !cancelled_without_lock(&control, generation);
        if persist {
            if status.error.is_none() {
                status.last_success = Some(Utc::now().to_rfc3339());
            }
            next = Instant::now()
                + if status.error.is_some() {
                    Duration::from_secs(300)
                } else {
                    interval
                };
            control.status = status.clone();
        }
        drop(control);
        // Keep DB waits outside the control lock used by request hooks.
        if persist {
            let _write = shared.2.lock().unwrap_or_else(|e| e.into_inner());
            if !cancelled() {
                if let Ok(db) = Connection::open(config.data_dir.join("manager.sqlite3")) {
                    let _ = db.busy_timeout(Duration::from_secs(2));
                    let _ = db.execute(
                        "INSERT INTO meta(name,value) VALUES('price_sync_status',?1)
                         ON CONFLICT(name) DO UPDATE SET value=excluded.value",
                        [serde_json::to_string(&status).unwrap_or_default()],
                    );
                }
            }
        }
    }
}

fn cancelled_without_lock(control: &Control, generation: u64) -> bool {
    control.stopped || control.generation != generation
}

fn fetch_catalog(source: Source, url: &str) -> Result<Catalog, String> {
    let client = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(4))
        .timeout(Duration::from_secs(8))
        .redirect(reqwest::redirect::Policy::limited(3))
        .user_agent(concat!("cpa-apikey-manager/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|_| "无法建立价格来源连接".to_string())?;
    // Fetch complete public catalogs. Never send client keys, model usage, or management credentials.
    let response = client
        .get(url)
        .header("Accept", "application/json")
        .send()
        .map_err(|error| {
            if error.is_timeout() {
                "价格来源连接超时"
            } else {
                "无法连接价格来源，请检查网络或代理配置"
            }
            .to_string()
        })?;
    if !response.status().is_success() {
        return Err(format!("价格来源返回 HTTP {}", response.status().as_u16()));
    }
    if response
        .content_length()
        .is_some_and(|size| size > MAX_CATALOG_BYTES as u64)
    {
        return Err("价格目录超过 32 MB 限制".into());
    }
    let mut bytes = Vec::new();
    response
        .take((MAX_CATALOG_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| "读取价格目录失败或超时".to_string())?;
    if bytes.len() > MAX_CATALOG_BYTES {
        return Err("价格目录超过 32 MB 限制".into());
    }
    let data: Value =
        serde_json::from_slice(&bytes).map_err(|_| "价格来源返回了无效 JSON".to_string())?;
    Catalog::parse(source, &data)
}

fn synchronize(
    config: &Config,
    cache: &mut Cache,
    status: &mut Status,
    cancelled: &impl Fn() -> bool,
    fresh: Duration,
    write_lock: &Mutex<()>,
) -> Result<(), String> {
    let source_config = config::read_source(config)?;
    let mut db = Connection::open(config.data_dir.join("manager.sqlite3"))
        .map_err(|_| "价格同步无法打开数据库".to_string())?;
    db.busy_timeout(Duration::from_secs(2))
        .map_err(|_| "无法配置价格同步数据库".to_string())?;
    let storage = |_| "价格同步数据库操作失败，请检查磁盘和数据目录".to_string();
    let mut names = BTreeSet::new();
    {
        let mut query = db.prepare("SELECT id FROM models").map_err(storage)?;
        let rows = query
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(storage)?;
        for row in rows {
            let model = row.map_err(storage)?;
            match source_config.aliases.get(&model) {
                Some(targets) if targets.len() == 1 => {
                    names.insert(crate::channel::base_model(&targets[0]).to_string());
                }
                Some(_) => {} // A routing alias with multiple targets has no single safe price.
                None => {
                    names.insert(crate::channel::base_model(&model).to_string());
                }
            }
        }
    }
    let mut missing = BTreeSet::new();
    let mut upgrades = BTreeMap::new();
    {
        let mut query = db.prepare(
            "SELECT p.model,p.price,p.revision FROM prices p JOIN price_origins o ON o.model=p.model"
        ).map_err(storage)?;
        let rows = query
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            })
            .map_err(storage)?;
        for row in rows {
            let (model, value, revision) = row.map_err(storage)?;
            if serde_json::from_str::<Price>(&value)
                .is_ok_and(|price| price.rules_version < RULES_VERSION)
            {
                names.insert(model.clone());
                upgrades.insert(model, revision);
            }
        }
    }
    for model in names {
        let exists: bool = db
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM prices WHERE model=?1)",
                [&model],
                |r| r.get(0),
            )
            .map_err(storage)?;
        if !exists || upgrades.contains_key(&model) {
            missing.insert(model);
        }
    }
    status.unmatched = missing.iter().cloned().collect();
    if missing.is_empty() {
        return Ok(());
    }
    let all_missing = missing.clone();
    let mut selected = Vec::<(String, Source, Entry)>::new();
    let mut available = 0;
    for source in Source::ORDER {
        if cancelled() {
            return Err("价格同步已取消".into());
        }
        if missing.is_empty() {
            continue;
        }
        let url = match source {
            Source::ModelsDev => &config.price_sync.models_dev_url,
            Source::LiteLlm => &config.price_sync.litellm_url,
            Source::OpenRouter => &config.price_sync.openrouter_url,
        };
        if url.is_empty() {
            continue;
        }
        let reuse = cache.get(&source).is_some_and(|entry| {
            entry.url == *url
                && entry.fetched.elapsed()
                    < if entry.catalog.is_ok() {
                        fresh
                    } else {
                        Duration::from_secs(60)
                    }
        });
        if !reuse {
            cache.insert(
                source,
                Cached {
                    url: url.clone(),
                    fetched: Instant::now(),
                    catalog: fetch_catalog(source, url),
                },
            );
        }
        let mut report = SourceStatus {
            source: source.name().into(),
            ..SourceStatus::default()
        };
        if let Some(cached) = cache.get(&source) {
            match &cached.catalog {
                Ok(catalog) => {
                    available += 1;
                    for model in &missing {
                        if let Some(entry) = catalog.select(model) {
                            selected.push((model.clone(), source, entry.clone()));
                            report.matched += 1;
                        }
                    }
                    for (model, _, _) in &selected {
                        missing.remove(model);
                    }
                }
                Err(error) => {
                    report.error = Some(error.clone());
                }
            }
        }
        status.sources.push(report);
    }
    // Second pass only after every exact match failed, so an exact name in a later source
    // still wins over a compatible name in an earlier one.
    for source in Source::ORDER {
        if missing.is_empty() || cancelled() {
            break;
        }
        let Some(Ok(catalog)) = cache.get(&source).map(|cached| cached.catalog.as_ref()) else {
            continue;
        };
        let mut matched = 0;
        for model in missing.clone() {
            if let Some(entry) = catalog.select_compatible(&model) {
                selected.push((model.clone(), source, entry.clone()));
                missing.remove(&model);
                matched += 1;
            }
        }
        if let Some(report) = status
            .sources
            .iter_mut()
            .find(|r| r.source == source.name())
        {
            report.matched += matched;
        }
    }
    if cancelled() {
        return Err("价格同步已取消".into());
    }
    if available == 0 {
        return Err("所有价格来源均不可用，已保留现有价格；稍后将自动重试，也可手动重试。".into());
    }
    for (_, _, entry) in &selected {
        entry
            .price
            .validate()
            .map_err(|_| "价格目录中的计费规则无效")?;
    }
    let _write = write_lock.lock().unwrap_or_else(|e| e.into_inner());
    if cancelled() {
        return Err("价格同步已取消".into());
    }
    let now = Utc::now();
    let tx = db.transaction().map_err(storage)?;
    let mut added = 0;
    let mut updated = 0;
    for (model, source, entry) in selected {
        let value = serde_json::to_string(&entry.price).map_err(|_| "价格数据编码失败")?;
        // Revision and origin checks protect manual saves made during the downloads.
        let changed = if let Some(revision) = upgrades.get(&model) {
            tx.execute(
                "UPDATE prices SET price=?1,revision=revision+1 WHERE model=?2 AND revision=?3
                 AND EXISTS(SELECT 1 FROM price_origins WHERE model=?2)",
                params![value, model, revision],
            )
            .map_err(storage)?
        } else {
            tx.execute(
                "INSERT OR IGNORE INTO prices(model,price,revision) VALUES(?1,?2,1)",
                params![model, value],
            )
            .map_err(storage)?
        };
        if changed == 1 {
            tx.execute("INSERT INTO price_origins(model,source,source_model,synced_at) VALUES(?1,?2,?3,?4)
                ON CONFLICT(model) DO UPDATE SET source=excluded.source,source_model=excluded.source_model,synced_at=excluded.synced_at",
                params![model, source.name(), entry.source_model, now.to_rfc3339()]).map_err(storage)?;
            crate::engine::backfill_unpriced(&tx, &model, &entry.price).map_err(storage)?;
            tx.execute(
                "INSERT INTO audit(created,action,subject,detail) VALUES(?1,'price_sync',?2,?3)",
                params![
                    now.timestamp_millis(),
                    model,
                    json!({"source":source.name(),"source_model":entry.source_model,"rules":entry.price.rules.len(),"upgraded":upgrades.contains_key(&model)}).to_string()
                ],
            )
            .map_err(storage)?;
            if upgrades.contains_key(&model) {
                updated += 1;
            } else {
                added += 1;
            }
        }
    }
    if cancelled() {
        return Err("价格同步已取消".into());
    }
    tx.commit().map_err(storage)?;
    status.added = added;
    status.updated = updated;
    status.unmatched.clear();
    for model in all_missing {
        let found: Option<(String, bool)> = db
            .query_row(
                "SELECT price,EXISTS(SELECT 1 FROM price_origins WHERE model=?1) FROM prices WHERE model=?1",
                [&model],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(storage)?;
        if found.is_none_or(|(value, automatic)| {
            automatic
                && serde_json::from_str::<Price>(&value)
                    .is_ok_and(|price| price.rules_version < RULES_VERSION)
        }) {
            status.unmatched.push(model);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn pausing_preserves_manual_sync_and_stopped_status() {
        let make_worker = |enabled: bool, stopped: bool, pending: bool| {
            let mut config = Config::default();
            config.price_sync.enabled = enabled;
            Worker {
                shared: Arc::new((
                    Mutex::new(Control {
                        config,
                        generation: 0,
                        pending,
                        force: false,
                        refresh: false,
                        running: false,
                        stopped,
                        status: Status::default(),
                        missing: BTreeMap::new(),
                    }),
                    Condvar::new(),
                    Mutex::new(()),
                )),
                handle: None,
            }
        };
        let stopped = make_worker(true, true, false);
        drop(stopped.pause());
        let manual = make_worker(false, false, true);
        drop(manual.pause());
        assert_eq!(stopped.status()["running"], false);
        assert_eq!(manual.status()["running"], true);
    }

    #[test]
    fn upgrades_automatic_rules_without_merging_sources_or_touching_manual_prices() {
        let root = std::env::current_dir()
            .unwrap()
            .join("temp")
            .join(format!("price-sync-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let mut config = Config {
            cpa_config_path: root.join("config.yaml"),
            data_dir: root.clone(),
            ..Config::default()
        };
        config.price_sync.openrouter_url.clear();
        fs::write(&config.cpa_config_path, "access:\n  api-keys: []\n").unwrap();
        let db = Connection::open(root.join("manager.sqlite3")).unwrap();
        db.execute_batch(include_str!("schema.sql")).unwrap();
        let primary = Catalog::parse(
            Source::ModelsDev,
            &json!({"openai":{"models":{"gpt-5.5":{
                "cost":{"input":5,"output":30,"cache_read":0.5,
                    "tiers":[{"input":10,"output":45,"cache_read":1,
                        "tier":{"type":"context","size":272000}}]},
                "experimental":{"modes":{"fast":{
                    "cost":{"input":12.5,"output":75,"cache_read":1.25},
                    "provider":{"body":{"service_tier":"priority"}}
                }}}
            }}}}),
        )
        .unwrap();
        let fallback = Catalog::parse(
            Source::LiteLlm,
            &json!({
                "gpt-5.5":{
                    "input_cost_per_token":5e-6,"output_cost_per_token":3e-5,
                    "cache_read_input_token_cost":5e-7,
                    "input_cost_per_token_batches":2.5e-6,
                    "output_cost_per_token_batches":1.5e-5,
                    "input_cost_per_token_flex":2.5e-6,
                    "output_cost_per_token_flex":1.5e-5,
                    "input_cost_per_token_above_272k_tokens_batches":5e-6,
                    "output_cost_per_token_above_272k_tokens_batches":2.25e-5
                },
                "fallback-only":{"input_cost_per_token":1e-6,"output_cost_per_token":2e-6}
            }),
        )
        .unwrap();
        let mut legacy = primary.select("gpt-5.5").unwrap().price.clone();
        legacy.rules_version = 1;
        legacy.rules.push(crate::accounting::PriceRule {
            input_tokens_above: None,
            service_tier: Some("batch".into()),
            input: "2.5".into(),
            output: "15".into(),
            cache_read: Some("0.25".into()),
            cache_write: None,
        });
        let batch = legacy.rules.last().unwrap().clone();
        let mut flex = batch.clone();
        flex.service_tier = Some("flex".into());
        legacy.rules.push(flex.clone());
        for mut combined in [batch, flex] {
            combined.input_tokens_above = Some(272_000);
            legacy.rules.push(combined);
        }
        assert_eq!(legacy.rules.len(), 6);
        let legacy_json = serde_json::to_string(&legacy).unwrap();
        db.execute_batch(
            "INSERT INTO models VALUES('gpt-5.5'),('manual-model'),('fallback-only');",
        )
        .unwrap();
        for model in ["gpt-5.5", "manual-model"] {
            db.execute(
                "INSERT INTO prices VALUES(?1,?2,7)",
                params![model, legacy_json],
            )
            .unwrap();
        }
        db.execute(
            "INSERT INTO price_origins VALUES('gpt-5.5','models.dev','openai/gpt-5.5','old')",
            [],
        )
        .unwrap();
        db.execute(
            "INSERT INTO requests(request_id,model,key_id,requested,trace_id,started,status,price)
             VALUES('settled','gpt-5.5','key','gpt-5.5','trace',0,'settled',?1)",
            [&legacy_json],
        )
        .unwrap();
        db.execute(
            "INSERT INTO usage(id,request_id,key_id,model,started,tokens,cost,detail,failed,stream)
             VALUES('settled','settled','key','gpt-5.5',0,10,123,'{}',0,0)",
            [],
        )
        .unwrap();
        let mut cache = Cache::new();
        cache.insert(
            Source::ModelsDev,
            Cached {
                url: config.price_sync.models_dev_url.clone(),
                fetched: Instant::now(),
                catalog: Ok(primary),
            },
        );
        cache.insert(
            Source::LiteLlm,
            Cached {
                url: config.price_sync.litellm_url.clone(),
                fetched: Instant::now(),
                catalog: Ok(fallback),
            },
        );
        let mut status = Status::default();
        synchronize(
            &config,
            &mut cache,
            &mut status,
            &|| false,
            Duration::from_secs(3600),
            &Mutex::new(()),
        )
        .unwrap();
        assert_eq!((status.added, status.updated), (1, 1));
        let (updated, revision): (String, i64) = db
            .query_row(
                "SELECT price,revision FROM prices WHERE model='gpt-5.5'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        let price: Price = serde_json::from_str(&updated).unwrap();
        assert_eq!(price.rules_version, RULES_VERSION);
        assert_eq!(price.rules.len(), 2);
        assert!(price.rules.iter().all(|rule| {
            rule.service_tier.is_none() || rule.service_tier.as_deref() == Some("priority")
        }));
        assert_eq!(revision, 8);
        let manual: String = db
            .query_row(
                "SELECT price FROM prices WHERE model='manual-model'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(manual, legacy_json);
        let (cost, snapshot): (i64, String) = db
            .query_row(
                "SELECT u.cost,r.price FROM usage u JOIN requests r ON r.request_id=u.request_id
             WHERE u.id='settled'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(cost, 123);
        assert_eq!(snapshot, legacy_json);
        status = Status::default();
        synchronize(
            &config,
            &mut cache,
            &mut status,
            &|| false,
            Duration::from_secs(3600),
            &Mutex::new(()),
        )
        .unwrap();
        assert_eq!((status.added, status.updated), (0, 0));

        db.execute(
            "UPDATE prices SET price=?1 WHERE model='gpt-5.5'",
            [&legacy_json],
        )
        .unwrap();
        for cached in cache.values_mut() {
            cached.catalog = Err("测试目录不可用".into());
        }
        assert!(synchronize(
            &config,
            &mut cache,
            &mut status,
            &|| false,
            Duration::from_secs(3600),
            &Mutex::new(()),
        )
        .is_err());
        let unchanged: String = db
            .query_row(
                "SELECT price FROM prices WHERE model='gpt-5.5'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(unchanged, legacy_json);
        drop(db);
        fs::remove_dir_all(root).unwrap();
    }
}
