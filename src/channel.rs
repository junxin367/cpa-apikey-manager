//! Channels are model families identified from model names, independent of upstream credentials.

/// Brand spellings a capitalised ID would get wrong, plus the unclassified channel.
const LABELS: [(&str, &str); 7] = [
    ("gpt", "GPT"),
    ("claude", "Claude"),
    ("deepseek", "DeepSeek"),
    ("glm", "GLM"),
    ("kimi", "Kimi"),
    ("gemini", "Gemini"),
    ("other", "其他"),
];

pub fn label(id: &str) -> String {
    if let Some((_, name)) = LABELS.iter().find(|(key, _)| *key == id) {
        return (*name).into();
    }
    let mut chars = id.chars();
    match chars.next() {
        Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
        None => "其他".into(),
    }
}

/// Channel IDs are lowercase ASCII words, the shape `classify` produces.
pub fn valid(id: &str) -> bool {
    (2..=32).contains(&id.len()) && id.bytes().all(|c| c.is_ascii_lowercase())
}

/// A bare Kimi version root such as `k2` or `k3`.
pub fn kimi_short(root: &str) -> bool {
    root.strip_prefix('k')
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|c| c.is_ascii_digit()))
}

/// Kept separately from model channels so existing upstream rules retain their usage and limits.
pub fn legacy(upstream_format: Option<&str>) -> &'static str {
    if upstream_format.is_some_and(|format| format.trim().eq_ignore_ascii_case("antigravity")) {
        "antigravity"
    } else {
        ""
    }
}

pub fn classify(model: &str) -> String {
    let name = model
        .rsplit('/')
        .find(|part| !part.is_empty())
        .unwrap_or("")
        .to_ascii_lowercase();
    let mut words = name
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty());
    let first = words.next().unwrap_or("");
    // CPA exposes Kimi coding models by short names such as "k3" and "k3-256k".
    if kimi_short(first) {
        return "kimi".into();
    }
    if first.strip_prefix('o').is_some_and(|n| {
        n.starts_with(|c: char| ('1'..='9').contains(&c)) && n.bytes().all(|c| c.is_ascii_digit())
    }) {
        return "gpt".into();
    }
    let root = letters(first);
    let wrapped = matches!(
        root,
        "model" | "anthropic" | "databricks" | "us" | "eu" | "apac" | "global"
    );
    if let Some(family) = family(root).or_else(|| {
        wrapped
            .then(|| words.find_map(|word| family(letters(word))))
            .flatten()
    }) {
        return family.into();
    }
    if valid(root) && root != "antigravity" {
        root.into()
    } else {
        "other".into()
    }
}

/// Leading letters of a word: `qwen3` is `qwen`.
fn letters(word: &str) -> &str {
    &word[..word
        .find(|c: char| !c.is_ascii_alphabetic())
        .unwrap_or(word.len())]
}

fn family(root: &str) -> Option<&'static str> {
    Some(match root {
        "gpt" | "chatgpt" | "codex" => "gpt",
        "claude" => "claude",
        "deepseek" => "deepseek",
        "glm" => "glm",
        "kimi" | "moonshot" => "kimi",
        "gemini" => "gemini",
        _ => return None,
    })
}

#[derive(Clone, Copy)]
pub enum Scope<'a> {
    Total,
    Model(&'a str),
    Channel(&'a str),
}
impl<'a> Scope<'a> {
    // These SQL fragments are constants; identifiers never come from user input.
    pub fn filter(self) -> &'static str {
        match self {
            Self::Total => "?2=''",
            Self::Model(_) => "model=?2",
            Self::Channel("antigravity") => "legacy_channel=?2 AND legacy_channel!=''",
            Self::Channel(_) => "channel=?2",
        }
    }
    pub fn value(self) -> &'a str {
        match self {
            Self::Total => "",
            Self::Model(id) | Self::Channel(id) => id,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Total => "total",
            Self::Model(_) => "model",
            Self::Channel(_) => "channel",
        }
    }
    pub fn subject(self) -> String {
        match self {
            Self::Total => "当前密钥全部模型".into(),
            Self::Model(id) => format!("模型「{id}」"),
            Self::Channel(id) => format!("渠道「{}」", label(id)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upstream_rules_are_separate_from_model_channels() {
        assert!(valid("antigravity"));
        assert_eq!(label("antigravity"), "Antigravity");
        assert_eq!(legacy(Some("antigravity")), "antigravity");
        assert_eq!(legacy(Some("Antigravity")), "antigravity");
        assert_eq!(legacy(Some("claude")), "");
        assert_eq!(legacy(None), "");
        assert_eq!(classify("antigravity/claude-sonnet-4-5"), "claude");
        assert_eq!(classify("antigravity/gemini-3-pro-high"), "gemini");
        assert_eq!(classify("antigravity"), "other");
    }

    #[test]
    fn classifies_gemini_and_short_kimi_names() {
        assert_eq!(classify("gemini-3.6-flash-high"), "gemini");
        assert_eq!(classify("google/gemini-3.8-flash"), "gemini");
        assert_eq!(label("gemini"), "Gemini");
        assert_eq!(classify("k3"), "kimi");
        assert_eq!(classify("k3-256k"), "kimi");
        assert_eq!(classify("kimi-k3"), "kimi");
        assert_eq!(classify("claude-opus-4-6-thinking"), "claude");
        assert_eq!(classify("gpt-image-2.5-flare"), "gpt");
        assert_eq!(classify("k"), "other");
    }

    #[test]
    fn identifies_new_families_from_model_names() {
        for (model, family) in [
            ("qwen3-coder-plus", "qwen"),
            ("devin/swe-2", "swe"),
            ("grok-code-fast-1", "grok"),
            ("qwen3-gpt-distilled", "qwen"),
            ("gemma-4-26b-claude-sonnet-distilled", "gemma"),
            ("kling-v2", "kling"),
            ("devin/model_claude_4_5_opus", "claude"),
            ("devin/MODEL_GPT_5_2", "gpt"),
            ("us.anthropic.claude-sonnet-4-5-20250929-v1:0", "claude"),
            ("gpt4o", "gpt"),
            ("o3-mini", "gpt"),
            ("4o-mini", "other"),
            ("x1", "other"),
        ] {
            assert_eq!(classify(model), family, "{model}");
        }
        assert_eq!(label("qwen"), "Qwen");
        assert_eq!(label("other"), "其他");
        assert!(valid("qwen"));
        assert!(!valid("Qwen"));
        assert!(!valid("q"));
        assert!(!valid("qwen3"));
    }
}
