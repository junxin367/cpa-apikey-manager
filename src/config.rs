use crate::accounting::Result;
use serde::{Deserialize, Serialize};
use serde_yaml::Value;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs, path::PathBuf};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(default, rename_all = "kebab-case")]
pub struct Config {
    pub cpa_config_path: PathBuf,
    pub cpa_base_url: String,
    pub data_dir: PathBuf,
    pub price_sync: PriceSyncConfig,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            cpa_config_path: PathBuf::new(),
            cpa_base_url: "http://127.0.0.1:8317".into(),
            data_dir: "data/cpa-apikey-manager".into(),
            price_sync: PriceSyncConfig::default(),
        }
    }
}
impl Config {
    pub fn validate(&self) -> Result<()> {
        if self.cpa_config_path.as_os_str().is_empty() {
            return Err("必须配置 cpa-config-path".into());
        }
        let url = reqwest::Url::parse(&self.cpa_base_url).map_err(|_| "cpa-base-url 无效")?;
        if !["http", "https"].contains(&url.scheme())
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err("cpa-base-url 必须是没有凭据和查询参数的 HTTP(S) 地址".into());
        }
        self.price_sync.validate()
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(default, rename_all = "kebab-case")]
pub struct PriceSyncConfig {
    pub enabled: bool,
    pub interval_hours: u64,
    pub models_dev_url: String,
    pub litellm_url: String,
    pub openrouter_url: String,
}
impl Default for PriceSyncConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            interval_hours: 6,
            models_dev_url: "https://models.dev/catalog.json".into(),
            litellm_url: "https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json".into(),
            openrouter_url: "https://openrouter.ai/api/v1/models".into(),
        }
    }
}
impl PriceSyncConfig {
    fn validate(&self) -> Result<()> {
        if !(1..=168).contains(&self.interval_hours) {
            return Err("价格同步间隔必须是 1 到 168 小时".into());
        }
        for text in [
            &self.models_dev_url,
            &self.litellm_url,
            &self.openrouter_url,
        ] {
            if text.is_empty() {
                continue;
            }
            let url = reqwest::Url::parse(text).map_err(|_| "价格来源地址无效")?;
            if !["http", "https"].contains(&url.scheme())
                || url.host_str().is_none()
                || !url.username().is_empty()
                || url.password().is_some()
                || url.fragment().is_some()
            {
                return Err("价格来源必须是没有嵌入凭据和片段的 HTTP(S) 地址".into());
            }
        }
        Ok(())
    }
}

pub struct Source {
    pub keys: Vec<String>,
    pub aliases: BTreeMap<String, Vec<String>>,
    pub models: Vec<String>,
}

pub fn caller_scope(key: &str) -> String {
    let mut hash = Sha256::new();
    hash.update(b"cli-proxy-api:caller-scope:v1\0");
    hash.update(key.trim().as_bytes());
    hex::encode(hash.finalize())
}

pub fn read_source(config: &Config) -> Result<Source> {
    let bytes = fs::read(&config.cpa_config_path).map_err(|_| {
        "无法读取宿主配置文件，暂时无法校验密钥权限。请联系管理员检查 cpa-config-path 配置和文件读取权限。"
    })?;
    let doc: Value = serde_yaml::from_slice(&bytes)
        .map_err(|_| "宿主 YAML 配置格式无效。请联系管理员修正配置文件后重试。")?;
    let value = doc
        .get("access")
        .and_then(|a| a.get("api-keys"))
        .or_else(|| doc.get("api-keys"))
        .ok_or("宿主配置缺少 access.api-keys / api-keys")?;
    let list = value
        .as_sequence()
        .ok_or("客户端 api-keys 必须是字符串列表")?;
    let mut keys = Vec::new();
    for value in list {
        let s = value
            .as_str()
            .ok_or("客户端 api-keys 只能包含字符串")?
            .trim();
        if s.is_empty() {
            return Err("客户端 api-keys 不能包含空密钥".into());
        }
        if !keys.iter().any(|k| k == s) {
            keys.push(s.to_string());
        }
    }
    let mut aliases: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut models = Vec::new();
    // Only documented model mapping nodes; never infer relationships from spelling.
    fn walk(
        value: &Value,
        in_models: bool,
        aliases: &mut BTreeMap<String, Vec<String>>,
        models: &mut Vec<String>,
    ) {
        if let Some(map) = value.as_mapping() {
            if let Some(name) = map
                .get(Value::String("name".into()))
                .and_then(Value::as_str)
                .filter(|_| in_models)
            {
                let alias = map
                    .get(Value::String("alias".into()))
                    .and_then(Value::as_str)
                    .unwrap_or(name);
                if !name.is_empty() && !alias.is_empty() {
                    models.push(name.into());
                    models.push(alias.into());
                    if alias != name {
                        aliases.entry(alias.into()).or_default().push(name.into());
                    }
                }
            }
            for (k, v) in map {
                if k.as_str() != Some("excluded-models") {
                    walk(
                        v,
                        in_models || k.as_str() == Some("models"),
                        aliases,
                        models,
                    );
                }
            }
        } else if let Some(values) = value.as_sequence() {
            for value in values {
                walk(value, in_models, aliases, models);
            }
        }
    }
    for key in [
        "api-keys",
        "codex-api-key",
        "claude-api-key",
        "gemini-api-key",
        "openai-compatibility",
        "oauth-model-alias",
    ] {
        if let Some(node) = doc.get(key) {
            walk(node, key == "oauth-model-alias", &mut aliases, &mut models);
        }
    }
    if let Some(node) = doc.get("oauth").and_then(|v| v.get("model-alias")) {
        walk(node, true, &mut aliases, &mut models);
    }
    for values in aliases.values_mut() {
        values.sort();
        values.dedup();
    }
    models.sort();
    models.dedup();
    Ok(Source {
        keys,
        aliases,
        models,
    })
}
