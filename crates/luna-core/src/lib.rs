//! Luna core: event bus, agent loop, orchestrator, providers, tools.

pub mod config;
pub mod error;
pub mod event;
pub mod orchestrator;
pub mod provider;
pub mod storage;
pub mod tool;

pub use error::{Error, Result};

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

/// Handle for interacting with a running agent (or orchestrator run).
///
/// Commands flow in through `commands`, events flow out on the broadcast bus,
/// and `cancel_token` aborts the run.
#[derive(Debug, Clone)]
pub struct AgentHandle {
    pub commands: mpsc::Sender<Command>,
    pub cancel_token: CancellationToken,
    pub session_id: uuid::Uuid,
}

impl AgentHandle {
    pub fn new(commands: mpsc::Sender<Command>, cancel_token: CancellationToken) -> Self {
        Self {
            commands,
            cancel_token,
            session_id: uuid::Uuid::new_v4(),
        }
    }

    /// Request graceful cancellation of the run.
    pub fn cancel(&self) {
        self.cancel_token.cancel();
    }
}

/// Commands a UI can send into the core.
#[derive(Debug, Clone)]
pub enum Command {
    /// Send a user message to the agent.
    UserMessage(String),
    /// Respond to a pending approval request.
    Approval {
        request_id: uuid::Uuid,
        approved: bool,
    },
    /// Cancel the current run.
    Cancel,
}

/// A single event on the Luna bus.
#[derive(Debug, Clone)]
pub struct LunaEvent {
    pub session_id: uuid::Uuid,
    pub agent_id: Option<String>,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub kind: EventKind,
}

impl LunaEvent {
    pub fn new(session_id: uuid::Uuid, agent_id: Option<String>, kind: EventKind) -> Self {
        Self {
            session_id,
            agent_id,
            timestamp: chrono::Utc::now(),
            kind,
        }
    }
}

/// The kinds of events the core emits. UIs render these; storage persists them.
#[derive(Debug, Clone)]
pub enum EventKind {
    /// The run started.
    RunStarted,
    /// A user message was accepted.
    UserMessage(String),
    /// The assistant produced a complete message.
    AssistantMessage(String),
    /// Streaming text delta (already coalesced by the bus).
    TokenDelta(String),
    /// A tool call was requested by the model.
    ToolCallRequested {
        call_id: String,
        name: String,
        arguments: serde_json::Value,
    },
    /// A tool call finished.
    ToolCallFinished {
        call_id: String,
        name: String,
        outcome: tool::ToolOutcome,
    },
    /// A tool needs user approval before running.
    ApprovalRequested {
        request_id: uuid::Uuid,
        agent_id: String,
        tool_name: String,
        arguments: serde_json::Value,
    },
    /// The approval was resolved (by UI or config).
    ApprovalResolved {
        request_id: uuid::Uuid,
        approved: bool,
    },
    /// Orchestrator spawned a child agent.
    AgentSpawned {
        agent_id: String,
        parent: Option<String>,
        spec_name: String,
    },
    /// Orchestrator transition (fan-out, join, delegation).
    OrchestratorTransition(String),
    /// The run finished (success or error).
    RunFinished {
        summary: Option<String>,
        error: Option<String>,
    },
}

impl EventKind {
    /// Coalescing key: token deltas with the same key merge into one event.
    pub fn coalesce_key(&self) -> Option<&str> {
        match self {
            EventKind::TokenDelta(_) => Some("token_delta"),
            _ => None,
        }
    }

    /// Merge `other` into `self` if both are coalescable of the same kind.
    pub fn coalesce(&mut self, other: EventKind) -> bool {
        match (self, other) {
            (EventKind::TokenDelta(a), EventKind::TokenDelta(b)) => {
                a.push_str(&b);
                true
            }
            _ => false,
        }
    }
}

/// Broadcast bus shared by core and UIs.
#[derive(Debug, Clone)]
pub struct EventBus {
    tx: tokio::sync::broadcast::Sender<LunaEvent>,
}

impl EventBus {
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = tokio::sync::broadcast::channel(capacity);
        Self { tx }
    }

    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<LunaEvent> {
        self.tx.subscribe()
    }

    /// Publish an event, coalescing token deltas when the channel is full.
    pub fn publish(&self, event: LunaEvent) {
        let _ = self.tx.send(event);
    }

    /// Publish an event. Token-delta coalescing happens in the UI layer at
    /// render time; the bus keeps every event so storage never loses data.
    pub fn publish_coalesced(&self, event: LunaEvent) {
        self.publish(event);
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new(4096)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn bus_delivers_to_subscribers() {
        let bus = EventBus::default();
        let mut rx = bus.subscribe();
        bus.publish(LunaEvent::new(
            uuid::Uuid::new_v4(),
            None,
            EventKind::RunStarted,
        ));
        let ev = rx.recv().await.expect("event");
        assert!(matches!(ev.kind, EventKind::RunStarted));
    }

    #[tokio::test]
    async fn token_deltas_coalesce() {
        let mut a = EventKind::TokenDelta("hello ".into());
        assert!(a.coalesce(EventKind::TokenDelta("world".into())));
        match a {
            EventKind::TokenDelta(s) => assert_eq!(s, "hello world"),
            _ => unreachable!(),
        }
    }
}
