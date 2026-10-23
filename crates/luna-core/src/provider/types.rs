//! Conversation types shared by providers, the agent loop, and UIs.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Role of a message author.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

/// A block of content within a message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text { text: String },
    ToolUse { id: String, name: String, input: Value },
    ToolResult { tool_use_id: String, content: String, is_error: bool },
}

/// A single message in a conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: Vec<ContentBlock>,
}

impl Message {
    pub fn system(text: impl Into<String>) -> Self {
        Self { role: Role::System, content: vec![ContentBlock::Text { text: text.into() }] }
    }

    pub fn user(text: impl Into<String>) -> Self {
        Self { role: Role::User, content: vec![ContentBlock::Text { text: text.into() }] }
    }

    pub fn assistant_text(text: impl Into<String>) -> Self {
        Self { role: Role::Assistant, content: vec![ContentBlock::Text { text: text.into() }] }
    }

    pub fn tool_result(tool_use_id: impl Into<String>, content: impl Into<String>, is_error: bool) -> Self {
        Self {
            role: Role::Tool,
            content: vec![ContentBlock::ToolResult {
                tool_use_id: tool_use_id.into(),
                content: content.into(),
                is_error,
            }],
        }
    }

    /// Concatenated text of all text blocks (for display).
    pub fn text(&self) -> String {
        self.content
            .iter()
            .filter_map(|b| match b {
                ContentBlock::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("")
    }

    /// Tool-use blocks in this message as `(id, name, input)`.
    pub fn tool_uses(&self) -> Vec<(String, String, Value)> {
        self.content
            .iter()
            .filter_map(|b| match b {
                ContentBlock::ToolUse { id, name, input } => Some((id.clone(), name.clone(), input.clone())),
                _ => None,
            })
            .collect()
    }
}

/// A tool definition advertised to the model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    /// JSON Schema for the `input` object.
    pub parameters: Value,
}

/// A request for one streaming completion.
#[derive(Debug, Clone)]
pub struct CompletionRequest {
    pub model: String,
    pub system: Option<String>,
    pub messages: Vec<Message>,
    pub tools: Vec<ToolSpec>,
    pub max_tokens: u32,
    pub temperature: f32,
}

/// A chunk of a streaming completion.
#[derive(Debug, Clone)]
pub enum StreamChunk {
    /// Incremental text.
    TextDelta(String),
    /// A complete tool call (assembled from deltas by the provider).
    ToolUse { id: String, name: String, input: Value },
    /// Usage report at end of stream.
    Usage { input_tokens: u64, output_tokens: u64 },
    /// The stream finished with this stop reason.
    Done { stop_reason: StopReason },
}

/// Why a completion stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    EndTurn,
    ToolUse,
    MaxTokens,
    Cancelled,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_text_concatenates_blocks() {
        let m = Message {
            role: Role::Assistant,
            content: vec![
                ContentBlock::Text { text: "a".into() },
                ContentBlock::ToolUse { id: "t1".into(), name: "x".into(), input: Value::Null },
                ContentBlock::Text { text: "b".into() },
            ],
        };
        assert_eq!(m.text(), "ab");
        assert_eq!(m.tool_uses().len(), 1);
    }
}
