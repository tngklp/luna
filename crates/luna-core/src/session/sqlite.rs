//! The SQLite index.
//!
//! Holds only what needs querying: session metadata, a full-text index of
//! messages, and the pending-change log. Transcripts stay in JSONL.

use std::path::Path;
use std::sync::{Arc, Mutex};

use rusqlite::{params, Connection};

use crate::error::{LunaError, Result};

use super::{PendingChange, SessionSummary};

const SCHEMA: &str = "
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;

CREATE TABLE IF NOT EXISTS sessions (
    path            TEXT PRIMARY KEY,
    id              TEXT NOT NULL,
    cwd             TEXT NOT NULL,
    name            TEXT,
    created         TEXT NOT NULL,
    modified        TEXT NOT NULL,
    message_count   INTEGER NOT NULL DEFAULT 0,
    first_message   TEXT NOT NULL DEFAULT '',
    parent_path     TEXT
);

CREATE INDEX IF NOT EXISTS sessions_modified ON sessions (modified DESC);

CREATE VIRTUAL TABLE IF NOT EXISTS messages_fts USING fts5 (
    session_path UNINDEXED,
    body,
    tokenize = 'unicode61'
);

CREATE TABLE IF NOT EXISTS changes (
    path        TEXT PRIMARY KEY,
    added       INTEGER NOT NULL DEFAULT 0,
    removed     INTEGER NOT NULL DEFAULT 0,
    diff        TEXT NOT NULL DEFAULT '',
    at          TEXT NOT NULL
);
";

/// A handle to the index. Cheap to clone; the connection is shared.
#[derive(Clone)]
pub struct SessionIndex {
    connection: Arc<Mutex<Connection>>,
}

impl SessionIndex {
    pub fn open(path: &Path) -> Result<SessionIndex> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|source| LunaError::Io { path: parent.to_path_buf(), source })?;
        }
        let connection = Connection::open(path)?;
        connection.execute_batch(SCHEMA)?;
        Ok(SessionIndex { connection: Arc::new(Mutex::new(connection)) })
    }

    /// An in-memory index, for tests.
    pub fn in_memory() -> Result<SessionIndex> {
        let connection = Connection::open_in_memory()?;
        connection.execute_batch(SCHEMA)?;
        Ok(SessionIndex { connection: Arc::new(Mutex::new(connection)) })
    }

    fn with<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        let guard =
            self.connection.lock().map_err(|_| LunaError::Store("index lock poisoned".into()))?;
        f(&guard)
    }

    pub fn upsert(&self, summary: &SessionSummary) -> Result<()> {
        self.with(|connection| {
            connection.execute(
                "INSERT INTO sessions (path, id, cwd, name, created, modified, message_count, first_message, parent_path)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                 ON CONFLICT(path) DO UPDATE SET
                    name = excluded.name,
                    modified = excluded.modified,
                    message_count = excluded.message_count,
                    first_message = excluded.first_message",
                params![
                    summary.path,
                    summary.id,
                    summary.cwd,
                    summary.name,
                    summary.created,
                    summary.modified,
                    summary.message_count as i64,
                    summary.first_message,
                    summary.parent_session_path,
                ],
            )?;
            Ok(())
        })
    }

    pub fn remove(&self, path: &str) -> Result<()> {
        self.with(|connection| {
            connection.execute("DELETE FROM sessions WHERE path = ?1", params![path])?;
            connection
                .execute("DELETE FROM messages_fts WHERE session_path = ?1", params![path])?;
            Ok(())
        })
    }

    pub fn list(&self) -> Result<Vec<SessionSummary>> {
        self.with(|connection| {
            let mut statement = connection.prepare(
                "SELECT path, id, cwd, name, created, modified, message_count, first_message, parent_path
                 FROM sessions ORDER BY modified DESC",
            )?;
            let rows = statement.query_map([], row_to_summary)?;
            Ok(rows.flatten().collect())
        })
    }

    /// Full-text search across every indexed message.
    pub fn search(&self, query: &str) -> Result<Vec<SessionSummary>> {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return self.list();
        }
        // FTS5 treats punctuation as syntax; quoting the whole query makes a
        // search for `foo::bar` behave the way a user expects.
        let escaped = format!("\"{}\"", trimmed.replace('"', " "));
        self.with(|connection| {
            let mut statement = connection.prepare(
                "SELECT s.path, s.id, s.cwd, s.name, s.created, s.modified, s.message_count,
                        s.first_message, s.parent_path
                 FROM sessions s
                 WHERE s.path IN (SELECT session_path FROM messages_fts WHERE messages_fts MATCH ?1)
                    OR s.first_message LIKE ?2
                 ORDER BY s.modified DESC",
            )?;
            let like = format!("%{trimmed}%");
            let rows = statement.query_map(params![escaped, like], row_to_summary)?;
            Ok(rows.flatten().collect())
        })
    }

    /// Index one message body for search.
    pub fn index_message(&self, session_path: &str, body: &str) -> Result<()> {
        if body.trim().is_empty() {
            return Ok(());
        }
        self.with(|connection| {
            connection.execute(
                "INSERT INTO messages_fts (session_path, body) VALUES (?1, ?2)",
                params![session_path, body],
            )?;
            Ok(())
        })
    }

    pub fn record_change(&self, change: &PendingChange) -> Result<()> {
        self.with(|connection| {
            connection.execute(
                "INSERT INTO changes (path, added, removed, diff, at) VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(path) DO UPDATE SET
                    added = excluded.added,
                    removed = excluded.removed,
                    diff = excluded.diff,
                    at = excluded.at",
                params![
                    change.path,
                    change.added as i64,
                    change.removed as i64,
                    change.diff,
                    change.at
                ],
            )?;
            Ok(())
        })
    }

    pub fn changes(&self) -> Result<Vec<PendingChange>> {
        self.with(|connection| {
            let mut statement = connection
                .prepare("SELECT path, added, removed, diff, at FROM changes ORDER BY at DESC")?;
            let rows = statement.query_map([], |row| {
                Ok(PendingChange {
                    path: row.get(0)?,
                    added: row.get::<_, i64>(1)? as usize,
                    removed: row.get::<_, i64>(2)? as usize,
                    diff: row.get(3)?,
                    at: row.get(4)?,
                })
            })?;
            Ok(rows.flatten().collect())
        })
    }

    pub fn clear_change(&self, path: &str) -> Result<()> {
        self.with(|connection| {
            connection.execute("DELETE FROM changes WHERE path = ?1", params![path])?;
            Ok(())
        })
    }

    pub fn clear_changes(&self) -> Result<()> {
        self.with(|connection| {
            connection.execute("DELETE FROM changes", [])?;
            Ok(())
        })
    }
}

fn row_to_summary(row: &rusqlite::Row<'_>) -> rusqlite::Result<SessionSummary> {
    Ok(SessionSummary {
        path: row.get(0)?,
        id: row.get(1)?,
        cwd: row.get(2)?,
        name: row.get(3)?,
        created: row.get(4)?,
        modified: row.get(5)?,
        message_count: row.get::<_, i64>(6)? as usize,
        first_message: row.get(7)?,
        parent_session_path: row.get(8)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary(path: &str, first: &str) -> SessionSummary {
        SessionSummary {
            path: path.to_string(),
            id: path.to_string(),
            cwd: "/work".to_string(),
            name: None,
            created: "2026-01-01T00:00:00Z".to_string(),
            modified: "2026-01-02T00:00:00Z".to_string(),
            message_count: 1,
            first_message: first.to_string(),
            parent_session_path: None,
        }
    }

    #[test]
    fn upsert_and_list() {
        let index = SessionIndex::in_memory().unwrap();
        index.upsert(&summary("a", "hello")).unwrap();
        index.upsert(&summary("b", "world")).unwrap();
        assert_eq!(index.list().unwrap().len(), 2);
    }

    #[test]
    fn upsert_updates_instead_of_duplicating() {
        let index = SessionIndex::in_memory().unwrap();
        index.upsert(&summary("a", "hello")).unwrap();
        let mut updated = summary("a", "hello");
        updated.message_count = 9;
        index.upsert(&updated).unwrap();
        let listed = index.list().unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].message_count, 9);
    }

    #[test]
    fn search_finds_indexed_messages() {
        let index = SessionIndex::in_memory().unwrap();
        index.upsert(&summary("a", "hello")).unwrap();
        index.index_message("a", "the quick brown fox").unwrap();
        let found = index.search("brown").unwrap();
        assert_eq!(found.len(), 1);
    }

    #[test]
    fn search_falls_back_to_the_first_message() {
        let index = SessionIndex::in_memory().unwrap();
        index.upsert(&summary("a", "refactor the parser")).unwrap();
        assert_eq!(index.search("parser").unwrap().len(), 1);
    }

    #[test]
    fn changes_round_trip() {
        let index = SessionIndex::in_memory().unwrap();
        index
            .record_change(&PendingChange {
                path: "src/a.rs".into(),
                added: 3,
                removed: 1,
                diff: "@@ -1 +1 @@".into(),
                at: "2026-01-01T00:00:00Z".into(),
            })
            .unwrap();
        assert_eq!(index.changes().unwrap().len(), 1);
        index.clear_changes().unwrap();
        assert!(index.changes().unwrap().is_empty());
    }

    #[test]
    fn removing_a_session_clears_its_search_rows() {
        let index = SessionIndex::in_memory().unwrap();
        index.upsert(&summary("a", "hello")).unwrap();
        index.index_message("a", "unique phrase").unwrap();
        index.remove("a").unwrap();
        assert!(index.search("unique").unwrap().is_empty());
    }
}
