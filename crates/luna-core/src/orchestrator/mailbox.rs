//! Messaging between agents in a tree.
//!
//! A subagent can DM one peer or broadcast to the whole tree. Messages are
//! queued rather than delivered mid-turn, so an agent reads them at a natural
//! boundary instead of having its context rewritten underneath it.

use std::collections::BTreeMap;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

/// One message.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub from: String,
    /// `None` means everyone.
    pub to: Option<String>,
    pub body: String,
    pub at: String,
}

impl Message {
    pub fn render(&self) -> String {
        match &self.to {
            Some(to) => format!("[{} → {}] {}", self.from, to, self.body),
            None => format!("[{} → all] {}", self.from, self.body),
        }
    }
}

/// The mailbox for a whole tree.
#[derive(Debug, Default)]
pub struct Mailbox {
    /// Undelivered messages, keyed by recipient.
    pending: Mutex<BTreeMap<String, Vec<Message>>>,
    /// Everything sent, for the UI.
    log: Mutex<Vec<Message>>,
}

impl Mailbox {
    pub fn new() -> Self {
        Mailbox::default()
    }

    /// Send to one agent.
    pub fn send(&self, from: &str, to: &str, body: impl Into<String>) {
        let message = Message {
            from: from.to_string(),
            to: Some(to.to_string()),
            body: body.into(),
            at: chrono::Utc::now().to_rfc3339(),
        };
        if let Ok(mut pending) = self.pending.lock() {
            pending.entry(to.to_string()).or_default().push(message.clone());
        }
        if let Ok(mut log) = self.log.lock() {
            log.push(message);
        }
    }

    /// Send to everyone except the sender.
    pub fn broadcast(&self, from: &str, body: impl Into<String>, recipients: &[String]) {
        let body = body.into();
        for recipient in recipients {
            if recipient == from {
                continue;
            }
            let message = Message {
                from: from.to_string(),
                to: None,
                body: body.clone(),
                at: chrono::Utc::now().to_rfc3339(),
            };
            if let Ok(mut pending) = self.pending.lock() {
                pending.entry(recipient.clone()).or_default().push(message.clone());
            }
            if let Ok(mut log) = self.log.lock() {
                log.push(message);
            }
        }
    }

    /// Take everything waiting for one agent.
    pub fn drain(&self, agent: &str) -> Vec<Message> {
        self.pending.lock().ok().and_then(|mut pending| pending.remove(agent)).unwrap_or_default()
    }

    /// Whether anything is waiting.
    pub fn has_mail(&self, agent: &str) -> bool {
        self.pending
            .lock()
            .map(|pending| pending.get(agent).map(|list| !list.is_empty()).unwrap_or(false))
            .unwrap_or(false)
    }

    /// Everything sent, oldest first.
    pub fn log(&self) -> Vec<Message> {
        self.log.lock().map(|log| log.clone()).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_direct_message_reaches_only_its_recipient() {
        let mailbox = Mailbox::new();
        mailbox.send("a", "b", "hello");
        assert!(mailbox.has_mail("b"));
        assert!(!mailbox.has_mail("c"));
        assert_eq!(mailbox.drain("b").len(), 1);
        assert!(!mailbox.has_mail("b"));
    }

    #[test]
    fn a_broadcast_skips_the_sender() {
        let mailbox = Mailbox::new();
        let everyone = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        mailbox.broadcast("a", "heads up", &everyone);
        assert!(!mailbox.has_mail("a"));
        assert!(mailbox.has_mail("b"));
        assert!(mailbox.has_mail("c"));
    }

    #[test]
    fn messages_are_drained_in_order() {
        let mailbox = Mailbox::new();
        mailbox.send("a", "b", "first");
        mailbox.send("a", "b", "second");
        let drained = mailbox.drain("b");
        assert_eq!(drained[0].body, "first");
        assert_eq!(drained[1].body, "second");
    }

    #[test]
    fn the_log_keeps_everything_sent() {
        let mailbox = Mailbox::new();
        mailbox.send("a", "b", "one");
        mailbox.broadcast("a", "two", &["b".to_string()]);
        assert_eq!(mailbox.log().len(), 2);
    }

    #[test]
    fn a_message_renders_its_route() {
        let direct = Message {
            from: "a".into(),
            to: Some("b".into()),
            body: "hi".into(),
            at: String::new(),
        };
        assert_eq!(direct.render(), "[a → b] hi");

        let broadcast = Message { to: None, ..direct };
        assert_eq!(broadcast.render(), "[a → all] hi");
    }
}
