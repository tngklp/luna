//! Application state and the event loop.
//!
//! The loop is deliberately simple: poll input, drain agent events, redraw when
//! something changed. Agent events arrive on a channel and are coalesced, so a
//! fast token stream costs one redraw per frame rather than one per token.
//!
//! The session lives behind a mutex rather than being moved into the worker, so
//! the UI can always read its stats and the next turn can always find it.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{
    self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEventKind,
};
use luna_core::agent::{build_system, AgentDeps, AgentSession, UiAnswer, UiBridge, UiRequest};
use luna_core::config::Config;
use luna_core::event::{AgentEvent, AgentRef, Emitted, EventSink, SinkRef};
use luna_core::orchestrator::Orchestrator;
use luna_core::provider::ProviderRegistry;
use luna_core::session::{SessionHeader, SessionStore, Transcript};
use luna_core::types::{Message, Role, ThinkingLevel};
use ratatui::backend::Backend;
use ratatui::Terminal;
use tokio::runtime::Handle;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

use crate::theme::Theme;

/// A line in the transcript, already flattened for rendering.
#[derive(Debug, Clone)]
pub enum Line {
    User(String),
    Assistant(String),
    Thinking(String),
    Tool { name: String, detail: String, running: bool, is_error: bool },
    ToolOutput(String),
    Notice { level: String, text: String },
    Error(String),
}

/// What the composer is doing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    Normal,
    /// A modal prompt is open; the composer is disabled.
    Approval,
}

/// A pending approval, answered by the next keypress.
pub struct PendingApproval {
    pub tool: String,
    pub title: String,
    pub detail: String,
    pub reply: tokio::sync::oneshot::Sender<UiAnswer>,
}

/// Bridges the core's approval requests onto the TUI.
struct TuiUi {
    sender: UnboundedSender<PendingApproval>,
}

#[async_trait::async_trait]
impl UiBridge for TuiUi {
    async fn ask(&self, request: UiRequest) -> UiAnswer {
        let (reply, answer) = tokio::sync::oneshot::channel();
        let pending = match request {
            UiRequest::Approval { tool, title, detail } => {
                PendingApproval { tool, title, detail, reply }
            }
        };
        if self.sender.send(pending).is_err() {
            return UiAnswer::Deny;
        }
        answer.await.unwrap_or(UiAnswer::Deny)
    }
}

/// Forwards core events into the UI channel.
struct ChannelSink {
    sender: UnboundedSender<Emitted>,
}

impl EventSink for ChannelSink {
    fn emit(&self, event: Emitted) {
        // A closed channel means the UI is gone; dropping is correct.
        let _ = self.sender.send(event);
    }
}

/// The live session, shared between the UI thread and the worker.
type SharedSession = Arc<Mutex<Option<AgentSession>>>;

pub struct App {
    pub config: Config,
    pub theme: Theme,
    pub lines: Vec<Line>,
    pub input: String,
    pub cursor: usize,
    pub scroll: usize,
    pub follow: bool,
    pub mode: Mode,
    pub pending: Option<PendingApproval>,
    pub status: String,
    pub model: String,
    pub thinking: ThinkingLevel,
    pub running: bool,
    pub agents: Vec<(String, String, String)>,
    pub sessions: Vec<String>,
    pub show_sessions: bool,
    pub should_quit: bool,
    pub tokens_in: u32,
    pub tokens_out: u32,
    pub cost: f64,
    pub context_percent: f32,

    providers: Arc<ProviderRegistry>,
    store: SessionStore,
    transcript: Option<Transcript>,
    session: SharedSession,
    orchestrator: Option<Arc<Orchestrator>>,
    events: UnboundedReceiver<Emitted>,
    approvals: UnboundedReceiver<PendingApproval>,
    event_tx: UnboundedSender<Emitted>,
    approval_tx: UnboundedSender<PendingApproval>,
    sink: SinkRef,
    handle: Handle,
    dirty: bool,
    last_frame: Instant,
}

impl App {
    pub fn new(
        config: Config,
        providers: Arc<ProviderRegistry>,
        store: SessionStore,
        _sink: SinkRef,
        handle: Handle,
    ) -> Result<App> {
        let (event_tx, events) = unbounded_channel();
        let (approval_tx, approvals) = unbounded_channel();
        let sink: SinkRef = Arc::new(ChannelSink { sender: event_tx.clone() });

        let model = config.general.default_model.clone();
        let sessions = store
            .list()
            .map(|list| list.into_iter().map(|summary| summary.path).collect())
            .unwrap_or_default();

        let mut app = App {
            theme: Theme::dark(),
            lines: Vec::new(),
            input: String::new(),
            cursor: 0,
            scroll: 0,
            follow: true,
            mode: Mode::Normal,
            pending: None,
            status: "ready".to_string(),
            model,
            thinking: ThinkingLevel::Medium,
            running: false,
            agents: Vec::new(),
            sessions,
            show_sessions: false,
            should_quit: false,
            tokens_in: 0,
            tokens_out: 0,
            cost: 0.0,
            context_percent: 0.0,
            config,
            providers,
            store,
            transcript: None,
            session: Arc::new(Mutex::new(None)),
            orchestrator: None,
            events,
            approvals,
            event_tx,
            approval_tx,
            sink,
            handle,
            dirty: true,
            last_frame: Instant::now(),
        };

        app.lines.push(Line::Notice {
            level: "info".into(),
            text: "Luna — type a message, /help for commands, Ctrl+C to quit".into(),
        });
        app.start_session()?;
        Ok(app)
    }

    /// Create a fresh session and agent.
    fn start_session(&mut self) -> Result<()> {
        let id = uuid::Uuid::new_v4().to_string();
        let cwd = std::env::current_dir().unwrap_or_else(|_| ".".into());
        let header = SessionHeader::new(&id, cwd.display().to_string(), &self.model);
        let transcript = self.store.create(&header)?;
        let system = build_system(&self.config, &cwd, None);

        let deps = Arc::new(
            AgentDeps::new(self.providers.clone(), self.sink.clone(), &cwd)
                .with_ui(Arc::new(TuiUi { sender: self.approval_tx.clone() })),
        );
        let orchestrator =
            Arc::new(Orchestrator::new(deps.clone(), self.config.clone(), AgentRef::root(&id)));
        let mut session = AgentSession::new(deps, AgentRef::root(&id), &self.model, system);
        session.thinking = self.thinking;

        self.transcript = Some(transcript);
        *self.session.lock().unwrap() = Some(session);
        self.orchestrator = Some(orchestrator);
        Ok(())
    }

    /// Run until the user quits.
    pub fn run<B: Backend>(
        &mut self,
        terminal: &mut Terminal<B>,
        runtime: &tokio::runtime::Runtime,
    ) -> Result<()> {
        let _guard = runtime.enter();
        loop {
            self.drain_events();
            self.drain_approvals();
            self.refresh_stats();

            if self.dirty || self.last_frame.elapsed() > Duration::from_millis(33) {
                terminal.draw(|frame| crate::ui::draw(frame, self))?;
                self.dirty = false;
                self.last_frame = Instant::now();
            }

            if event::poll(Duration::from_millis(16))? {
                match event::read()? {
                    Event::Key(key) if key.kind == KeyEventKind::Press => self.on_key(key),
                    Event::Mouse(mouse) => match mouse.kind {
                        MouseEventKind::ScrollUp => self.scroll_up(3),
                        MouseEventKind::ScrollDown => self.scroll_down(3),
                        _ => {}
                    },
                    Event::Paste(text) => {
                        self.input.insert_str(self.cursor, &text);
                        self.cursor += text.len();
                        self.dirty = true;
                    }
                    Event::Resize(_, _) => self.dirty = true,
                    _ => {}
                }
            }

            if self.should_quit {
                return Ok(());
            }
        }
    }

    fn on_key(&mut self, key: KeyEvent) {
        // An approval prompt owns the keyboard until it is answered.
        if self.mode == Mode::Approval {
            self.answer_approval(key);
            return;
        }

        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Char('c') if ctrl => self.should_quit = true,
            KeyCode::Char('d') if ctrl => self.should_quit = true,
            KeyCode::Char('n') if ctrl => self.new_session(),
            KeyCode::Char('l') if ctrl => {
                self.lines.clear();
                self.dirty = true;
            }
            KeyCode::Char('s') if ctrl => {
                self.show_sessions = !self.show_sessions;
                self.refresh_sessions();
                self.dirty = true;
            }
            KeyCode::Esc => {
                if self.running {
                    self.abort();
                } else {
                    self.show_sessions = false;
                    self.dirty = true;
                }
            }
            KeyCode::Enter => {
                if key.modifiers.contains(KeyModifiers::SHIFT) {
                    self.insert_char('\n');
                } else {
                    self.submit();
                }
            }
            KeyCode::Char(ch) => self.insert_char(ch),
            KeyCode::Backspace => self.backspace(),
            KeyCode::Delete => self.delete_forward(),
            KeyCode::Left => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::Right => self.cursor = (self.cursor + 1).min(self.input.len()),
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = self.input.len(),
            KeyCode::Up => {
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    self.scroll_up(1);
                } else {
                    self.scroll_up(3);
                }
            }
            KeyCode::Down => {
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    self.scroll_down(1);
                } else {
                    self.scroll_down(3);
                }
            }
            KeyCode::PageUp => self.scroll_up(15),
            KeyCode::PageDown => self.scroll_down(15),
            _ => {}
        }
        self.dirty = true;
    }

    fn insert_char(&mut self, ch: char) {
        self.input.insert(self.cursor, ch);
        self.cursor += ch.len_utf8();
    }

    fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        // Step back one whole character, not one byte.
        let mut index = self.cursor - 1;
        while index > 0 && !self.input.is_char_boundary(index) {
            index -= 1;
        }
        self.input.remove(index);
        self.cursor = index;
    }

    fn delete_forward(&mut self) {
        if self.cursor >= self.input.len() {
            return;
        }
        self.input.remove(self.cursor);
    }

    fn scroll_up(&mut self, amount: usize) {
        self.follow = false;
        self.scroll = self.scroll.saturating_add(amount);
    }

    fn scroll_down(&mut self, amount: usize) {
        self.scroll = self.scroll.saturating_sub(amount);
        if self.scroll == 0 {
            self.follow = true;
        }
    }

    fn answer_approval(&mut self, key: KeyEvent) {
        let Some(pending) = self.pending.take() else {
            self.mode = Mode::Normal;
            return;
        };
        let answer = match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => UiAnswer::Allow,
            KeyCode::Char('a') | KeyCode::Char('A') => UiAnswer::AllowAlways,
            _ => UiAnswer::Deny,
        };
        let _ = pending.reply.send(answer);
        self.mode = Mode::Normal;
        self.dirty = true;
    }

    fn abort(&mut self) {
        if let Some(session) = self.session.lock().unwrap().as_ref() {
            session.abort();
        }
        if let Some(orchestrator) = &self.orchestrator {
            orchestrator.cancel();
        }
        self.running = false;
        self.lines.push(Line::Notice { level: "warning".into(), text: "aborted".into() });
        self.dirty = true;
    }

    fn new_session(&mut self) {
        if let Err(err) = self.start_session() {
            self.lines.push(Line::Error(err.to_string()));
        } else {
            self.lines.clear();
            self.lines.push(Line::Notice { level: "info".into(), text: "new session".into() });
        }
        self.dirty = true;
    }

    /// Send the composer contents.
    fn submit(&mut self) {
        let text = self.input.trim().to_string();
        if text.is_empty() {
            return;
        }
        self.input.clear();
        self.cursor = 0;

        if text.starts_with('/') {
            self.run_command(&text);
            return;
        }

        // While a turn is running, a message steers it instead of starting one.
        if self.running {
            if let Some(session) = self.session.lock().unwrap().as_mut() {
                session.steer(text.clone());
            }
            self.lines.push(Line::User(text));
            self.dirty = true;
            return;
        }

        self.lines.push(Line::User(text.clone()));
        self.follow = true;
        self.scroll = 0;
        self.running = true;
        self.status = "thinking".into();
        self.dirty = true;

        let session = self.session.clone();
        let sender = self.event_tx.clone();
        self.handle.spawn(async move {
            // Take the session out for the duration of the turn so the UI thread
            // never blocks on it, then put it back.
            let taken = session.lock().unwrap().take();
            let Some(mut agent) = taken else { return };
            let outcome = agent.run_turn(text).await;
            if let Err(err) = outcome {
                let _ = sender.send(Emitted::notice("error", err.to_string()));
            }
            *session.lock().unwrap() = Some(agent);
            let _ = sender.send(Emitted::global(AgentEvent::AgentSettled {}));
        });
    }

    fn run_command(&mut self, text: &str) {
        let mut parts = text.splitn(2, ' ');
        let command = parts.next().unwrap_or_default();
        let argument = parts.next().unwrap_or_default().trim();

        match command {
            "/help" => {
                for line in [
                    "/help              show this",
                    "/new               start a new session",
                    "/model <name>      switch model, e.g. anthropic/claude-sonnet-4-5",
                    "/thinking <level>  off|minimal|low|medium|high|xhigh|max",
                    "/sessions          list saved sessions",
                    "/copy              copy the transcript to the clipboard",
                    "/quit              exit",
                ] {
                    self.lines.push(Line::Notice { level: "info".into(), text: line.into() });
                }
            }
            "/new" => self.new_session(),
            "/quit" | "/exit" => self.should_quit = true,
            "/model" => {
                if argument.is_empty() {
                    self.lines.push(Line::Notice {
                        level: "info".into(),
                        text: format!("current model: {}", self.model),
                    });
                } else {
                    self.model = argument.to_string();
                    if let Some(session) = self.session.lock().unwrap().as_mut() {
                        session.model = argument.to_string();
                    }
                    self.lines.push(Line::Notice {
                        level: "info".into(),
                        text: format!("model set to {argument}"),
                    });
                }
            }
            "/thinking" => {
                let level = ThinkingLevel::from_str_lossy(argument);
                self.thinking = level;
                if let Some(session) = self.session.lock().unwrap().as_mut() {
                    session.set_thinking(level);
                }
                self.lines.push(Line::Notice {
                    level: "info".into(),
                    text: format!("thinking level: {}", level.as_str()),
                });
            }
            "/sessions" => {
                self.show_sessions = true;
                self.refresh_sessions();
            }
            "/copy" => {
                let text = transcript_text(&self.lines);
                match copy_to_clipboard(&text) {
                    Ok(()) => self.lines.push(Line::Notice {
                        level: "info".into(),
                        text: format!("copied {} characters", text.chars().count()),
                    }),
                    Err(err) => self.lines.push(Line::Error(format!("could not copy: {err}"))),
                }
            }
            other => {
                self.lines.push(Line::Error(format!("unknown command `{other}`; try /help")));
            }
        }
        self.dirty = true;
    }

    fn refresh_sessions(&mut self) {
        self.sessions = self
            .store
            .list()
            .map(|list| list.into_iter().map(|summary| summary.path).collect())
            .unwrap_or_default();
    }

    /// Pull token and cost totals out of the live session.
    fn refresh_stats(&mut self) {
        let Ok(guard) = self.session.lock() else { return };
        let Some(session) = guard.as_ref() else { return };
        self.tokens_in = session.stats.usage.input;
        self.tokens_out = session.stats.usage.output;
        self.cost = session.stats.cost;
        let window = session.model_info().context_window.max(1);
        let used = luna_core::tokens::estimate_messages(&session.messages);
        self.context_percent = (used as f32 / window as f32) * 100.0;
    }

    /// Apply everything the agent produced since the last frame.
    fn drain_events(&mut self) {
        while let Ok(event) = self.events.try_recv() {
            self.apply(event);
            self.dirty = true;
        }
    }

    fn drain_approvals(&mut self) {
        while let Ok(pending) = self.approvals.try_recv() {
            self.lines.push(Line::Notice {
                level: "warning".into(),
                text: format!("{}: {}", pending.title, pending.detail),
            });
            self.pending = Some(pending);
            self.mode = Mode::Approval;
            self.dirty = true;
        }
    }

    fn apply(&mut self, emitted: Emitted) {
        match emitted.event {
            AgentEvent::MessageStart { .. } => {
                self.running = true;
                self.status = "streaming".into();
            }
            AgentEvent::MessageUpdate { assistant_message_event } => {
                let update = assistant_message_event;
                match update.kind.as_str() {
                    "text_delta" => {
                        if let Some(delta) = update.delta {
                            self.push_assistant(&delta);
                        }
                    }
                    "thinking_delta" => {
                        if let Some(delta) = update.delta {
                            self.push_thinking(&delta);
                        }
                    }
                    "toolcall_start" => {
                        if let Some(partial) = update.partial {
                            if let Some(luna_core::types::ContentBlock::ToolCall { name, .. }) =
                                partial.content.last()
                            {
                                self.lines.push(Line::Tool {
                                    name: name.clone(),
                                    detail: String::new(),
                                    running: true,
                                    is_error: false,
                                });
                            }
                        }
                    }
                    "toolcall_delta" => {
                        if let Some(delta) = update.delta {
                            if let Some(Line::Tool { detail, .. }) = self.lines.last_mut() {
                                detail.push_str(&delta);
                            }
                        }
                    }
                    "toolcall_end" => {
                        if let Some(call) = update.tool_call {
                            if let Some(Line::Tool { name, detail, .. }) = self.lines.last_mut() {
                                *name = call.name;
                                *detail = compact_arguments(&call.arguments);
                            }
                        }
                    }
                    _ => {}
                }
            }
            AgentEvent::ToolExecutionStart { tool_name, args, .. } => {
                self.lines.push(Line::Tool {
                    name: tool_name,
                    detail: compact_arguments(&args),
                    running: true,
                    is_error: false,
                });
            }
            AgentEvent::ToolExecutionUpdate { partial_result, .. } => {
                let text: String = partial_result
                    .content
                    .iter()
                    .filter_map(|block| block.as_text())
                    .collect::<Vec<_>>()
                    .join("");
                if !text.is_empty() {
                    self.lines.push(Line::ToolOutput(text));
                }
            }
            AgentEvent::ToolExecutionEnd { is_error, .. } => {
                if let Some(Line::Tool { running, is_error: flag, .. }) = self.lines.last_mut() {
                    *running = false;
                    *flag = is_error;
                }
            }
            AgentEvent::AgentSettled {} => {
                self.running = false;
                self.status = "ready".into();
                self.persist_turn();
            }
            AgentEvent::QueueUpdate { steering, follow_up } => {
                let queued = steering.len() + follow_up.len();
                if queued > 0 {
                    self.status = format!("{queued} queued");
                }
            }
            AgentEvent::ThinkingLevelChanged { level } => {
                self.thinking = ThinkingLevel::from_str_lossy(&level);
            }
            AgentEvent::SessionInfoChanged { .. } => {}
            AgentEvent::AgentSpawned { agent_id, role, title, .. } => {
                self.agents.push((agent_id, role, title));
            }
            AgentEvent::AgentFinished { agent_id, status, .. } => {
                self.agents.retain(|(id, _, _)| id != &agent_id);
                self.lines.push(Line::Notice {
                    level: if status == "done" { "info".into() } else { "warning".into() },
                    text: format!("subagent {agent_id} {status}"),
                });
            }
            AgentEvent::Notice { level, message } => {
                if level == "error" {
                    self.lines.push(Line::Error(message));
                } else {
                    self.lines.push(Line::Notice { level, text: message });
                }
            }
            AgentEvent::MessageEnd { .. } => {}
        }
    }

    /// Append anything new in the session to the transcript file.
    ///
    /// The transcript keeps its own message list, so only the tail is written.
    fn persist_turn(&mut self) {
        let Some(transcript) = &mut self.transcript else { return };
        let Ok(guard) = self.session.lock() else { return };
        let Some(session) = guard.as_ref() else { return };

        let already = transcript.messages().len();
        for message in session.messages.iter().skip(already) {
            if !is_persistable(message) {
                continue;
            }
            if let Err(err) = transcript.append_message(message) {
                self.lines.push(Line::Error(format!("could not save the session: {err}")));
                return;
            }
        }
    }

    /// Append streamed text, extending the last assistant line when possible.
    fn push_assistant(&mut self, delta: &str) {
        match self.lines.last_mut() {
            Some(Line::Assistant(text)) => text.push_str(delta),
            _ => self.lines.push(Line::Assistant(delta.to_string())),
        }
    }

    fn push_thinking(&mut self, delta: &str) {
        match self.lines.last_mut() {
            Some(Line::Thinking(text)) => text.push_str(delta),
            _ => self.lines.push(Line::Thinking(delta.to_string())),
        }
    }
}

/// Copy text to the system clipboard.
///
/// Uses the platform's own tool rather than a clipboard crate: it is one
/// process, it works over SSH-forwarded sessions, and it adds no dependency.
fn copy_to_clipboard(text: &str) -> std::io::Result<()> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let (program, args): (&str, &[&str]) = if cfg!(windows) {
        ("clip", &[])
    } else if cfg!(target_os = "macos") {
        ("pbcopy", &[])
    } else {
        ("wl-copy", &[])
    };

    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    if let Some(stdin) = child.stdin.as_mut() {
        stdin.write_all(text.as_bytes())?;
    }
    child.wait()?;
    Ok(())
}

/// Render tool arguments as one short line.
fn compact_arguments(args: &serde_json::Value) -> String {
    for key in ["command", "path", "pattern", "goal", "title"] {
        if let Some(value) = args.get(key).and_then(|value| value.as_str()) {
            return value.to_string();
        }
    }
    let rendered = args.to_string();
    if rendered.chars().count() > 120 {
        rendered.chars().take(120).collect::<String>() + "…"
    } else {
        rendered
    }
}

/// The transcript as plain text, for copying out.
pub fn transcript_text(lines: &[Line]) -> String {
    let mut out = String::new();
    for line in lines {
        match line {
            Line::User(text) => out.push_str(&format!("> {text}\n")),
            Line::Assistant(text) => out.push_str(&format!("{text}\n")),
            Line::Thinking(text) => out.push_str(&format!("[thinking] {text}\n")),
            Line::Tool { name, detail, .. } => out.push_str(&format!("[{name}] {detail}\n")),
            Line::ToolOutput(text) => out.push_str(&format!("{text}\n")),
            Line::Notice { text, .. } => out.push_str(&format!("· {text}\n")),
            Line::Error(text) => out.push_str(&format!("! {text}\n")),
        }
    }
    out
}

/// Whether a message should be persisted to the transcript.
pub fn is_persistable(message: &Message) -> bool {
    message.role != Role::System
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn compact_arguments_prefers_the_interesting_field() {
        assert_eq!(compact_arguments(&json!({"command": "cargo test"})), "cargo test");
        assert_eq!(compact_arguments(&json!({"path": "src/main.rs"})), "src/main.rs");
    }

    #[test]
    fn compact_arguments_truncates_the_fallback() {
        let long = json!({ "blob": "x".repeat(400) });
        assert!(compact_arguments(&long).chars().count() <= 121);
    }

    #[test]
    fn transcript_text_marks_each_kind() {
        let lines = vec![
            Line::User("hi".into()),
            Line::Assistant("hello".into()),
            Line::Error("boom".into()),
        ];
        let text = transcript_text(&lines);
        assert!(text.contains("> hi"));
        assert!(text.contains("hello"));
        assert!(text.contains("! boom"));
    }

    #[test]
    fn system_messages_are_not_persisted() {
        assert!(!is_persistable(&Message { role: Role::System, blocks: vec![] }));
        assert!(is_persistable(&Message::user("x")));
    }
}
