//! Public price catalogs. Only strong, unambiguous identities are auto-selected.
use crate::accounting::{money, Price, MAX_VALUE};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Source {
    ModelsDev,
    LiteLlm,
    OpenRouter,
}
impl Source {
    pub const ORDER: [Self; 3] = [Self::ModelsDev, Self::LiteLlm, Self::OpenRouter];
    pub fn name(self) -> &'static str {
        match self {
            Self::ModelsDev => "models.dev",
            Self::LiteLlm => "LiteLLM",
            Self::OpenRouter => "OpenRouter",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub source_model: String,
    pub model: String,
    pub price: Price,
}

pub struct Catalog {
    pub source: Source,
    pub entries: Vec<Entry>,
    // None means the legacy provider-only models.dev format.
    canonical: Option<BTreeMap<String, Option<String>>>,
}

fn identity(text: &str) -> String {
    text.trim().to_lowercase()
}

/// Convert USD/token or USD/million to our fixed USD/million representation.
/// Decimal exponents are handled as integers; a positive sub-micro rate never becomes free.
pub fn rate(value: &Value, per_token: bool) -> Option<String> {
    let text = match value {
        Value::Number(number) => number.to_string(),
        Value::String(text) => text.trim().to_string(),
        _ => return None,
    };
    if text.is_empty() || text.len() > 64 || text.starts_with('-') {
        return None;
    }
    let mut parts = text.split(['e', 'E']);
    let mantissa = parts.next()?;
    let exponent = match parts.next() {
        Some(value) => value.parse::<i32>().ok()?,
        None => 0,
    };
    if parts.next().is_some() || !(-100..=100).contains(&exponent) {
        return None;
    }
    let mut decimal = mantissa.split('.');
    let whole = decimal.next()?;
    let fraction = decimal.next().unwrap_or("");
    if decimal.next().is_some()
        || whole.is_empty()
        || !whole
            .bytes()
            .chain(fraction.bytes())
            .all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let digits = format!("{whole}{fraction}").parse::<u128>().ok()?;
    if digits == 0 {
        return Some(money(0));
    }
    let shift = exponent - i32::try_from(fraction.len()).ok()? + if per_token { 12 } else { 6 };
    let micros = if shift >= 0 {
        digits.checked_mul(10_u128.checked_pow(shift as u32)?)?
    } else if -shift > 38 {
        1
    } else {
        let divisor = 10_u128.checked_pow((-shift) as u32)?;
        digits / divisor + u128::from(digits % divisor != 0)
    };
    (micros <= MAX_VALUE as u128).then(|| money(micros as i64))
}

fn optional_rate(object: &Value, names: &[&str], per_token: bool) -> Option<Option<String>> {
    for name in names {
        if let Some(value) = object.get(name) {
            if !value.is_null() {
                return rate(value, per_token).map(Some);
            }
        }
    }
    Some(None)
}

fn parse_price(object: &Value, source: Source) -> Option<Price> {
    let (input, output, reads, writes, per_token): (&str, &str, &[&str], &[&str], bool) =
        match source {
            Source::ModelsDev => ("input", "output", &["cache_read"], &["cache_write"], false),
            Source::LiteLlm => (
                "input_cost_per_token",
                "output_cost_per_token",
                &["cache_read_input_token_cost", "input_cache_read"],
                &[
                    "cache_creation_input_token_cost",
                    "cache_write_input_token_cost",
                    "input_cache_write",
                    "input_cache_creation",
                ],
                true,
            ),
            Source::OpenRouter => (
                "prompt",
                "completion",
                &["input_cache_read", "cache_read_input_token_cost"],
                &[
                    "input_cache_write",
                    "input_cache_creation",
                    "cache_creation_input_token_cost",
                    "cache_write_input_token_cost",
                ],
                true,
            ),
        };
    Some(Price {
        // Missing or invalid prices must not silently become zero.
        input: rate(object.get(input)?, per_token)?,
        output: rate(object.get(output)?, per_token)?,
        cache_read: optional_rate(object, reads, per_token)?,
        cache_write: optional_rate(object, writes, per_token)?,
    })
}

impl Catalog {
    pub fn parse(source: Source, data: &Value) -> Result<Self, String> {
        let mut catalog = Self {
            source,
            entries: Vec::new(),
            canonical: None,
        };
        match source {
            Source::ModelsDev => {
                let providers = if let Some(providers) = data.get("providers") {
                    let models = data
                        .get("models")
                        .and_then(Value::as_object)
                        .filter(|models| !models.is_empty())
                        .ok_or("models.dev 缺少规范模型目录")?;
                    let mut identities = BTreeMap::<String, Option<String>>::new();
                    for model in models.keys() {
                        for name in [
                            model.as_str(),
                            model.split_once('/').map(|(_, tail)| tail).unwrap_or(model),
                        ] {
                            let key = identity(name);
                            let canonical = identity(model);
                            identities
                                .entry(key)
                                .and_modify(|old| {
                                    if old.as_ref() != Some(&canonical) {
                                        *old = None;
                                    }
                                })
                                .or_insert(Some(canonical));
                        }
                    }
                    catalog.canonical = Some(identities);
                    providers
                } else {
                    data
                };
                for (provider, entry) in providers.as_object().ok_or("models.dev 目录格式无效")?
                {
                    let Some(models) = entry.get("models").and_then(Value::as_object) else {
                        continue;
                    };
                    for (model, entry) in models {
                        // Only standard text output rates are supported by this plugin.
                        if entry
                            .pointer("/modalities/output")
                            .and_then(Value::as_array)
                            .is_some_and(|items| !items.iter().any(|v| v == "text"))
                        {
                            continue;
                        }
                        if let Some(price) = entry.get("cost").and_then(|v| parse_price(v, source))
                        {
                            catalog.entries.push(Entry {
                                source_model: format!("{provider}/{model}"),
                                model: model.clone(),
                                price,
                            });
                        }
                    }
                }
            }
            Source::LiteLlm => {
                for (model, entry) in data.as_object().ok_or("LiteLLM 目录格式无效")? {
                    if entry
                        .get("mode")
                        .and_then(Value::as_str)
                        .is_some_and(|mode| !["chat", "completion"].contains(&mode))
                    {
                        continue;
                    }
                    if model == "sample_spec" {
                        continue;
                    }
                    if let Some(price) = parse_price(entry, source) {
                        catalog.entries.push(Entry {
                            source_model: model.clone(),
                            model: model.clone(),
                            price,
                        });
                    }
                }
            }
            Source::OpenRouter => {
                for entry in data
                    .get("data")
                    .and_then(Value::as_array)
                    .ok_or("OpenRouter 目录格式无效")?
                {
                    let Some(model) = entry.get("id").and_then(Value::as_str) else {
                        continue;
                    };
                    if entry
                        .pointer("/architecture/output_modalities")
                        .and_then(Value::as_array)
                        .is_some_and(|items| !items.iter().any(|v| v == "text"))
                    {
                        continue;
                    }
                    if let Some(price) = entry.get("pricing").and_then(|v| parse_price(v, source)) {
                        catalog.entries.push(Entry {
                            source_model: model.into(),
                            model: model.into(),
                            price,
                        });
                    }
                }
            }
        }
        catalog
            .entries
            .retain(|e| !e.model.trim().is_empty() && e.source_model.len() <= 512);
        if catalog.entries.is_empty() {
            return Err(format!("{} 未返回可用的文本模型价格", source.name()));
        }
        Ok(catalog)
    }

    pub fn select(&self, model: &str) -> Option<&Entry> {
        let wanted = identity(model);
        if let Some(canonical) = &self.canonical {
            if let Some(Some(official)) = canonical.get(&wanted) {
                let matches: Vec<_> = self
                    .entries
                    .iter()
                    .filter(|e| identity(&e.source_model) == *official)
                    .collect();
                if matches.len() == 1 {
                    return matches.first().copied();
                }
            }
            // A qualified provider identity may select that provider, but unqualified
            // models must not silently choose a reseller when official metadata exists.
            if !wanted.contains('/') {
                return None;
            }
        }
        let direct: Vec<_> = self
            .entries
            .iter()
            .filter(|e| identity(&e.source_model) == wanted)
            .collect();
        if !direct.is_empty() {
            return (direct.len() == 1).then(|| direct[0]);
        }
        if wanted.contains('/') {
            return None;
        }
        let matches: Vec<_> = self
            .entries
            .iter()
            .filter(|e| {
                identity(&e.model) == wanted
                    || identity(e.source_model.rsplit('/').next().unwrap_or("")) == wanted
            })
            .collect();
        (matches.len() == 1).then(|| matches[0])
    }
}
