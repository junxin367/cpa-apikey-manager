//! Product channels describe model families. Antigravity is the one upstream-based channel:
//! it is known only after the host selects credentials, from the upstream request format.
pub const ALL: [(&str, &str); 7] = [
    ("gpt", "GPT"),
    ("claude", "Claude"),
    ("deepseek", "DeepSeek"),
    ("glm", "GLM"),
    ("kimi", "Kimi"),
    ("antigravity", "Antigravity"),
    ("other", "其他"),
];

pub fn label(id: &str) -> &'static str {
    ALL.iter()
        .find(|(key, _)| *key == id)
        .map_or("其他", |(_, name)| name)
}

pub fn valid(id: &str) -> bool {
    ALL.iter().any(|(key, _)| *key == id)
}

/// Channel of one execution. `upstream_format` is the host's `ToFormat`, which is set only
/// after credential selection; any model served by Antigravity credentials counts as Antigravity.
pub fn resolve(model: &str, upstream_format: Option<&str>) -> &'static str {
    if upstream_format.is_some_and(|format| format.trim().eq_ignore_ascii_case("antigravity")) {
        "antigravity"
    } else {
        classify(model)
    }
}

pub fn classify(model: &str) -> &'static str {
    let name = model
        .rsplit('/')
        .find(|part| !part.is_empty())
        .unwrap_or("")
        .to_ascii_lowercase();
    let root = name.split(['-', '_', '.', ':']).next().unwrap_or("");
    match root {
        "gpt" | "chatgpt" | "codex" => "gpt",
        "claude" => "claude",
        "deepseek" => "deepseek",
        "glm" => "glm",
        "kimi" | "moonshot" => "kimi",
        _ if root.strip_prefix('o').is_some_and(|n| {
            n.starts_with(|c: char| ('1'..='9').contains(&c))
                && n.bytes().all(|c| c.is_ascii_digit())
        }) =>
        {
            "gpt"
        }
        _ => "other",
    }
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
    fn antigravity_comes_from_upstream_format_only() {
        assert!(valid("antigravity"));
        assert_eq!(label("antigravity"), "Antigravity");
        assert_eq!(
            resolve("claude-sonnet-4-5", Some("antigravity")),
            "antigravity"
        );
        assert_eq!(
            resolve("gemini-3-pro-high", Some("Antigravity")),
            "antigravity"
        );
        assert_eq!(resolve("claude-sonnet-4-5", Some("claude")), "claude");
        assert_eq!(resolve("claude-sonnet-4-5", None), "claude");
        assert_eq!(classify("antigravity"), "other");
    }
}
