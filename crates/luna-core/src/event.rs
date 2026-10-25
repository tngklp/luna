//! The event stream both front-ends consume.
//!
//! The schema deliberately mirrors the one the ported piui renderer already
//! understands (`message_start`, `message_update`, `tool_execution_*`, …), so the
//! GUI needs no changes inside its components. Orchestration adds `agentId`,
//! `parentAgentId`, `depth` and `role`, which older consumers simply ignore.
//!
//! `AgentEvent` stays a plain internally-tagged enum; [`Emitted`] merges the
//! owning agent into the JSON object at the top level. Doing that merge by hand
//! avoids serde's restrictions on `flatten` with internally tagged enums.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::types::{ContentBlock, Role, ToolCall};

/// One assistant streaming step, shaped like the SDK event piui reads.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantUpdate {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delta: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_index: Option<usize>,
    /// The message as it stands mid-stream, so a tool block can be located.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub partial: Option<PartialMessage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call: Option<ToolCall>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartialMessage {
    pub content: Vec<ContentBlock>,
}

impl AssistantUpdate {
    fn new(kind: &str) -> Self {
        AssistantUpdate { kind: kind.to_string(), ..Default::default() }
    }

    pub fn text_delta(delta: impl Into<String>) -> Self {
        AssistantUpdate { delta: Some(delta.into()), ..Self::new("text_delta") }
    }

    pub fn text_end(content: impl Into<String>) -> Self {
        AssistantUpdate { content: Some(content.into()), ..Self::new("text_end") }
    }

    pub fn thinking_delta(delta: impl Into<String>) -> Self {
        AssistantUpdate { delta: Some(delta.into()), ..Self::new("thinking_delta") }
    }

    pub fn thinking_end(content: impl Into<String>) -> Self {
        AssistantUpdate { content: Some(content.into()), ..Self::new("thinking_end") }
    }

    pub fn tool_call_start(index: usize, partial: Vec<ContentBlock>) -> Self {
        AssistantUpdate {
            content_index: Some(index),
            partial: Some(PartialMessage { content: partial }),
            ..Self::new("toolcall_start")
        }
    }

    pub fn tool_call_delta(delta: impl Into<String>) -> Self {
        AssistantUpdate { delta: Some(delta.into()), ..Self::new("toolcall_delta") }
    }

    pub fn tool_call_end(call: ToolCall) -> Self {
        AssistantUpdate { tool_call: Some(call), ..Self::new("toolcall_end") }
    }

    pub fn error(reason: impl Into<String>) -> Self {
        AssistantUpdate { reason: Some(reason.into()), ..Self::new("error") }
    }
}

/// Where an event sits in the agent tree.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRef {
    pub agent_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_agent_id: Option<String>,
    #[serde(default)]
    pub depth: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
}

impl AgentRef {
    pub fn root(agent_id: impl Into<String>) -> Self {
        AgentRef { agent_id: agent_id.into(), parent_agent_id: None, depth: 0, role: None }
    }

    pub fn child(agent_id: impl Into<String>, parent: &AgentRef, role: impl Into<String>) -> Self {
        AgentRef {
            agent_id: agent_id.into(),
            parent_agent_id: Some(parent.agent_id.clone()),
            depth: parent.depth + 1,
            role: Some(role.into()),
        }
    }

    pub fn to_map(&self) -> Map<String, Value> {
        let mut map = Map::new();
        map.insert("agentId".into(), Value::String(self.agent_id.clone()));
        if let Some(parent) = &self.parent_agent_id {
            map.insert("parentAgentId".into(), Value::String(parent.clone()));
        }
        map.insert("depth".into(), Value::from(self.depth));
        if let Some(role) = &self.role {
            map.insert("role".into(), Value::String(role.clone()));
        }
        map
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamMessage {
    pub role: Role,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}

/// The streamed shape of a running tool's output, matching what piui reads.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolPartial {
    pub content: Vec<ContentBlock>,
}

impl ToolPartial {
    pub fn text(text: impl Into<String>) -> Self {
        ToolPartial { content: vec![ContentBlock::text(text)] }
    }
}

/// Everything the UI layer observes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentEvent {
    #[serde(rename_all = "camelCase")]
    MessageStart { message: StreamMessage },

    #[serde(rename_all = "camelCase")]
    MessageUpdate { assistant_message_event: AssistantUpdate },

    #[serde(rename_all = "camelCase")]
    MessageEnd {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        stop_reason: Option<String>,
    },

    #[serde(rename_all = "camelCase")]
    ToolExecutionStart {
        tool_call_id: String,
        tool_name: String,
        #[serde(default)]
        args: Value,
    },

    #[serde(rename_all = "camelCase")]
    ToolExecutionUpdate {
        tool_call_id: String,
        tool_name: String,
        #[serde(default)]
        args: Value,
        partial_result: ToolPartial,
    },

    #[serde(rename_all = "camelCase")]
    ToolExecutionEnd {
        tool_call_id: String,
        #[serde(default)]
        is_error: bool,
    },

    #[serde(rename_all = "camelCase")]
    AgentSettled {},

    #[serde(rename_all = "camelCase")]
    QueueUpdate {
        #[serde(default)]
        steering: Vec<String>,
        #[serde(default)]
        follow_up: Vec<String>,
    },

    #[serde(rename_all = "camelCase")]
    ThinkingLevelChanged { level: String },

    #[serde(rename_all = "camelCase")]
    SessionInfoChanged {
        session_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
    },

    /// A subagent joined the tree.
    #[serde(rename_all = "camelCase")]
    AgentSpawned {
        agent_id: String,
        parent_agent_id: String,
        role: String,
        title: String,
        depth: usize,
    },

    /// A subagent finished, successfully or otherwise.
    #[serde(rename_all = "camelCase")]
    AgentFinished {
        agent_id: String,
        parent_agent_id: String,
        status: String,
        summary: String,
        depth: usize,
    },

    /// A transient message for the user.
    #[serde(rename_all = "camelCase")]
    Notice { level: String, message: String },
}

/// An event plus the agent that produced it, ready to be serialized for a UI.
#[derive(Debug, Clone)]
pub struct Emitted {
    pub agent: Option<AgentRef>,
    pub event: AgentEvent,
}

impl Emitted {
    pub fn new(agent: AgentRef, event: AgentEvent) -> Self {
        Emitted { agent: Some(agent), event }
    }

    /// An event that belongs to no particular agent.
    pub fn global(event: AgentEvent) -> Self {
        Emitted { agent: None, event }
    }

    pub fn notice(level: impl Into<String>, message: impl Into<String>) -> Self {
        Emitted::global(AgentEvent::Notice { level: level.into(), message: message.into() })
    }

    /// The wire form: the event's own fields, plus the owning agent's identity.
    pub fn to_value(&self) -> Value {
        let mut map = match serde_json::to_value(&self.event) {
            Ok(Value::Object(map)) => map,
            Ok(other) => {
                let mut map = Map::new();
                map.insert("payload".into(), other);
                map
            }
            Err(_) => Map::new(),
        };
        if let Some(agent) = &self.agent {
            for (key, value) in agent.to_map() {
                map.insert(key, value);
            }
        }
        Value::Object(map)
    }

    pub fn to_json(&self) -> String {
        self.to_value().to_string()
    }
}

/// Sink for events. Implementations must not block the agent loop.
pub trait EventSink: Send + Sync + 'static {
    fn emit(&self, event: Emitted);
}

/// An event sink that drops everything, for tests and headless runs.
pub struct NullSink;

impl EventSink for NullSink {
    fn emit(&self, _event: Emitted) {}
}

impl<F> EventSink for F
where
    F: Fn(Emitted) + Send + Sync + 'static,
{
    fn emit(&self, event: Emitted) {
        self(event);
    }
}

/// Shared sink handle passed down the agent tree.
pub type SinkRef = std::sync::Arc<dyn EventSink>;
