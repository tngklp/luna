//! The vocabulary shared by every layer: messages, blocks, usage, models.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::System => "system",
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::Tool => "tool",
        }
    }
}

/// A tool invocation requested by the model.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub arguments: serde_json::Value,
}

/// The outcome of running a {@link ToolCall}.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolResult {
    pub tool_call_id: String,
    pub content: String,
    #[serde(default)]
    pub is_error: bool,
}

/// A piece of a message.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ContentBlock {
    Text { text: String },
    Thinking { text: String },
    ToolCall { id: String, name: String, arguments: serde_json::Value },
    ToolResult { tool_call_id: String, content: String, is_error: bool },
    Image { mime_type: String, data: String },
}

impl ContentBlock {
    pub fn text(text: impl Into<String>) -> Self {
        ContentBlock::Text { text: text.into() }
    }

    pub fn as_text(&self) -> Option<&str> {
        match self {
            ContentBlock::Text { text } => Some(text),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub role: Role,
    pub blocks: Vec<ContentBlock>,
}

impl Message {
    pub fn user(text: impl Into<String>) -> Self {
        Message { role: Role::User, blocks: vec![ContentBlock::text(text)] }
    }

    pub fn assistant(blocks: Vec<ContentBlock>) -> Self {
        Message { role: Role::Assistant, blocks }
    }

    pub fn tool(
        tool_call_id: impl Into<String>,
        content: impl Into<String>,
        is_error: bool,
    ) -> Self {
        Message {
            role: Role::Tool,
            blocks: vec![ContentBlock::ToolResult {
                tool_call_id: tool_call_id.into(),
                content: content.into(),
                is_error,
            }],
        }
    }

    /// Concatenated text of the message: text blocks, plus tool results.
    ///
    /// Tool results count as text because that is what they are to a reader —
    /// the output of a call — and callers such as the transcript and the
    /// subagent report want it without special-casing the role.
    pub fn text(&self) -> String {
        let mut out = String::new();
        for block in &self.blocks {
            match block {
                ContentBlock::Text { text } => out.push_str(text),
                ContentBlock::ToolResult { content, .. } => out.push_str(content),
                _ => {}
            }
        }
        out
    }

    pub fn tool_calls(&self) -> Vec<ToolCall> {
        self.blocks
            .iter()
            .filter_map(|block| match block {
                ContentBlock::ToolCall { id, name, arguments } => Some(ToolCall {
                    id: id.clone(),
                    name: name.clone(),
                    arguments: arguments.clone(),
                }),
                _ => None,
            })
            .collect()
    }

    pub fn thinking(&self) -> String {
        let mut out = String::new();
        for block in &self.blocks {
            if let ContentBlock::Thinking { text } = block {
                out.push_str(text);
            }
        }
        out
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    #[serde(default)]
    pub input: u32,
    #[serde(default)]
    pub output: u32,
    #[serde(default)]
    pub cache_read: u32,
    #[serde(default)]
    pub cache_write: u32,
}

impl Usage {
    pub fn total(&self) -> u32 {
        self.input + self.output
    }

    pub fn add(&mut self, other: Usage) {
        self.input += other.input;
        self.output += other.output;
        self.cache_read += other.cache_read;
        self.cache_write += other.cache_write;
    }
}

/// Per-million-token pricing, in USD.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cost {
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
    pub cache_write: f64,
}

impl Cost {
    pub fn of(&self, usage: &Usage) -> f64 {
        let per = |tokens: u32, rate: f64| (tokens as f64 / 1_000_000.0) * rate;
        per(usage.input, self.input)
            + per(usage.output, self.output)
            + per(usage.cache_read, self.cache_read)
            + per(usage.cache_write, self.cache_write)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub provider: String,
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub reasoning: bool,
    pub context_window: u32,
    pub max_tokens: u32,
    #[serde(default)]
    pub input: Vec<String>,
    #[serde(default)]
    pub cost: Cost,
}

impl ModelInfo {
    pub fn supports_images(&self) -> bool {
        self.input.iter().any(|kind| kind == "image")
    }
}

/// How much reasoning effort the model should spend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThinkingLevel {
    Off,
    Minimal,
    Low,
    #[default]
    Medium,
    High,
    Xhigh,
    Max,
}

impl ThinkingLevel {
    pub const ALL: [ThinkingLevel; 7] = [
        ThinkingLevel::Off,
        ThinkingLevel::Minimal,
        ThinkingLevel::Low,
        ThinkingLevel::Medium,
        ThinkingLevel::High,
        ThinkingLevel::Xhigh,
        ThinkingLevel::Max,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            ThinkingLevel::Off => "off",
            ThinkingLevel::Minimal => "minimal",
            ThinkingLevel::Low => "low",
            ThinkingLevel::Medium => "medium",
            ThinkingLevel::High => "high",
            ThinkingLevel::Xhigh => "xhigh",
            ThinkingLevel::Max => "max",
        }
    }

    /// Anthropic "budget_tokens" for extended thinking, if the level asks for it.
    pub fn thinking_budget(self) -> Option<u32> {
        match self {
            ThinkingLevel::Off | ThinkingLevel::Minimal => None,
            ThinkingLevel::Low => Some(2_000),
            ThinkingLevel::Medium => Some(6_000),
            ThinkingLevel::High => Some(12_000),
            ThinkingLevel::Xhigh => Some(24_000),
            ThinkingLevel::Max => Some(32_000),
        }
    }

    /// Rough effort percentage used by OpenAI-style `reasoning_effort`.
    pub fn reasoning_effort(self) -> Option<&'static str> {
        match self {
            ThinkingLevel::Off => None,
            ThinkingLevel::Minimal => Some("minimal"),
            ThinkingLevel::Low => Some("low"),
            ThinkingLevel::Medium => Some("medium"),
            ThinkingLevel::High => Some("high"),
            ThinkingLevel::Xhigh | ThinkingLevel::Max => Some("high"),
        }
    }

    pub fn next(self) -> ThinkingLevel {
        let all = ThinkingLevel::ALL;
        let index = all.iter().position(|level| *level == self).unwrap_or(3);
        all[(index + 1) % all.len()]
    }

    pub fn from_str_lossy(value: &str) -> ThinkingLevel {
        ThinkingLevel::ALL.iter().copied().find(|level| level.as_str() == value).unwrap_or_default()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    EndTurn,
    ToolUse,
    MaxTokens,
    StopSequence,
    Refusal,
    Error,
}

/// What a provider needs to know about a callable tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolSchema {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}
