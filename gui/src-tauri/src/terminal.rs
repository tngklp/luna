//! The integrated terminal.
//!
//! A shell runs as a child process; its output is forwarded to the webview as
//! events and its input comes back through a command. This is deliberately not a
//! PTY: a plain pipe is enough for the commands a coding agent's user runs, and
//! it avoids a native dependency on every platform.

use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Child;

pub const TERMINAL_EVENT: &str = "luna://terminal";

/// One running shell.
struct Session {
    /// Held so the id returned to the UI stays meaningful.
    _id: u64,
    child: Child,
    /// Input goes through a channel so the command never holds a lock across an
    /// await, which would make its future non-`Send`.
    input: tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
}

/// Owns the terminal sessions for one window.
pub struct TerminalState {
    next_id: AtomicU64,
    session: Mutex<Option<Session>>,
}

impl Default for TerminalState {
    fn default() -> Self {
        TerminalState { next_id: AtomicU64::new(1), session: Mutex::new(None) }
    }
}

impl TerminalState {
    pub fn new() -> Arc<TerminalState> {
        Arc::new(TerminalState::default())
    }

    /// Start a shell, replacing any running one.
    pub fn start(&self, app: AppHandle, cwd: &std::path::Path) -> Result<u64, String> {
        self.stop();

        let (program, args): (&str, Vec<&str>) = if cfg!(windows) {
            ("cmd", vec!["/Q"])
        } else {
            ("sh", vec![])
        };

        let mut child = tokio::process::Command::new(program)
            .args(args)
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|err| format!("could not start {program}: {err}"))?;

        let mut stdin = child.stdin.take().ok_or("the shell has no stdin")?;
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);

        // A dedicated task owns stdin, so writes never block the caller.
        let (input, mut receiver) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
        tokio::spawn(async move {
            while let Some(bytes) = receiver.recv().await {
                if stdin.write_all(&bytes).await.is_err() {
                    break;
                }
                let _ = stdin.flush().await;
            }
        });

        // Both pipes feed the same event, so the transcript reads in real order.
        if let Some(stdout) = child.stdout.take() {
            let app = app.clone();
            tokio::spawn(async move {
                let mut lines = BufReader::new(stdout).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    let _ = app.emit(TERMINAL_EVENT, format!("{line}\r\n"));
                }
            });
        }
        if let Some(stderr) = child.stderr.take() {
            let app = app.clone();
            tokio::spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    let _ = app.emit(TERMINAL_EVENT, format!("{line}\r\n"));
                }
            });
        }

        if let Ok(mut slot) = self.session.lock() {
            *slot = Some(Session { _id: id, child, input });
        }
        Ok(id)
    }

    /// Write to the shell.
    pub fn write(&self, data: &str) -> Result<(), String> {
        let guard = self.session.lock().map_err(|_| "the terminal is unavailable")?;
        let session = guard.as_ref().ok_or("no terminal is running")?;
        session
            .input
            .send(data.as_bytes().to_vec())
            .map_err(|_| "the terminal has closed".to_string())
    }

    /// Kill the shell.
    pub fn stop(&self) {
        if let Ok(mut slot) = self.session.lock() {
            if let Some(mut session) = slot.take() {
                let _ = session.child.start_kill();
            }
        }
    }

    pub fn is_running(&self) -> bool {
        self.session.lock().map(|slot| slot.is_some()).unwrap_or(false)
    }
}

/// Whether a terminal is running, for the UI to decide what to show.
#[tauri::command]
pub fn terminal_running(
    terminal: tauri::State<'_, std::sync::Arc<crate::terminal::TerminalState>>,
) -> bool {
    terminal.is_running()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_state_has_no_session() {
        let state = TerminalState::default();
        assert!(!state.is_running());
    }

    #[test]
    fn stopping_an_idle_terminal_is_harmless() {
        let state = TerminalState::default();
        state.stop();
        assert!(!state.is_running());
    }
}
