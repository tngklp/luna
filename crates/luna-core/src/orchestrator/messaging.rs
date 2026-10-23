//! Inter-agent messaging: mailboxes, shared scratchpad, result aggregation.

use crate::event::{EventKind, LunaEvent};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

/// A message between agents (or from the orchestrator).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMessage {
    pub id: uuid::Uuid,
    pub from: String,
    pub to: String,
    pub kind: MessageKind,
    pub body: String,
    pub data: Option<Value>,
    pub sent_at: chrono::DateTime<chrono::Utc>,
}

/// What a message means to its receiver.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageKind {
    /// A task assignment (orchestrator → worker).
    Task,
    /// A partial or final result (worker → supervisor).
    Result,
    /// A question awaiting an answer.
    Query,
    /// A reply to a query.
    Reply,
    /// Free-form coordination chatter.
    Note,
}

impl MessageKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            MessageKind::Task => "task",
            MessageKind::Result => "result",
            MessageKind::Query => "query",
            MessageKind::Reply => "reply",
            MessageKind::Note => "note",
        }
    }
}

/// Receive-only handle for one agent's mailbox.
#[derive(Clone)]
pub struct Mailbox {
    agent_id: String,
    rx: Arc<Mutex<mpsc::Receiver<AgentMessage>>>,
}

/// Send handle into an agent's mailbox.
#[derive(Clone)]
pub struct MailboxTx {
    agent_id: String,
    tx: mpsc::Sender<AgentMessage>,
}

impl MailboxTx {
    pub fn agent_id(&self) -> &str {
        &self.agent_id
    }

    /// Deliver a message; errors only if the receiver was dropped.
    pub async fn send(&self, from: &str, kind: MessageKind, body: impl Into<String>, data: Option<Value>) -> Result<(), String> {
        let msg = AgentMessage {
            id: uuid::Uuid::new_v4(),
            from: from.to_string(),
            to: self.agent_id.clone(),
            kind,
            body: body.into(),
            data,
            sent_at: chrono::Utc::now(),
        };
        self.tx.send(msg).await.map_err(|e| e.to_string())
    }
}

impl Mailbox {
    /// Take all currently queued messages without awaiting.
    pub fn drain(&self) -> Vec<AgentMessage> {
        let mut rx = self.rx.lock().unwrap();
        let mut out = Vec::new();
        while let Ok(msg) = rx.try_recv() {
            out.push(msg);
        }
        out
    }

    /// Receive the next message, awaiting if empty. Honors cancellation.
    pub async fn recv(&self, cancel: &tokio_util::sync::CancellationToken) -> Option<AgentMessage> {
        let mut rx = self.rx.lock().unwrap();
        tokio::select! {
            msg = rx.recv() => msg,
            _ = cancel.cancelled() => None,
        }
    }

    pub fn agent_id(&self) -> &str {
        &self.agent_id
    }
}

/// Hub connecting all agents in a run.
#[derive(Default, Clone)]
pub struct MessageHub {
    boxes: Arc<Mutex<HashMap<String, MailboxTx>>>,
    /// Shared scratchpad all agents can read/write (key → note).
    scratchpad: Arc<Mutex<HashMap<String, String>>>,
}

impl MessageHub {
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a mailbox for `agent_id`; returns (rx handle, tx handle).
    pub fn register(&self, agent_id: &str) -> (Mailbox, MailboxTx) {
        let (tx, rx) = mpsc::channel(256);
        let tx_handle = MailboxTx { agent_id: agent_id.to_string(), tx };
        let rx_handle = Mailbox {
            agent_id: agent_id.to_string(),
            rx: Arc::new(Mutex::new(rx)),
        };
        self.boxes.lock().unwrap().insert(agent_id.to_string(), tx_handle.clone());
        (rx_handle, tx_handle)
    }

    /// Send to a named agent's mailbox.
    pub async fn send_to(
        &self,
        to: &str,
        from: &str,
        kind: MessageKind,
        body: impl Into<String>,
        data: Option<Value>,
    ) -> Result<(), String> {
        match self.boxes.lock().unwrap().get(to) {
            Some(tx) => tx.send(from, kind, body, data).await,
            None => Err(format!("no mailbox for agent `{to}`")),
        }
    }

    /// Broadcast to every registered agent except `from`.
    pub async fn broadcast(&self, from: &str, kind: MessageKind, body: impl Into<String>) {
        let body = body.into();
        let targets: Vec<MailboxTx> = self
            .boxes
            .lock()
            .unwrap()
            .values()
            .filter(|t| t.agent_id != from)
            .cloned()
            .collect();
        for t in targets {
            let _ = t.send(from, kind, body.clone(), None).await;
        }
    }

    pub fn agent_ids(&self) -> Vec<String> {
        let mut v: Vec<String> = self.boxes.lock().unwrap().keys().cloned().collect();
        v.sort();
        v
    }

    // ----- scratchpad -----

    pub fn scratch_set(&self, key: impl Into<String>, value: impl Into<String>) {
        self.scratchpad.lock().unwrap().insert(key.into(), value.into());
    }

    pub fn scratch_get(&self, key: &str) -> Option<String> {
        self.scratchpad.lock().unwrap().get(key).cloned()
    }

    /// Full scratchpad snapshot (for injecting into a supervisor prompt).
    pub fn scratch_snapshot(&self) -> Vec<(String, String)> {
        let mut v: Vec<(String, String)> = self.scratchpad.lock().unwrap().clone().into_iter().collect();
        v.sort();
        v
    }
}

/// Aggregate worker outputs into a supervisor-ready summary block.
pub fn aggregate_results(results: &HashMap<String, String>, errors: &HashMap<String, String>) -> String {
    let mut names: Vec<&String> = results.keys().chain(errors.keys()).collect();
    names.sort();
    let mut out = String::from("## Worker results\n");
    for name in &names {
        if let Some(r) = results.get(*name) {
            out.push_str(&format!("### {name}\n{r}\n\n"));
        } else if let Some(e) = errors.get(*name) {
            out.push_str(&format!("### {name} (failed)\n{e}\n\n"));
        }
    }
    out
}

/// Publish a message event on the bus for UI visibility.
pub fn publish_message_event(bus: &crate::event::EventBus, session_id: uuid::Uuid, msg: &AgentMessage) {
    bus.publish(LunaEvent::new(
        session_id,
        Some(msg.to.clone()),
        EventKind::OrchestratorTransition(format!(
            "message [{}] {} → {}: {}",
            msg.kind.as_str(),
            msg.from,
            msg.to,
            if msg.body.len() > 120 { format!("{}…", &msg.body[..120]) } else { msg.body.clone() }
        )),
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn mailbox_roundtrip() {
        let hub = MessageHub::new();
        let (rx, _tx) = hub.register("worker-a");
        hub.send_to("worker-a", "boss", MessageKind::Task, "do the thing", None)
            .await
            .unwrap();
        let msgs = rx.drain();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].from, "boss");
        assert_eq!(msgs[0].kind, MessageKind::Task);
        assert_eq!(msgs[0].body, "do the thing");
    }

    #[tokio::test]
    async fn broadcast_skips_sender() {
        let hub = MessageHub::new();
        let (rx_a, _) = hub.register("a");
        let (rx_b, _) = hub.register("b");
        hub.broadcast("a", MessageKind::Note, "hello all").await;
        assert!(rx_a.drain().is_empty());
        assert_eq!(rx_b.drain().len(), 1);
    }

    #[tokio::test]
    async fn unknown_mailbox_errors() {
        let hub = MessageHub::new();
        let err = hub.send_to("ghost", "a", MessageKind::Note, "x", None).await;
        assert!(err.is_err());
    }

    #[tokio::test]
    async fn scratchpad_works() {
        let hub = MessageHub::new();
        hub.scratch_set("findings", "nothing yet");
        assert_eq!(hub.scratch_get("findings"), Some("nothing yet".into()));
        assert_eq!(hub.scratch_get("nope"), None);
        assert_eq!(hub.scratch_snapshot().len(), 1);
    }

    #[test]
    fn aggregate_orders_and_marks_failures() {
        let mut results = HashMap::new();
        results.insert("beta".to_string(), "b ok".to_string());
        let mut errors = HashMap::new();
        errors.insert("alpha".to_string(), "a failed".to_string());
        let text = aggregate_results(&results, &errors);
        assert!(text.contains("### alpha (failed)"));
        assert!(text.contains("### beta\nb ok"));
        // alpha before beta
        assert!(text.find("alpha").unwrap() < text.find("beta").unwrap());
    }

    #[tokio::test]
    async fn recv_honors_cancel() {
        let hub = MessageHub::new();
        let (rx, _) = hub.register("a");
        let cancel = tokio_util::sync::CancellationToken::new();
        cancel.cancel();
        assert!(rx.recv(&cancel).await.is_none());
    }
}
