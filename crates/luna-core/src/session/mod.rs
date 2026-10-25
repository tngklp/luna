//! Session persistence.
//!
//! Two stores, each doing what it is good at:
//!
//! * **JSONL** — the transcript, appended one message per line. Crash-safe,
//!   greppable, and cheap to resume or fork.
//! * **SQLite** — an index over those files for fast listing, search and
//!   metadata updates, plus the change log the UI reviews.

pub mod jsonl;
pub mod sqlite;

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::types::Message;

pub use jsonl::Transcript;
pub use sqlite::SessionIndex;

/// A session as the sidebar shows it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummary {
    pub path: String,
    pub id: String,
    pub cwd: String,
    pub name: Option<String>,
    /// ISO 8601.
    pub created: String,
    /// ISO 8601.
    pub modified: String,
    pub message_count: usize,
    /// First user message, used as a fallback title.
    pub first_message: String,
    pub parent_session_path: Option<String>,
}

/// Header written as the first line of every transcript.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionHeader {
    pub id: String,
    pub cwd: String,
    pub model: String,
    pub created: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub parent: Option<String>,
}

impl SessionHeader {
    pub fn new(id: impl Into<String>, cwd: impl Into<String>, model: impl Into<String>) -> Self {
        SessionHeader {
            id: id.into(),
            cwd: cwd.into(),
            model: model.into(),
            created: Utc::now().to_rfc3339(),
            name: None,
            parent: None,
        }
    }
}

/// One line of a transcript.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Entry {
    Header(SessionHeader),
    Message {
        message: Message,
        at: String,
    },
    /// A note the UI shows but the model does not see.
    Note {
        level: String,
        text: String,
        at: String,
    },
}

impl Entry {
    pub fn message(message: Message) -> Self {
        Entry::Message { message, at: Utc::now().to_rfc3339() }
    }

    pub fn note(level: impl Into<String>, text: impl Into<String>) -> Self {
        Entry::Note { level: level.into(), text: text.into(), at: Utc::now().to_rfc3339() }
    }
}

/// A file the agent changed, awaiting review.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingChange {
    pub path: String,
    pub added: usize,
    pub removed: usize,
    /// Unified diff, for the review pane.
    pub diff: String,
    pub at: String,
}

/// Everything a front-end needs to open a session.
pub struct SessionStore {
    pub index: SessionIndex,
    pub dir: PathBuf,
}

impl SessionStore {
    pub fn open(data_dir: &Path) -> Result<SessionStore> {
        let dir = data_dir.join("sessions");
        std::fs::create_dir_all(&dir)
            .map_err(|source| crate::error::LunaError::Io { path: dir.clone(), source })?;
        let index = SessionIndex::open(&data_dir.join("luna.db"))?;
        Ok(SessionStore { index, dir })
    }

    /// Path for a new session file.
    pub fn new_path(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.jsonl"))
    }

    pub fn create(&self, header: &SessionHeader) -> Result<Transcript> {
        let path = self.new_path(&header.id);
        Transcript::create_indexed(&path, header, Some(self.index.clone()))
    }

    pub fn open_transcript(&self, path: &Path) -> Result<Transcript> {
        Transcript::open_indexed(path, Some(self.index.clone()))
    }

    pub fn list(&self) -> Result<Vec<SessionSummary>> {
        self.index.list()
    }

    pub fn search(&self, query: &str) -> Result<Vec<SessionSummary>> {
        self.index.search(query)
    }

    pub fn delete(&self, path: &Path) -> Result<()> {
        std::fs::remove_file(path).ok();
        self.index.remove(&path.display().to_string())
    }

    /// Copy a session into a new one, keeping the history but not the identity.
    pub fn fork(&self, path: &Path, new_id: &str) -> Result<Transcript> {
        let source = Transcript::open(path)?;
        let mut header = source.header().clone();
        header.id = new_id.to_string();
        header.parent = Some(path.display().to_string());
        header.created = Utc::now().to_rfc3339();
        let target = self.new_path(new_id);
        let mut transcript =
            Transcript::create_indexed(&target, &header, Some(self.index.clone()))?;
        for message in source.messages() {
            transcript.append_message(message)?;
        }
        Ok(transcript)
    }
}

/// Format a timestamp for display, tolerating a malformed one.
pub fn parse_time(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value).ok().map(|time| time.with_timezone(&Utc))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("luna-session-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_session_round_trips_through_the_store() {
        let dir = temp_dir("roundtrip");
        let store = SessionStore::open(&dir).unwrap();
        let header = SessionHeader::new("s1", "/work", "anthropic/claude-sonnet-4-5");
        let mut transcript = store.create(&header).unwrap();
        transcript.append_message(&Message::user("hello")).unwrap();
        transcript
            .append_message(&Message::assistant(vec![crate::types::ContentBlock::text("hi")]))
            .unwrap();

        let reopened = store.open_transcript(&store.new_path("s1")).unwrap();
        assert_eq!(reopened.messages().len(), 2);
        assert_eq!(reopened.messages()[0].text(), "hello");

        let listed = store.list().unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].first_message, "hello");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn forking_copies_history_and_records_the_parent() {
        let dir = temp_dir("fork");
        let store = SessionStore::open(&dir).unwrap();
        let header = SessionHeader::new("s1", "/work", "m");
        let mut transcript = store.create(&header).unwrap();
        transcript.append_message(&Message::user("original")).unwrap();

        let forked = store.fork(&store.new_path("s1"), "s2").unwrap();
        assert_eq!(forked.messages().len(), 1);
        assert_eq!(
            forked.header().parent.as_deref(),
            Some(store.new_path("s1").display().to_string().as_str())
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn deleting_removes_the_file_and_the_index_row() {
        let dir = temp_dir("delete");
        let store = SessionStore::open(&dir).unwrap();
        let header = SessionHeader::new("s1", "/work", "m");
        store.create(&header).unwrap();
        store.delete(&store.new_path("s1")).unwrap();
        assert!(store.list().unwrap().is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }
}
