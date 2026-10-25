//! The wire types the renderer expects.
//!
//! These mirror `gui/src/shared/ipc.ts` field for field. The ported piui
//! components read them directly, so a rename here is a breaking change there.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use luna_core::config::{ApprovalAction, ApprovalConfig as CoreApprovals};
use luna_core::types::{ContentBlock, Message, ModelInfo, Role, ThinkingLevel, Usage};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub name: String,
    pub version: String,
    pub platform: String,
    pub versions: Versions,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Versions {
    pub tauri: String,
    pub rust: String,
    pub luna: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeInfoDto {
    /// The engine version, shown as the "Pi" version in the footer.
    pub sdk_version: String,
    pub cli_version: Option<String>,
    /// Whether a provider is usable. The banner keys off this.
    pub cli_installed: bool,
    pub version_match: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelDto {
    pub provider: String,
    pub id: String,
    pub name: String,
    pub reasoning: bool,
    pub context_window: u32,
    pub max_tokens: u32,
    pub input: Vec<String>,
    pub cost: CostDto,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CostDto {
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
    pub cache_write: f64,
}

impl From<ModelInfo> for ModelDto {
    fn from(model: ModelInfo) -> Self {
        ModelDto {
            provider: model.provider,
            id: model.id,
            name: model.name,
            reasoning: model.reasoning,
            context_window: model.context_window,
            max_tokens: model.max_tokens,
            input: model.input,
            cost: CostDto {
                input: model.cost.input,
                output: model.cost.output,
                cache_read: model.cost.cache_read,
                cache_write: model.cost.cache_write,
            },
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextUsageDto {
    pub tokens: Option<u32>,
    pub context_window: u32,
    pub percent: Option<f32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TokensDto {
    pub input: u32,
    pub output: u32,
    pub cache_read: u32,
    pub cache_write: u32,
    pub total: u32,
}

impl From<Usage> for TokensDto {
    fn from(usage: Usage) -> Self {
        TokensDto {
            input: usage.input,
            output: usage.output,
            cache_read: usage.cache_read,
            cache_write: usage.cache_write,
            total: usage.total(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionStatusDto {
    pub cwd: String,
    pub model: Option<ModelDto>,
    pub thinking_level: String,
    pub available_thinking_levels: Vec<String>,
    pub supports_thinking: bool,
    pub is_streaming: bool,
    pub is_compacting: bool,
    pub is_retrying: bool,
    pub session_id: String,
    pub session_file: Option<String>,
    pub session_name: Option<String>,
    pub message_count: usize,
    pub pending_message_count: usize,
    pub steering: Vec<String>,
    pub follow_up: Vec<String>,
    pub context_usage: Option<ContextUsageDto>,
    pub cost: f64,
    pub tokens: TokensDto,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummaryDto {
    pub path: String,
    pub id: String,
    pub cwd: String,
    pub name: Option<String>,
    pub created: String,
    pub modified: String,
    pub message_count: usize,
    pub first_message: String,
    pub parent_session_path: Option<String>,
}

impl From<luna_core::session::SessionSummary> for SessionSummaryDto {
    fn from(summary: luna_core::session::SessionSummary) -> Self {
        SessionSummaryDto {
            path: summary.path,
            id: summary.id,
            cwd: summary.cwd,
            name: summary.name,
            created: summary.created,
            modified: summary.modified,
            message_count: summary.message_count,
            first_message: summary.first_message,
            parent_session_path: summary.parent_session_path,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceDto {
    pub path: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandDto {
    pub name: String,
    pub description: String,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub argument_hint: Option<String>,
}

/// One entry in the transcript, in the shape the chat surface renders.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ChatItemDto {
    #[serde(rename_all = "camelCase")]
    User { id: String, text: String, image_count: usize, entry_id: Option<String> },
    #[serde(rename_all = "camelCase")]
    Assistant {
        id: String,
        blocks: Vec<ChatBlockDto>,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        stopped: Option<bool>,
        entry_id: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    Tool {
        id: String,
        tool_call_id: String,
        name: String,
        arguments: Value,
        #[serde(skip_serializing_if = "Option::is_none")]
        text: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        is_error: Option<bool>,
        #[serde(skip_serializing_if = "Option::is_none")]
        running: Option<bool>,
        #[serde(skip_serializing_if = "Option::is_none")]
        file_path: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        duration_ms: Option<u64>,
    },
    #[serde(rename_all = "camelCase")]
    Bash { id: String, command: String, output: String, exit_code: Option<i32> },
}

impl ChatItemDto {
    /// A bash call, rendered as a terminal card rather than a generic tool.
    pub fn bash(id: impl Into<String>, command: impl Into<String>, output: impl Into<String>, exit_code: Option<i32>) -> Self {
        ChatItemDto::Bash {
            id: id.into(),
            command: command.into(),
            output: output.into(),
            exit_code,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ChatBlockDto {
    #[serde(rename_all = "camelCase")]
    Text { text: String },
    #[serde(rename_all = "camelCase")]
    Thinking { text: String },
    #[serde(rename_all = "camelCase")]
    ToolCall { id: String, name: String, arguments: Value },
}

/// Convert a stored message into the renderer's item shape.
///
/// A tool result becomes its own item rather than being folded into the
/// assistant message, because the transcript renders it as a card.
pub fn to_chat_items(messages: &[Message]) -> Vec<ChatItemDto> {
    let mut items = Vec::with_capacity(messages.len());
    for (index, message) in messages.iter().enumerate() {
        let id = format!("m{index}");
        match message.role {
            Role::User => {
                let image_count = message
                    .blocks
                    .iter()
                    .filter(|block| matches!(block, ContentBlock::Image { .. }))
                    .count();
                items.push(ChatItemDto::User {
                    id: id.clone(),
                    text: message.text(),
                    image_count,
                    entry_id: Some(id),
                });
            }
            Role::Assistant => {
                let blocks: Vec<ChatBlockDto> = message
                    .blocks
                    .iter()
                    .filter_map(|block| match block {
                        ContentBlock::Text { text } => {
                            Some(ChatBlockDto::Text { text: text.clone() })
                        }
                        ContentBlock::Thinking { text } => {
                            Some(ChatBlockDto::Thinking { text: text.clone() })
                        }
                        ContentBlock::ToolCall { id, name, arguments } => {
                            Some(ChatBlockDto::ToolCall {
                                id: id.clone(),
                                name: name.clone(),
                                arguments: arguments.clone(),
                            })
                        }
                        _ => None,
                    })
                    .collect();
                items.push(ChatItemDto::Assistant {
                    id: id.clone(),
                    blocks,
                    error: None,
                    stopped: None,
                    entry_id: Some(id),
                });
            }
            Role::Tool => {
                for block in &message.blocks {
                    if let ContentBlock::ToolResult { tool_call_id, content, is_error } = block {
                        // A bash result reads better as a terminal card, so it
                        // gets its own item kind rather than a generic tool card.
                        if let Some(command) = bash_command_for(messages, tool_call_id) {
                            items.push(ChatItemDto::bash(
                                format!("{id}-{tool_call_id}"),
                                command,
                                content.clone(),
                                if *is_error { Some(1) } else { Some(0) },
                            ));
                            continue;
                        }
                        items.push(ChatItemDto::Tool {
                            id: format!("{id}-{tool_call_id}"),
                            tool_call_id: tool_call_id.clone(),
                            name: "tool".to_string(),
                            arguments: Value::Null,
                            text: Some(content.clone()),
                            is_error: Some(*is_error),
                            running: Some(false),
                            file_path: None,
                            duration_ms: None,
                        });
                    }
                }
            }
            Role::System => {}
        }
    }
    items
}

/// Find the command behind a bash tool call, so its result can be rendered as a
/// terminal card.
fn bash_command_for(messages: &[Message], tool_call_id: &str) -> Option<String> {
    for message in messages {
        for block in &message.blocks {
            if let ContentBlock::ToolCall { id, name, arguments } = block {
                if id == tool_call_id && name == "bash" {
                    return arguments
                        .get("command")
                        .and_then(|command| command.as_str())
                        .map(|command| command.to_string());
                }
            }
        }
    }
    None
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptInput {
    pub text: String,
    #[serde(default)]
    pub images: Option<Vec<ImageInput>>,
    /// `steer` or `followUp` when the agent is already streaming.
    #[serde(default)]
    pub streaming_behavior: Option<String>,
}

impl PromptInput {
    /// The images as content blocks the providers understand.
    pub fn image_blocks(&self) -> Vec<ContentBlock> {
        self.images
            .as_ref()
            .map(|images| {
                images
                    .iter()
                    .map(|image| ContentBlock::Image {
                        mime_type: image.mime_type.clone(),
                        data: image.data.clone(),
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageInput {
    pub data: String,
    pub mime_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "camelCase")]
pub enum UiRequestDto {
    #[serde(rename_all = "camelCase")]
    Approval { id: String, tool: String, title: String, detail: String },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UiResponseDto {
    pub id: String,
    pub answer: String,
}

/// Approval policy in the shape the settings dialog edits.
///
/// `defaultPolicy` covers anything unlisted; `tools` holds one entry per class
/// (`read`, `write`, `bash`, `network`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalConfigDto {
    pub default_policy: String,
    pub tools: std::collections::BTreeMap<String, String>,
}

impl From<CoreApprovals> for ApprovalConfigDto {
    fn from(config: CoreApprovals) -> Self {
        let tools = std::collections::BTreeMap::from([
            ("read".to_string(), config.read.as_str().to_string()),
            ("write".to_string(), config.write.as_str().to_string()),
            ("bash".to_string(), config.bash.as_str().to_string()),
            ("network".to_string(), config.network.as_str().to_string()),
        ]);
        ApprovalConfigDto { default_policy: "ask".to_string(), tools }
    }
}

fn parse_action(value: &str) -> ApprovalAction {
    match value {
        "allow" => ApprovalAction::Allow,
        "deny" => ApprovalAction::Deny,
        _ => ApprovalAction::Ask,
    }
}

impl From<ApprovalConfigDto> for CoreApprovals {
    fn from(config: ApprovalConfigDto) -> Self {
        let get = |key: &str, fallback: ApprovalAction| {
            config.tools.get(key).map(|value| parse_action(value)).unwrap_or(fallback)
        };
        let default = parse_action(&config.default_policy);
        CoreApprovals {
            read: get("read", default),
            write: get("write", default),
            bash: get("bash", default),
            network: get("network", default),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingChangeDto {
    pub path: String,
    pub added: usize,
    pub removed: usize,
    pub diff: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangesStateDto {
    pub changes: Vec<PendingChangeDto>,
    pub revision: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FsFileDto {
    pub path: String,
    pub content: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelDefDto {
    pub id: String,
    pub name: String,
    pub reasoning: bool,
    #[serde(default)]
    pub input: Vec<String>,
    pub context_window: u32,
    pub max_tokens: u32,
    #[serde(default)]
    pub cost: CostDto,
}

impl From<ModelInfo> for ModelDefDto {
    fn from(model: ModelInfo) -> Self {
        ModelDefDto {
            id: model.id,
            name: model.name,
            reasoning: model.reasoning,
            input: model.input,
            context_window: model.context_window,
            max_tokens: model.max_tokens,
            cost: CostDto {
                input: model.cost.input,
                output: model.cost.output,
                cache_read: model.cost.cache_read,
                cache_write: model.cost.cache_write,
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderConfigDto {
    pub id: String,
    #[serde(default)]
    pub preset: Option<String>,
    pub kind: String,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub api_key_env: Option<String>,
    #[serde(default = "yes")]
    pub discover_models: bool,
    #[serde(default)]
    pub models: Vec<ModelDefDto>,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelsConfigDto {
    /// Absolute path of the file the catalogue is read from.
    pub path: String,
    pub default_model: String,
    pub providers: Vec<ProviderConfigDto>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelsUpdateResultDto {
    pub config: ModelsConfigDto,
    pub models: Vec<ModelDto>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStateDto {
    pub phase: String,
    pub version: Option<String>,
    pub notes: Option<String>,
    pub error: Option<String>,
}

impl UpdateStateDto {
    pub fn idle() -> Self {
        UpdateStateDto {
            phase: "idle".to_string(),
            version: None,
            notes: None,
            error: None,
        }
    }

    pub fn up_to_date(current: &str) -> Self {
        UpdateStateDto {
            phase: "up-to-date".to_string(),
            version: Some(current.to_string()),
            notes: None,
            error: None,
        }
    }

    pub fn available(tag: &str, notes: String, current: &str) -> Self {
        UpdateStateDto {
            phase: "available".to_string(),
            version: Some(tag.to_string()),
            notes: Some(if notes.is_empty() {
                format!("Luna {tag} is available. You are on {current}.")
            } else {
                notes
            }),
            error: None,
        }
    }

    pub fn error(message: String) -> Self {
        UpdateStateDto {
            phase: "error".to_string(),
            version: None,
            notes: None,
            error: Some(message),
        }
    }
}

/// The thinking levels the picker offers.
pub fn thinking_levels() -> Vec<String> {
    ThinkingLevel::ALL.iter().map(|level| level.as_str().to_string()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn user_messages_become_user_items() {
        let items = to_chat_items(&[Message::user("hello")]);
        assert_eq!(items.len(), 1);
        let value = serde_json::to_value(&items[0]).unwrap();
        assert_eq!(value["kind"], json!("user"));
        assert_eq!(value["text"], json!("hello"));
    }

    #[test]
    fn assistant_blocks_keep_their_kind() {
        let message = Message::assistant(vec![
            ContentBlock::Thinking { text: "hmm".into() },
            ContentBlock::text("answer"),
            ContentBlock::ToolCall { id: "c1".into(), name: "read".into(), arguments: json!({}) },
        ]);
        let items = to_chat_items(&[message]);
        let value = serde_json::to_value(&items[0]).unwrap();
        assert_eq!(value["kind"], json!("assistant"));
        assert_eq!(value["blocks"][0]["type"], json!("thinking"));
        assert_eq!(value["blocks"][1]["type"], json!("text"));
        assert_eq!(value["blocks"][2]["type"], json!("toolCall"));
    }

    #[test]
    fn tool_results_become_tool_items() {
        let items = to_chat_items(&[Message::tool("c1", "output", false)]);
        let value = serde_json::to_value(&items[0]).unwrap();
        assert_eq!(value["kind"], json!("tool"));
        assert_eq!(value["toolCallId"], json!("c1"));
        assert_eq!(value["text"], json!("output"));
    }

    #[test]
    fn approval_config_round_trips() {
        let dto = ApprovalConfigDto {
            default_policy: "ask".into(),
            tools: std::collections::BTreeMap::from([
                ("read".to_string(), "allow".to_string()),
                ("write".to_string(), "ask".to_string()),
                ("bash".to_string(), "deny".to_string()),
                ("network".to_string(), "allow".to_string()),
            ]),
        };
        let core: CoreApprovals = dto.clone().into();
        assert_eq!(core.bash, ApprovalAction::Deny);
        assert_eq!(core.read, ApprovalAction::Allow);

        let back: ApprovalConfigDto = core.into();
        assert_eq!(back.tools["write"], "ask");
        assert_eq!(back.tools["bash"], "deny");
    }

    #[test]
    fn an_unlisted_class_falls_back_to_the_default_policy() {
        let dto = ApprovalConfigDto {
            default_policy: "deny".into(),
            tools: std::collections::BTreeMap::new(),
        };
        let core: CoreApprovals = dto.into();
        assert_eq!(core.read, ApprovalAction::Deny);
        assert_eq!(core.bash, ApprovalAction::Deny);
    }

    #[test]
    fn model_dtos_carry_pricing() {
        let dto: ModelDto = luna_core::catalog::describe("anthropic", "claude-sonnet-4-5").into();
        assert_eq!(dto.context_window, 200_000);
        assert!(dto.cost.input > 0.0);
    }
}
