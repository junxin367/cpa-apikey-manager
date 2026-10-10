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
}

impl Price {
    pub fn validate(&self) -> Result<()> {
        for p in [
            &self.input,
            &self.output,
            self.cache_read.as_ref().unwrap_or(&self.input),
            self.cache_write.as_ref().unwrap_or(&self.input),
        ] {
            micros(p)?;
        }
        Ok(())
    }
    pub fn cost(&self, tokens: &Tokens) -> Result<i64> {
        if !tokens.complete {
            return Err("Token 明细不完整，无法准确计价".into());
        }
        let buckets = [
            (tokens.input, &self.input),
            (tokens.output, &self.output),
            (
                tokens.cache_read,
                self.cache_read.as_ref().unwrap_or(&self.input),
            ),
            (
                tokens.cache_write,
                self.cache_write.as_ref().unwrap_or(&self.input),
            ),
        ];
        let mut sum = 0_i128;
        for (count, rate) in buckets {
            sum += i128::from(count) * i128::from(micros(rate)?);
        }
        let value = (sum + i128::from(SCALE) - 1) / i128::from(SCALE);
        i64::try_from(value)
            .ok()
            .filter(|v| *v <= MAX_VALUE)
            .ok_or_else(|| "费用超出范围".into())
    }
}
