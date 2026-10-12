use chrono::{DateTime, Datelike, Days, NaiveDate, TimeZone, Utc};
use serde::{Deserialize, Serialize};

pub type Result<T> = std::result::Result<T, String>;
pub const SCALE: i64 = 1_000_000;
pub const MAX_VALUE: i64 = 9_000_000_000_000_000;

/// Decimal USD -> micro USD, without floating point.
pub fn micros(text: &str) -> Result<i64> {
    let text = text.trim();
    let pieces: Vec<_> = text.split('.').collect();
    if pieces.is_empty()
        || pieces.len() > 2
        || pieces[0].is_empty()
        || !pieces[0].bytes().all(|c| c.is_ascii_digit())
    {
        return Err("金额必须是非负十进制数".into());
    }
    let fraction = pieces.get(1).copied().unwrap_or("");
    if fraction.len() > 6 || !fraction.bytes().all(|c| c.is_ascii_digit()) {
        return Err("金额最多支持 6 位小数".into());
    }
    let whole: i64 = pieces[0].parse().map_err(|_| "金额超出范围")?;
    let part: i64 = format!("{:0<6}", fraction)
        .parse()
        .map_err(|_| "金额格式错误")?;
    whole
        .checked_mul(SCALE)
        .and_then(|n| n.checked_add(part))
        .filter(|n| *n <= MAX_VALUE)
        .ok_or_else(|| "金额超出范围".into())
}

pub fn money(value: i64) -> String {
    format!("{}.{:06}", value / SCALE, value % SCALE)
}

pub fn period_bounds<Zone: TimeZone>(
    now: DateTime<Utc>,
    zone: Zone,
    period: &str,
) -> Result<(i64, i64)> {
    let date = now.with_timezone(&zone).date_naive();
    let start = match period {
        "day" => Some(date),
        "week" => date.checked_sub_days(Days::new(date.weekday().num_days_from_monday() as u64)),
        "month" => NaiveDate::from_ymd_opt(date.year(), date.month(), 1),
        _ => None,
    };
    let start = start.ok_or("不支持的周期")?;
    let end = match period {
        "day" => start.checked_add_days(Days::new(1)),
        "week" => start.checked_add_days(Days::new(7)),
        "month" => {
            let (year, month) = if start.month() == 12 {
                (start.year() + 1, 1)
            } else {
                (start.year(), start.month() + 1)
            };
            NaiveDate::from_ymd_opt(year, month, 1)
        }
        _ => None,
    }
    .ok_or("周期超出日期范围")?;
    let resolve = |d: NaiveDate| {
        // A few IANA zones skip midnight; use the first real instant on that date.
        (0..=180)
            .find_map(|m| {
                zone.from_local_datetime(
                    &d.and_hms_opt(0, 0, 0)
                        .unwrap()
                        .checked_add_signed(chrono::Duration::minutes(m))
                        .unwrap(),
                )
                .earliest()
            })
            .map(|t| t.timestamp_millis())
            .ok_or_else(|| "时区日期无有效时间".to_string())
    };
    Ok((resolve(start)?, resolve(end)?))
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct Detail {
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub reasoning_tokens: i64,
    pub cached_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_creation_tokens: i64,
    pub total_tokens: i64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Tokens {
    pub input: i64,
    pub output: i64,
    pub cache_read: i64,
    pub cache_write: i64,
    pub total: i64,
    pub complete: bool,
    #[serde(default)]
    pub service_tier: Option<String>,
}

pub fn normalize(d: &Detail, provider: &str, executor: &str) -> Result<Tokens> {
    if [
        d.input_tokens,
        d.output_tokens,
        d.reasoning_tokens,
        d.cached_tokens,
        d.cache_read_tokens,
        d.cache_creation_tokens,
        d.total_tokens,
    ]
    .iter()
    .any(|v| *v < 0 || *v > MAX_VALUE)
    {
        return Err("上游 Token 计数超出有效范围".into());
    }
    let semantics = format!("{} {}", provider, executor).to_lowercase();
    // CPA's legacy CachedTokens may contain cache creation when cache read is zero.
    let read = if d.cache_read_tokens > 0 || d.cache_creation_tokens > 0 {
        d.cache_read_tokens
    } else {
        d.cached_tokens
    };
    let independent = (semantics.contains("claude") || semantics.contains("anthropic"))
        && !semantics.contains("openaicompatexecutor")
        && !semantics.contains("openai-compatible");
    let separate_reasoning = ["gemini", "vertex", "antigravity"]
        .iter()
        .any(|s| semantics.contains(s));
    let subset = [
        "openai",
        "codex",
        "deepseek",
        "kimi",
        "glm",
        "qwen",
        "grok",
        "xai",
        "openrouter",
    ]
    .iter()
    .any(|s| semantics.contains(s));
    let input = if independent {
        d.input_tokens
    } else {
        d.input_tokens - read - d.cache_creation_tokens
    };
    // v8 UsageRecord exposes raw Claude output_tokens (thinking is already included),
    // while Gemini candidatesTokenCount excludes thoughtsTokenCount.
    let output = if separate_reasoning {
        d.output_tokens + d.reasoning_tokens
    } else {
        d.output_tokens
    };
    let expected = input + read + d.cache_creation_tokens + output;
    let total = if d.total_tokens > 0 {
        d.total_tokens
    } else {
        expected.max(0)
    };
    Ok(Tokens {
        input: input.max(0),
        output,
        cache_read: read,
        cache_write: d.cache_creation_tokens,
        total,
        complete: (independent || separate_reasoning || subset)
            && input >= 0
            && expected == total
            && (separate_reasoning || d.reasoning_tokens <= output),
        service_tier: None,
    })
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Price {
    pub input: String,
    pub output: String,
    #[serde(default)]
    pub cache_read: Option<String>,
    #[serde(default)]
    pub cache_write: Option<String>,
    #[serde(default)]
    pub rules: Vec<PriceRule>,
    #[serde(default)]
    pub rules_version: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PriceRule {
    #[serde(default)]
    pub input_tokens_above: Option<i64>,
    #[serde(default)]
    pub service_tier: Option<String>,
    pub input: String,
    pub output: String,
    #[serde(default)]
    pub cache_read: Option<String>,
    #[serde(default)]
    pub cache_write: Option<String>,
}

pub fn service_tier(value: &str) -> Option<String> {
    let value = value.trim().to_ascii_lowercase();
    match value.as_str() {
        "" | "auto" => None,
        "default" | "standard" => Some("standard".into()),
        "fast" | "priority" => Some("priority".into()),
        "batches" | "batch" => Some("batch".into()),
        _ => Some(value),
    }
}

fn rates(input: &str, output: &str, read: Option<&str>, write: Option<&str>) -> Result<[i64; 4]> {
    Ok([
        micros(input)?,
        micros(output)?,
        micros(read.unwrap_or(input))?,
        micros(write.unwrap_or(input))?,
    ])
}

impl Price {
    pub fn validate(&self) -> Result<()> {
        self.rates()?;
        if self.rules.len() > 64 {
            return Err("计费规则最多支持 64 条".into());
        }
        let mut conditions = std::collections::BTreeSet::new();
        for rule in &self.rules {
            rule.rates()?;
            if rule
                .input_tokens_above
                .is_some_and(|n| n <= 0 || n > MAX_VALUE)
                || (rule.input_tokens_above.is_none() && rule.service_tier.is_none())
            {
                return Err("计费规则需指定有效的输入 Token 门槛或服务等级".into());
            }
            if rule
                .service_tier
                .as_deref()
                .is_some_and(|tier| !["priority", "flex", "batch", "ultrafast"].contains(&tier))
            {
                return Err("服务等级必须是 priority、flex、batch 或 ultrafast".into());
            }
            if !conditions.insert((rule.input_tokens_above, rule.service_tier.as_deref())) {
                return Err("同一输入 Token 门槛和服务等级不能重复设置".into());
            }
        }
        Ok(())
    }
    fn rates(&self) -> Result<[i64; 4]> {
        rates(
            &self.input,
            &self.output,
            self.cache_read.as_deref(),
            self.cache_write.as_deref(),
        )
    }
    fn rates_for(&self, input: i64, tier: Option<&str>) -> Result<[i64; 4]> {
        let select = |tier| {
            self.rules
                .iter()
                .filter(|rule| {
                    rule.service_tier.as_deref() == tier
                        && rule
                            .input_tokens_above
                            .is_none_or(|threshold| input > threshold)
                })
                .max_by_key(|rule| rule.input_tokens_above.unwrap_or(0))
        };
        let context = select(None);
        if tier.is_none() {
            return context.map_or_else(|| self.rates(), PriceRule::rates);
        }
        let rule = select(tier).ok_or("缺少该服务等级的计费规则，请核对模型价格")?;
        if context.is_some_and(|context| context.input_tokens_above > rule.input_tokens_above) {
            return Err("缺少长上下文与该服务等级的组合费率，请核对模型价格".into());
        }
        rule.rates()
    }
    pub fn cost(&self, tokens: &Tokens) -> Result<i64> {
        let counts = [
            tokens.input,
            tokens.output,
            tokens.cache_read,
            tokens.cache_write,
        ];
        if !tokens.complete
            || counts.iter().any(|n| *n < 0 || *n > MAX_VALUE)
            || counts.iter().sum::<i64>() != tokens.total
        {
            return Err("Token 明细不完整，无法准确计价".into());
        }
        let input = tokens.input + tokens.cache_read + tokens.cache_write;
        let tier = tokens.service_tier.as_deref().and_then(service_tier);
        let tiers = match tier.as_deref() {
            Some("standard") => vec![None],
            Some(tier) => vec![Some(tier)],
            None => {
                let mut tiers = vec![None];
                for rule in &self.rules {
                    if let Some(tier) = rule.service_tier.as_deref() {
                        if !tiers.contains(&Some(tier)) {
                            tiers.push(Some(tier));
                        }
                    }
                }
                tiers
            }
        };
        let mut sum = 0;
        for tier in tiers {
            let rates = self.rates_for(input, tier)?;
            let cost = counts
                .iter()
                .zip(rates)
                .map(|(count, rate)| i128::from(*count) * i128::from(rate))
                .sum();
            // Older hosts cannot report the served tier; never assume the cheapest one.
            sum = sum.max(cost);
        }
        let value = (sum + i128::from(SCALE) - 1) / i128::from(SCALE);
        i64::try_from(value)
            .ok()
            .filter(|v| *v <= MAX_VALUE)
            .ok_or_else(|| "费用超出范围".into())
    }
}

impl PriceRule {
    fn rates(&self) -> Result<[i64; 4]> {
        rates(
            &self.input,
            &self.output,
            self.cache_read.as_deref(),
            self.cache_write.as_deref(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn price() -> Price {
        serde_json::from_value(json!({
            "input":"10", "output":"50", "cache_read":"1", "cache_write":"12.5",
            "rules":[
                {"input_tokens_above":272000, "input":"20", "output":"75", "cache_read":"2", "cache_write":"25"},
                {"service_tier":"priority", "input":"20", "output":"100", "cache_read":"2", "cache_write":"25"},
                {"input_tokens_above":272000, "service_tier":"priority", "input":"40", "output":"150", "cache_read":"4", "cache_write":"50"}
            ]
        })).unwrap()
    }
    fn tokens(input: i64, tier: Option<&str>) -> Tokens {
        Tokens {
            input,
            output: 1_000,
            total: input + 1_000,
            complete: true,
            service_tier: tier.map(str::to_string),
            ..Tokens::default()
        }
    }

    #[test]
    fn context_boundary_bills_all_tokens_and_includes_cached_input() {
        let price = price();
        assert_eq!(
            price.cost(&tokens(272_000, Some("default"))).unwrap(),
            2_770_000
        );
        assert_eq!(
            price.cost(&tokens(272_001, Some("standard"))).unwrap(),
            5_515_020
        );
        let tokens = Tokens {
            input: 200_000,
            cache_read: 60_000,
            cache_write: 12_001,
            output: 1_000,
            total: 273_001,
            complete: true,
            service_tier: Some("standard".into()),
        };
        assert_eq!(price.cost(&tokens).unwrap(), 4_495_025);
    }

    #[test]
    fn served_tier_and_combined_rates_are_distinct() {
        let price = price();
        assert_eq!(price.cost(&tokens(1_000, Some("fast"))).unwrap(), 120_000);
        assert_eq!(
            price.cost(&tokens(272_001, Some("priority"))).unwrap(),
            11_030_040
        );
        assert_eq!(price.cost(&tokens(1_000, None)).unwrap(), 120_000);
        assert_eq!(price.cost(&tokens(272_001, None)).unwrap(), 11_030_040);
        assert!(price.cost(&tokens(1_000, Some("unknown"))).is_err());
    }

    #[test]
    fn missing_combination_never_falls_back_to_a_cheaper_rule() {
        let mut price = price();
        price.rules.pop();
        assert!(price.cost(&tokens(272_001, Some("priority"))).is_err());
        assert!(price.cost(&tokens(272_001, None)).is_err());
        assert!(price.cost(&tokens(272_001, Some("standard"))).is_ok());
    }

    #[test]
    fn validates_conditions_and_preserves_fixed_point_rounding() {
        let mut price = price();
        price.validate().unwrap();
        price.rules.push(price.rules[0].clone());
        assert!(price.validate().is_err());
        price.rules.pop();
        price.rules[0].input_tokens_above = Some(0);
        assert!(price.validate().is_err());
        let flat: Price = serde_json::from_value(json!({"input":"0.000001","output":"0"})).unwrap();
        assert_eq!(flat.cost(&tokens(1, None)).unwrap(), 1);
        let mut incomplete = tokens(1, None);
        incomplete.total = 1;
        assert!(flat.cost(&incomplete).is_err());
    }
}
