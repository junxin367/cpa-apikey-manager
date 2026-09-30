//! A joinable background worker keeps public-price network I/O off request hooks.
use crate::{
    config::{self, Config},
    price_catalog::{Catalog, Entry, Source},
};
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Read,
    sync::{Arc, Condvar, Mutex},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const MAX_CATALOG_BYTES: usize = 32 * 1024 * 1024;

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
    pub unmatched: Vec<String>,
    pub sources: Vec<SourceStatus>,
    pub error: Option<String>,
}

struct Control {
    config: Config,
    generation: u64,
    pending: bool,
    force: bool,
    running: bool,
    stopped: bool,
    status: Status,
}
type Shared = Arc<(Mutex<Control>, Condvar)>;
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
impl Worker {
    pub fn start(config: Config, status: Status) -> Self {
        let shared = Arc::new((
            Mutex::new(Control {
                pending: config.price_sync.enabled,
                config,
                generation: 0,
                force: false,
                running: false,
                stopped: false,
                status,
            }),
            Condvar::new(),
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
        let (config, generation, force, mut status) = {
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
                control.status.clone(),
            )
        };
        if force {
            cache.clear();
        }
        let cancelled = || {
            let control = shared.0.lock().unwrap_or_else(|e| e.into_inner());
            control.stopped || control.generation != generation
        };
        status.added = 0;
        status.sources.clear();
        status.error = None;
        let result = synchronize(&config, &mut cache, &mut status, &cancelled);
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
                    Duration::from_secs(config.price_sync.interval_hours * 3600)
                };
            control.status = status.clone();
        }
        drop(control);
        // Keep DB waits outside the control lock used by request hooks.
        if persist {
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
                    names.insert(targets[0].clone());
                }
                Some(_) => {} // A routing alias with multiple targets has no single safe price.
                None => {
                    names.insert(model);
                }
            }
        }
    }
    let mut missing = BTreeSet::new();
    for model in names {
        let exists: bool = db
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM prices WHERE model=?1)",
                [&model],
                |r| r.get(0),
            )
            .map_err(storage)?;
        if !exists {
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
            break;
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
                        Duration::from_secs(config.price_sync.interval_hours * 3600)
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
    if cancelled() {
        return Err("价格同步已取消".into());
    }
    if available == 0 {
        return Err("所有价格来源均不可用，已保留现有价格；稍后将自动重试，也可手动重试。".into());
    }
    let now = Utc::now();
    let tx = db.transaction().map_err(storage)?;
    let mut added = 0;
    for (model, source, entry) in selected {
        // The unique key, not an earlier read, protects concurrent manual saves.
        let inserted = tx
            .execute(
                "INSERT OR IGNORE INTO prices(model,price,revision) VALUES(?1,?2,1)",
                params![
                    model,
                    serde_json::to_string(&entry.price).map_err(|_| "价格数据编码失败")?
                ],
            )
            .map_err(storage)?;
        if inserted == 1 {
            tx.execute("INSERT INTO price_origins(model,source,source_model,synced_at) VALUES(?1,?2,?3,?4)",
                params![model, source.name(), entry.source_model, now.to_rfc3339()]).map_err(storage)?;
            tx.execute(
                "INSERT INTO audit(created,action,subject,detail) VALUES(?1,'price_sync',?2,?3)",
                params![
                    now.timestamp_millis(),
                    model,
                    json!({"source":source.name(),"source_model":entry.source_model}).to_string()
                ],
            )
            .map_err(storage)?;
            added += 1;
        }
    }
    if cancelled() {
        return Err("价格同步已取消".into());
    }
    tx.commit().map_err(storage)?;
    status.added = added;
    status.unmatched.clear();
    for model in all_missing {
        let found: Option<i64> = db
            .query_row(
                "SELECT revision FROM prices WHERE model=?1",
                [&model],
                |r| r.get(0),
            )
            .optional()
            .map_err(storage)?;
        if found.is_none() {
            status.unmatched.push(model);
        }
    }
    Ok(())
}
