//! Public price catalogs. Only strong, unambiguous identities are auto-selected.
use crate::{
    accounting::{money, Price, MAX_VALUE},
    channel,
};
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

/// Reasoning-effort and thinking variants that CPA appends to a base model; they bill the same tokens.
const VARIANT_SUFFIXES: [&str; 7] = [
    "thinking", "high", "medium", "low", "minimal", "xhigh", "none",
];

fn strip_variant(name: &str) -> Option<&str> {
    // CPA thinking budgets such as "gemini-2.5-pro(8192)" or "(high)".
    if name.ends_with(')') {
        if let Some(open) = name.rfind('(').filter(|&i| i > 0) {
            return Some(&name[..open]);
        }
    }
    ['-', ':'].into_iter().find_map(|separator| {
        name.rsplit_once(separator)
            .filter(|(head, tail)| !head.is_empty() && VARIANT_SUFFIXES.contains(tail))
            .map(|(head, _)| head)
    })
}

/// Official catalog prefixes per channel, used when a bare name is shared by several resellers.
fn vendors(channel: &str) -> &'static [&'static str] {
    match channel {
        "gpt" => &["openai"],
        "claude" => &["anthropic"],
        "gemini" => &["gemini", "google"],
        "kimi" => &["moonshot", "moonshotai"],
        "deepseek" => &["deepseek"],
        "glm" => &["zai", "zhipuai"],
        _ => &[],
    }
}

/// Base names of a host model: itself, then variants with effort/thinking suffixes removed,
/// then "kimi-" forms of short Kimi names such as "k3".
fn base_names(model: &str) -> Vec<String> {
    let mut names = vec![identity(model)];
    while let Some(next) = strip_variant(names.last().unwrap()) {
        let next = next.to_string();
        names.push(next);
    }
    let kimi: Vec<_> = names
        .iter()
        .filter(|name| {
            !name.contains('/') && channel::kimi_short(name.split(['-', '.']).next().unwrap_or(""))
        })
        .map(|name| format!("kimi-{name}"))
        .collect();
    names.extend(kimi);
    names.dedup();
    names
}

/// Alternative catalog identities for a host model ID, most specific first, excluding the ID itself.
pub fn compatible_names(model: &str) -> Vec<String> {
    let original = identity(model);
    let mut names = Vec::<String>::new();
    for base in base_names(model) {
        let mut push = |name: String| {
            if name != original && !names.contains(&name) {
                names.push(name);
            }
        };
        if !base.contains('/') {
            push(base.clone());
            for vendor in vendors(channel::classify(&base)) {
                push(format!("{vendor}/{base}"));
            }
        } else {
            push(base);
        }
    }
    names
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
                    let mode = entry.get("mode").and_then(Value::as_str);
                    let image = mode == Some("image_generation");
                    if mode.is_some_and(|mode| !["chat", "completion"].contains(&mode)) && !image {
                        continue;
                    }
                    if model == "sample_spec" {
                        continue;
                    }
                    // Image models bill generated images as output tokens; per-image prices are skipped.
                    let image_entry;
                    let entry = if image {
                        let Some(rate) = entry.get("output_cost_per_image_token") else {
                            continue;
                        };
                        let mut adjusted = entry.clone();
                        adjusted["output_cost_per_token"] = rate.clone();
                        image_entry = adjusted;
                        &image_entry
                    } else {
                        entry
                    };
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

    /// Compatible lookup after an exact miss: effort suffixes, vendor prefixes and short names,
    /// then a family of at least two variants (e.g. `-flare`, `-sunburst`) that all share one price.
    pub fn select_compatible(&self, model: &str) -> Option<&Entry> {
        compatible_names(model)
            .iter()
            .find_map(|name| self.select(name))
            .or_else(|| {
                base_names(model)
                    .iter()
                    .find_map(|name| self.select_family(name))
            })
    }

    fn select_family(&self, base: &str) -> Option<&Entry> {
        if base.is_empty() || base.contains('/') || !base.bytes().any(|c| c.is_ascii_digit()) {
            return None;
        }
        let prefix = format!("{base}-");
        let mut family: Vec<_> = self
            .entries
            .iter()
            .filter(|e| {
                identity(e.source_model.rsplit('/').next().unwrap_or("")).starts_with(&prefix)
            })
            .collect();
        // Prefer unqualified (first-party) names over reseller-prefixed ones.
        family.sort_by_key(|e| (e.source_model.contains('/'), e.source_model.clone()));
        let variants: std::collections::BTreeSet<_> = family
            .iter()
            .map(|e| identity(e.source_model.rsplit('/').next().unwrap_or("")))
            .collect();
        let first = *family.first()?;
        (variants.len() >= 2 && family.iter().all(|e| e.price == first.price)).then_some(first)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn litellm() -> Catalog {
        let chat = |input: f64, output: f64| json!({"mode":"chat","input_cost_per_token":input,"output_cost_per_token":output});
        let image = json!({"mode":"image_generation","input_cost_per_token":0.000005,"output_cost_per_image_token":0.00003});
        Catalog::parse(
            Source::LiteLlm,
            &json!({
                "claude-opus-4-6": chat(0.000005, 0.000025),
                "gemini-3.6-flash": chat(0.00000075, 0.00000375),
                "moonshot/kimi-k3": chat(0.000003, 0.000015),
                "fireworks_ai/kimi-k3": chat(0.0000025, 0.00001),
                "gpt-image-2": image.clone(),
                "gpt-image-2.5-flare": image.clone(),
                "gpt-image-2.5-sunburst": image,
                "fal_ai/gpt-image-2": {"mode":"image_generation","output_cost_per_image":0.145},
                "gpt-9-mini": chat(0.000001, 0.000002),
                "gpt-9-pro": chat(0.00001, 0.00002),
                "embed-1": {"mode":"embedding","input_cost_per_token":0.000001,"output_cost_per_token":0},
            }),
        )
        .unwrap()
    }

    fn pick(catalog: &Catalog, model: &str) -> Option<String> {
        catalog
            .select(model)
            .or_else(|| catalog.select_compatible(model))
            .map(|entry| entry.source_model.clone())
    }

    #[test]
    fn image_models_use_image_token_output_price() {
        let catalog = litellm();
        let price = &catalog.select("gpt-image-2").unwrap().price;
        assert_eq!(
            (price.input.as_str(), price.output.as_str()),
            ("5.000000", "30.000000")
        );
        assert!(catalog
            .entries
            .iter()
            .all(|e| e.model != "fal_ai/gpt-image-2" && e.model != "embed-1"));
    }

    #[test]
    fn compatible_names_cover_host_variants() {
        let catalog = litellm();
        assert_eq!(
            pick(&catalog, "claude-opus-4-6-thinking").as_deref(),
            Some("claude-opus-4-6")
        );
        assert_eq!(
            pick(&catalog, "gemini-3.6-flash-high").as_deref(),
            Some("gemini-3.6-flash")
        );
        assert_eq!(
            pick(&catalog, "gemini-3.6-flash(8192)").as_deref(),
            Some("gemini-3.6-flash")
        );
        // A bare short name shared by resellers resolves through the official vendor prefix.
        assert_eq!(pick(&catalog, "k3").as_deref(), Some("moonshot/kimi-k3"));
        assert_eq!(
            pick(&catalog, "gpt-image-2.5-flare").as_deref(),
            Some("gpt-image-2.5-flare")
        );
        // Same-priced variants stand in for their family name.
        assert_eq!(
            pick(&catalog, "gpt-image-2.5").as_deref(),
            Some("gpt-image-2.5-flare")
        );
        // Differently priced variants never stand in for an unknown base model.
        assert_eq!(pick(&catalog, "gpt-9"), None);
        assert_eq!(pick(&catalog, "qwen-max"), None);
    }
}
