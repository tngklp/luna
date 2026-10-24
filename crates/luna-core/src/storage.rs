//! SQLite persistence: sessions, events, and runs.

use crate::error::{Error, Result};
use crate::event::{EventKind, LunaEvent};
use rusqlite::Connection;
use std::path::Path;

/// A stored session summary for the session list.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SessionSummary {
    pub id: String,
    pub started_at: String,
    pub last_activity: String,
    pub event_count: i64,
    pub first_user_message: Option<String>,
}

/// Write-through store for the event bus.
pub struct Store {
    conn: std::sync::Mutex<Connection>,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        let store = Self {
            conn: std::sync::Mutex::new(conn),
        };
        store.migrate()?;
        Ok(store)
    }

    /// In-memory store for tests.
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let store = Self {
            conn: std::sync::Mutex::new(conn),
        };
        store.migrate()?;
        Ok(store)
    }

    fn migrate(&self) -> Result<()> {
        self.conn.lock().unwrap().execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY,
                started_at TEXT NOT NULL,
                last_activity TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL REFERENCES sessions(id),
                ts TEXT NOT NULL,
                agent_id TEXT,
                kind TEXT NOT NULL,
                payload TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_events_session ON events(session_id, id);
            CREATE INDEX IF NOT EXISTS idx_sessions_activity ON sessions(last_activity);
            "#,
        )?;
        Ok(())
    }

    /// Ensure the session row exists.
    pub fn ensure_session(&self, session_id: &str) -> Result<()> {
        let now = chrono::Utc::now().to_rfc3339();
        self.conn
            .lock()
            .unwrap()
            .execute(
                "INSERT INTO sessions (id, started_at, last_activity) VALUES (?1, ?2, ?2)
                 ON CONFLICT(id) DO UPDATE SET last_activity = ?2",
                rusqlite::params![session_id, now],
            )
            .map_err(Error::Sqlite)?;
        Ok(())
    }

    /// Persist one event. Non-JSON-serializable payloads degrade to strings.
    pub fn record(&self, ev: &LunaEvent) -> Result<()> {
        self.ensure_session(&ev.session_id.to_string())?;
        let (kind, payload) = serialize_kind(&ev.kind);
        self.conn
            .lock()
            .unwrap()
            .execute(
                "INSERT INTO events (session_id, ts, agent_id, kind, payload) VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![
                    ev.session_id.to_string(),
                    ev.timestamp.to_rfc3339(),
                    ev.agent_id,
                    kind,
                    payload,
                ],
            )
            .map_err(Error::Sqlite)?;
        Ok(())
    }

    /// List sessions, most recently active first.
    pub fn list_sessions(&self, limit: u32) -> Result<Vec<SessionSummary>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            r#"
            SELECT s.id, s.started_at, s.last_activity, COUNT(e.id) as n,
                (SELECT payload FROM events e2
                 WHERE e2.session_id = s.id AND e2.kind = 'user_message'
                 ORDER BY e2.id LIMIT 1) as first_msg
            FROM sessions s LEFT JOIN events e ON e.session_id = s.id
            GROUP BY s.id
            ORDER BY s.last_activity DESC
            LIMIT ?1
            "#,
        )?;
        let rows = stmt
            .query_map(rusqlite::params![limit], |row| {
                Ok(SessionSummary {
                    id: row.get(0)?,
                    started_at: row.get(1)?,
                    last_activity: row.get(2)?,
                    event_count: row.get(3)?,
                    first_user_message: row.get::<_, Option<String>>(4)?.map(|p| {
                        serde_json::from_str::<serde_json::Value>(&p)
                            .ok()
                            .and_then(|v| v["text"].as_str().map(str::to_string))
                            .unwrap_or_else(|| p.chars().take(80).collect())
                    }),
                })
            })
            .map_err(Error::Sqlite)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(Error::Sqlite)?);
        }
        Ok(out)
    }

    /// Replay all events of a session in order.
    pub fn load_events(&self, session_id: &str) -> Result<Vec<LunaEvent>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT ts, agent_id, kind, payload FROM events WHERE session_id = ?1 ORDER BY id",
        )?;
        let rows = stmt
            .query_map(rusqlite::params![session_id], |row| {
                let ts: String = row.get(0)?;
                let agent_id: Option<String> = row.get(1)?;
                let kind: String = row.get(2)?;
                let payload: String = row.get(3)?;
                Ok((ts, agent_id, kind, payload))
            })
            .map_err(Error::Sqlite)?;

        let mut out = Vec::new();
        for row in rows {
            let (ts, agent_id, kind, payload) = row.map_err(Error::Sqlite)?;
            let timestamp = chrono::DateTime::parse_from_rfc3339(&ts)
                .map(|t| t.with_timezone(&chrono::Utc))
                .unwrap_or_else(|_| chrono::Utc::now());
            let session_id =
                uuid::Uuid::parse_str(session_id).unwrap_or_else(|_| uuid::Uuid::new_v4());
            let event_kind = deserialize_kind(&kind, &payload);
            out.push(LunaEvent {
                session_id,
                agent_id,
                timestamp,
                kind: event_kind,
            });
        }
        Ok(out)
    }
}

fn serialize_kind(kind: &EventKind) -> (String, String) {
    match kind {
        EventKind::RunStarted => ("run_started".into(), "{}".into()),
        EventKind::UserMessage(t) => ("user_message".into(), serde_json::json!({ "text": t }).to_string()),
        EventKind::AssistantMessage(t) => ("assistant_message".into(), serde_json::json!({ "text": t }).to_string()),
        EventKind::TokenDelta(t) => ("token_delta".into(), serde_json::json!({ "text": t }).to_string()),
        EventKind::ToolCallRequested { call_id, name, arguments } => (
            "tool_call_requested".into(),
            serde_json::json!({ "call_id": call_id, "name": name, "arguments": arguments }).to_string(),
        ),
        EventKind::ToolCallFinished { call_id, name, outcome } => (
            "tool_call_finished".into(),
            serde_json::json!({ "call_id": call_id, "name": name, "content": outcome.content, "is_error": outcome.is_error })
                .to_string(),
        ),
        EventKind::ApprovalRequested { request_id, agent_id, tool_name, arguments } => (
            "approval_requested".into(),
            serde_json::json!({ "request_id": request_id, "agent_id": agent_id, "tool_name": tool_name, "arguments": arguments })
                .to_string(),
        ),
        EventKind::ApprovalResolved { request_id, approved } => (
            "approval_resolved".into(),
            serde_json::json!({ "request_id": request_id, "approved": approved }).to_string(),
        ),
        EventKind::AgentSpawned { agent_id, parent, spec_name } => (
            "agent_spawned".into(),
            serde_json::json!({ "agent_id": agent_id, "parent": parent, "spec_name": spec_name }).to_string(),
        ),
        EventKind::OrchestratorTransition(d) => (
            "orchestrator_transition".into(),
            serde_json::json!({ "description": d }).to_string(),
        ),
        EventKind::RunFinished { summary, error } => (
            "run_finished".into(),
            serde_json::json!({ "summary": summary, "error": error }).to_string(),
        ),
    }
}

fn deserialize_kind(kind: &str, payload: &str) -> EventKind {
    let v: serde_json::Value = serde_json::from_str(payload).unwrap_or(serde_json::Value::Null);
    match kind {
        "run_started" => EventKind::RunStarted,
        "user_message" => {
            EventKind::UserMessage(v["text"].as_str().unwrap_or_default().to_string())
        }
        "assistant_message" => {
            EventKind::AssistantMessage(v["text"].as_str().unwrap_or_default().to_string())
        }
        "token_delta" => EventKind::TokenDelta(v["text"].as_str().unwrap_or_default().to_string()),
        "tool_call_requested" => EventKind::ToolCallRequested {
            call_id: v["call_id"].as_str().unwrap_or_default().to_string(),
            name: v["name"].as_str().unwrap_or_default().to_string(),
            arguments: v["arguments"].clone(),
        },
        "tool_call_finished" => EventKind::ToolCallFinished {
            call_id: v["call_id"].as_str().unwrap_or_default().to_string(),
            name: v["name"].as_str().unwrap_or_default().to_string(),
            outcome: crate::tool::registry::ToolOutcome {
                content: v["content"].as_str().unwrap_or_default().to_string(),
                is_error: v["is_error"].as_bool().unwrap_or(false),
            },
        },
        "approval_requested" => EventKind::ApprovalRequested {
            request_id: v["request_id"]
                .as_str()
                .and_then(|s| uuid::Uuid::parse_str(s).ok())
                .unwrap_or_default(),
            agent_id: v["agent_id"].as_str().unwrap_or_default().to_string(),
            tool_name: v["tool_name"].as_str().unwrap_or_default().to_string(),
            arguments: v["arguments"].clone(),
        },
        "approval_resolved" => EventKind::ApprovalResolved {
            request_id: v["request_id"]
                .as_str()
                .and_then(|s| uuid::Uuid::parse_str(s).ok())
                .unwrap_or_default(),
            approved: v["approved"].as_bool().unwrap_or(false),
        },
        "agent_spawned" => EventKind::AgentSpawned {
            agent_id: v["agent_id"].as_str().unwrap_or_default().to_string(),
            parent: v["parent"].as_str().map(str::to_string),
            spec_name: v["spec_name"].as_str().unwrap_or_default().to_string(),
        },
        "orchestrator_transition" => EventKind::OrchestratorTransition(
            v["description"].as_str().unwrap_or_default().to_string(),
        ),
        "run_finished" => EventKind::RunFinished {
            summary: v["summary"].as_str().map(str::to_string),
            error: v["error"].as_str().map(str::to_string),
        },
        _ => EventKind::OrchestratorTransition(format!("(unknown event kind `{kind}`)")),
    }
}

/// Spawn a background task that records every bus event into the store.
///
/// Must be called from within a tokio runtime context (the TUI's run loop
/// and the GUI's async commands both provide one).
pub fn attach_store(
    bus: &crate::event::EventBus,
    store: std::sync::Arc<Store>,
) -> tokio::task::JoinHandle<()> {
    let mut rx = bus.subscribe();
    tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(ev) => {
                    if let Err(e) = store.record(&ev) {
                        tracing::warn!(error = %e, "failed to persist event");
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!(skipped = n, "store receiver lagged");
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::EventBus;

    #[test]
    fn roundtrip_events() {
        let store = Store::open_in_memory().unwrap();
        let sid = uuid::Uuid::new_v4();
        store.ensure_session(&sid.to_string()).unwrap();
        store
            .record(&LunaEvent::new(
                sid,
                None,
                EventKind::UserMessage("hello".into()),
            ))
            .unwrap();
        store
            .record(&LunaEvent::new(
                sid,
                Some("agent1".into()),
                EventKind::ToolCallRequested {
                    call_id: "c1".into(),
                    name: "read_file".into(),
                    arguments: serde_json::json!({"path": "a.txt"}),
                },
            ))
            .unwrap();
        store
            .record(&LunaEvent::new(
                sid,
                None,
                EventKind::RunFinished {
                    summary: Some("done".into()),
                    error: None,
                },
            ))
            .unwrap();

        let events = store.load_events(&sid.to_string()).unwrap();
        assert_eq!(events.len(), 3);
        assert!(matches!(events[0].kind, EventKind::UserMessage(ref t) if t == "hello"));
        match &events[1].kind {
            EventKind::ToolCallRequested {
                name, arguments, ..
            } => {
                assert_eq!(name, "read_file");
                assert_eq!(arguments["path"], "a.txt");
            }
            other => panic!("unexpected: {other:?}"),
        }
        assert!(matches!(
            events[2].kind,
            EventKind::RunFinished {
                summary: Some(_),
                error: None
            }
        ));
    }

    #[test]
    fn session_list_orders_by_activity() {
        let store = Store::open_in_memory().unwrap();
        let s1 = uuid::Uuid::new_v4();
        let s2 = uuid::Uuid::new_v4();
        store
            .record(&LunaEvent::new(
                s1,
                None,
                EventKind::UserMessage("first".into()),
            ))
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
        store
            .record(&LunaEvent::new(
                s2,
                None,
                EventKind::UserMessage("second".into()),
            ))
            .unwrap();

        let sessions = store.list_sessions(10).unwrap();
        assert_eq!(sessions.len(), 2);
        assert_eq!(sessions[0].id, s2.to_string());
        assert_eq!(sessions[0].first_user_message.as_deref(), Some("second"));
    }

    #[tokio::test]
    async fn attach_store_records_from_bus() {
        let bus = EventBus::default();
        let store = std::sync::Arc::new(Store::open_in_memory().unwrap());
        let handle = attach_store(&bus, store.clone());
        let sid = uuid::Uuid::new_v4();
        bus.publish(LunaEvent::new(
            sid,
            None,
            EventKind::UserMessage("via bus".into()),
        ));
        // Give the forwarder a moment.
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        handle.abort();
        let events = store.load_events(&sid.to_string()).unwrap();
        assert_eq!(events.len(), 1);
    }
}
