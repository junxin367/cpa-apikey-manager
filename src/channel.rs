//! Product channels describe model families, never upstream providers.
pub const ALL: [(&str, &str); 6] = [
    ("gpt", "GPT"),
    ("claude", "Claude"),
    ("deepseek", "DeepSeek"),
    ("glm", "GLM"),
    ("kimi", "Kimi"),
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
