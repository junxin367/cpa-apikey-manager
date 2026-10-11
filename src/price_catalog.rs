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
const VARIANT_SUFFIXES: [&str; 8] = [
    "thinking", "think", "high", "medium", "low", "minimal", "xhigh", "none",
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

/// Base names of a host model: itself and the name after each routing prefix (CPA's
/// "team/claude-x" selects a credential, not a price), each with effort/thinking suffixes
/// removed, then "kimi-" forms of short Kimi names such as "k3".
fn base_names(model: &str) -> Vec<String> {
    let mut roots = vec![identity(model)];
    while let Some(tail) = roots
        .last()
        .and_then(|name| name.split_once('/'))
        .map(|(_, tail)| tail.to_string())
        .filter(|tail| !tail.is_empty())
    {
        roots.push(tail);
    }
    let mut names = Vec::new();
    for root in roots {
        let mut name = Some(root);
        while let Some(current) = name {
            name = strip_variant(&current).map(str::to_string);
            names.push(current);
        }
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

/// Words after "claude" that do not change the base Token price. "1m" only adds a long-context
/// surcharge, and tiered prices are not imported for any model.
const CLAUDE_NEUTRAL: [&str; 4] = ["thinking", "think", "latest", "1m"];

/// Official Anthropic spellings of a Claude name as third-party platforms write it, e.g.
/// "claude-4.5-sonnet", "anthropic--claude-opus-4.5", "claude-opus4-6", "model_claude_4_5_opus"
/// or "us.anthropic.claude-sonnet-4-5-20250929-v1:0". After "claude" only the family, a one- or
/// two-part version, a snapshot date, a revision marker and neutral words are accepted; words
/// such as "fast" or "free" may be billed differently, so those names produce nothing.
fn claude_names(name: &str) -> Vec<String> {
    let Some((start, _)) = name
        .match_indices("claude")
        .find(|(i, _)| *i == 0 || !name.as_bytes()[i - 1].is_ascii_alphanumeric())
    else {
        return Vec::new();
    };
    let mut tokens = Vec::new();
    for word in name[start + 6..].split(|c: char| !c.is_ascii_alphanumeric()) {
        if CLAUDE_NEUTRAL.contains(&word) {
            continue;
        }
        if word.len() > 1 && word.starts_with('v') && word[1..].bytes().all(|b| b.is_ascii_digit())
        {
            tokens.push("v");
            continue;
        }
        // "opus4" is "opus" followed by version 4.
        let bytes = word.as_bytes();
        let mut from = 0;
        for i in 1..=bytes.len() {
            if i == bytes.len() || bytes[i].is_ascii_digit() != bytes[i - 1].is_ascii_digit() {
                tokens.push(&word[from..i]);
                from = i;
            }
        }
    }
    let (mut family, mut version, mut date, mut revision) = (None, Vec::new(), None, false);
    for token in tokens {
        let digits = token.bytes().all(|b| b.is_ascii_digit());
        if revision && digits {
            continue; // Bedrock "-v1:0"
        } else if token == "v" {
            revision = true;
        } else if digits && token.len() == 8 && !version.is_empty() && date.is_none() {
            date = Some(token);
        } else if digits && token.len() <= 2 && version.len() < 2 && date.is_none() {
            version.push(token);
        } else if !digits && family.is_none() {
            family = Some(token);
        } else {
            return Vec::new();
        }
    }
    let (Some(family), Some(major)) = (family, version.first()) else {
        return Vec::new();
    };
    // Anthropic aliases "claude-opus-4" as "claude-opus-4-0"; 4.x+ put the family first, 3.x last.
    let versions = match version.get(1) {
        Some(&"0") => vec![format!("{major}-0"), major.to_string()],
        Some(minor) => vec![format!("{major}-{minor}")],
        None => vec![major.to_string(), format!("{major}-0")],
    };
    let mut names = Vec::new();
    for suffix in date
        .map(|d| format!("-{d}"))
        .into_iter()
        .chain([String::new()])
    {
        for v in &versions {
            names.push(format!("claude-{family}-{v}{suffix}"));
            names.push(format!("claude-{v}-{family}{suffix}"));
        }
    }
    // OpenRouter writes versions with a dot.
    if let Some(minor) = version.get(1) {
        names.push(format!("claude-{family}-{major}.{minor}"));
        names.push(format!("claude-{major}.{minor}-{family}"));
    }
    names
}

/// Catalogs write versions with a dot where some platforms use a dash: "gpt-5-4-mini" is
/// "gpt-5.4-mini" and "kimi-k2-6" is "kimi-k2.6". Only the first pair of one- or two-digit
/// numbers is joined, so dates such as "2024-05-13" keep their dashes.
fn dotted(name: &str) -> Option<String> {
    let parts: Vec<_> = name.split('-').collect();
    (1..parts.len()).find_map(|i| {
        let major = parts[i - 1].len()
            - parts[i - 1]
                .trim_end_matches(|c: char| c.is_ascii_digit())
                .len();
        let minor = parts[i];
        ((1..=2).contains(&major)
            && (1..=2).contains(&minor.len())
            && minor.bytes().all(|b| b.is_ascii_digit()))
        .then(|| format!("{}.{}", parts[..i].join("-"), parts[i..].join("-")))
    })
}

/// Alternative catalog identities for a host model ID, most specific first, excluding the ID itself.
pub fn compatible_names(model: &str) -> Vec<String> {
    let original = identity(model);
    let mut names = Vec::<String>::new();
    let bases = base_names(model);
    let claude: Vec<_> = bases.iter().flat_map(|base| claude_names(base)).collect();
    let dotted: Vec<_> = bases
        .iter()
        .filter(|base| !base.contains('/'))
        .filter_map(|base| dotted(base))
        .collect();
    for base in bases.into_iter().chain(claude).chain(dotted) {
        let mut push = |name: String| {
            if name != original && !names.contains(&name) {
                names.push(name);
            }
        };
        if !base.contains('/') {
            push(base.clone());
            for vendor in vendors(&channel::classify(&base)) {
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
        // A routing prefix such as CPA's "team/model" names a credential, not a catalog provider.
        assert_eq!(
            pick(&catalog, "team/gemini-3.6-flash-high").as_deref(),
            Some("gemini-3.6-flash")
        );
        assert_eq!(
            pick(&catalog, "devin/k3").as_deref(),
            Some("moonshot/kimi-k3")
        );
        // Dashed versions as some platforms write them.
        assert_eq!(
            pick(&catalog, "devin/gemini-3-6-flash-high").as_deref(),
            Some("gemini-3.6-flash")
        );
    }

    #[test]
    fn third_party_claude_names_use_official_prices() {
        let cost = |input: f64, output: f64| json!({"cost": {"input": input, "output": output}});
        let catalog = Catalog::parse(
            Source::ModelsDev,
            &json!({
                "models": {
                    "anthropic/claude-haiku-5-5": {},
                    "anthropic/claude-sonnet-4-5": {},
                    "anthropic/claude-opus-4-8": {},
                },
                "providers": {
                    "anthropic": {"models": {
                        "claude-haiku-5-5": cost(0.1, 0.5),
                        "claude-sonnet-4-5": cost(3.0, 15.0),
                        "claude-opus-4-8": cost(5.0, 25.0),
                    }},
                    "venice": {"models": {"claude-opus-4-8-fast": cost(30.0, 150.0)}},
                },
            }),
        )
        .unwrap();
        for (model, official) in [
            ("devin/claude-haiku-5-5", "anthropic/claude-haiku-5-5"),
            ("claude-haiku-5-5-20261007", "anthropic/claude-haiku-5-5"),
            ("kiro/claude-sonnet-4.5", "anthropic/claude-sonnet-4-5"),
            ("claude-4.5-sonnet", "anthropic/claude-sonnet-4-5"),
            (
                "anthropic--claude-4-5-sonnet",
                "anthropic/claude-sonnet-4-5",
            ),
            (
                "us.anthropic.claude-sonnet-4-5-20250929-v1:0",
                "anthropic/claude-sonnet-4-5",
            ),
            ("claude-opus4-8-think", "anthropic/claude-opus-4-8"),
            ("model_claude_4_8_opus", "anthropic/claude-opus-4-8"),
            // Long-context surcharges are not imported, so the 1M variant bills the base rates.
            ("claude-opus-4-8-thinking-1m", "anthropic/claude-opus-4-8"),
        ] {
            assert_eq!(pick(&catalog, model).as_deref(), Some(official), "{model}");
        }
        // Speed and free tiers may be billed differently from the base model.
        for model in [
            "claude-opus-4-8-fast",
            "claude-sonnet-4-5-free",
            "gemma-4-31b-claude-4.6-opus-reasoning-distilled",
        ] {
            assert_eq!(pick(&catalog, model), None, "{model}");
        }
    }
}
