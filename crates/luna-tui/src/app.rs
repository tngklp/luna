//! TUI application loop: input handling, event pumping, agent runs.

use crate::ui;
use luna_core::EventBus;
use luna_core::event::{EventKind, LunaEvent};
use luna_core::orchestrator::agent_loop::{AgentContext, AgentRunner, AgentSpec, ApprovalSink};
use luna_core::provider::types::Message;
use ratatui::Terminal;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio_util::sync::CancellationToken;

/// One item in the transcript.
#[derive(Debug, Clone)]
pub enum Item {
    User(String),
    /// Assistant text (markdown).
    Assistant(String),
    /// A tool call with its result.
    Tool(ToolItem),
    /// Orchestrator/agent lifecycle note.
    Note(String),
    Error(String),
}

/// A tool invocation shown as a collapsible card.
#[derive(Debug, Clone)]
pub struct ToolItem {
    pub name: String,
    pub args: String,
    pub output: Option<String>,
    pub is_error: bool,
    pub running: bool,
    pub expanded: bool,
}

/// A pending approval awaiting the user's decision.
#[derive(Debug, Clone)]
pub struct PendingApproval {
    pub request_id: uuid::Uuid,
    pub tool_name: String,
    pub arguments: String,
}

/// Live state of one agent in a multi-agent run.
#[derive(Debug, Clone, Default)]
pub struct AgentState {
    pub parent: Option<String>,
    pub status: String,
    pub last_output: String,
    pub tool_calls: u32,
}

/// Which pane has keyboard focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Composer,
    Transcript,
}

/// A slash command offered in the composer.
pub struct SlashCommand {
    pub name: &'static str,
    pub description: &'static str,
}

pub const SLASH_COMMANDS: &[SlashCommand] = &[
    SlashCommand {
        name: "/new",
        description: "start a new session",
    },
    SlashCommand {
        name: "/sessions",
        description: "browse saved sessions",
    },
    SlashCommand {
        name: "/model",
        description: "show the active provider and model",
    },
    SlashCommand {
        name: "/clear",
        description: "clear the transcript",
    },
    SlashCommand {
        name: "/help",
        description: "show keybindings and commands",
    },
    SlashCommand {
        name: "/quit",
        description: "exit Luna",
    },
];

pub struct App {
    pub items: Vec<Item>,
    pub input: String,
    pub cursor: usize,
    pub scroll: u16,
    pub should_quit: bool,
    pub running: bool,
    pub status: String,
    pub approval: Option<PendingApproval>,
    pub agents: HashMap<String, AgentState>,
    pub focus: Focus,
    /// Slash-command menu selection index (None = menu hidden).
    pub slash_index: Option<usize>,
    /// Session picker overlay: (id, preview) pairs.
    pub sessions: Vec<(String, String)>,
    pub session_picker: bool,
    pub session_index: usize,
    /// Provider/model label for the status bar.
    pub model_label: String,
    /// Tokens used in this session (rough).
    pub tokens: u64,
    /// Spinner frame for the running indicator.
    pub tick: u64,
}

impl App {
    fn new(model_label: String) -> Self {
        Self {
            items: Vec::new(),
            input: String::new(),
            cursor: 0,
            scroll: 0,
            should_quit: false,
            running: false,
            status: "idle".into(),
            approval: None,
            agents: HashMap::new(),
            focus: Focus::Composer,
            slash_index: None,
            sessions: Vec::new(),
            session_picker: false,
            session_index: 0,
            model_label,
            tokens: 0,
            tick: 0,
        }
    }

    fn push(&mut self, item: Item) {
        self.items.push(item);
        self.scroll = 0;
    }

    fn touch_agent(&mut self, agent_id: &str) -> &mut AgentState {
        self.agents.entry(agent_id.to_string()).or_default()
    }

    /// Slash commands matching the current input.
    pub fn slash_matches(&self) -> Vec<&'static SlashCommand> {
        if !self.input.starts_with('/') || self.input.contains(' ') {
            return Vec::new();
        }
        let q = self.input.to_lowercase();
        SLASH_COMMANDS
            .iter()
            .filter(|c| c.name.starts_with(&q))
            .collect()
    }

    fn sync_slash_menu(&mut self) {
        let n = self.slash_matches().len();
        self.slash_index = if n > 0 { Some(0) } else { None };
    }
}

/// Interactive approval sink: the loop publishes `ApprovalRequested`, the UI
/// picks it up, and this sink parks until the UI resolves it (or cancel).
struct UiApprovals {
    pending: Arc<Mutex<HashMap<uuid::Uuid, tokio::sync::oneshot::Sender<bool>>>>,
    cancel: CancellationToken,
}

#[async_trait::async_trait]
impl ApprovalSink for UiApprovals {
    async fn request_approval(
        &self,
        request_id: uuid::Uuid,
        _agent_id: &str,
        _tool_name: &str,
        _arguments: &serde_json::Value,
        cancel: &CancellationToken,
    ) -> bool {
        let (tx, rx) = tokio::sync::oneshot::channel::<bool>();
        self.pending.lock().unwrap().insert(request_id, tx);
        tokio::select! {
            decision = rx => decision.unwrap_or(false),
            _ = cancel.cancelled() => false,
            _ = self.cancel.cancelled() => false,
        }
    }
}

pub fn run(
    terminal: &mut Terminal<ratatui::prelude::CrosstermBackend<std::io::Stdout>>,
    bus: EventBus,
    ctx: AgentContext,
    spec: AgentSpec,
    history: Arc<Mutex<Vec<Message>>>,
    store: Option<Arc<luna_core::storage::Store>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let model_label = format!("{} · {}", spec.provider_key, spec.model);
    let mut app = App::new(model_label);
    let mut rx = bus.subscribe();
    let cancel = CancellationToken::new();

    let pending_map: Arc<Mutex<HashMap<uuid::Uuid, tokio::sync::oneshot::Sender<bool>>>> =
        Arc::new(Mutex::new(HashMap::new()));
    let approvals = Arc::new(UiApprovals {
        pending: pending_map.clone(),
        cancel: cancel.clone(),
    });
    let ctx = AgentContext { approvals, ..ctx };

    app.push(Item::Note(format!(
        "Luna ready — {}. Type a message, or /help for commands.",
        app.model_label
    )));

    loop {
        // Drain core events into items.
        loop {
            match rx.try_recv() {
                Ok(ev) => absorb(&mut app, &ev),
                Err(tokio::sync::broadcast::error::TryRecvError::Empty) => break,
                Err(tokio::sync::broadcast::error::TryRecvError::Lagged(n)) => {
                    app.push(Item::Note(format!("(skipped {n} events — UI lag)")));
                }
                Err(tokio::sync::broadcast::error::TryRecvError::Closed) => break,
            }
        }

        app.tick = app.tick.wrapping_add(1);
        terminal.draw(|f| ui::draw(f, &app))?;

        if crossterm::event::poll(std::time::Duration::from_millis(80))?
            && let crossterm::event::Event::Key(key) = crossterm::event::read()?
            && key.kind == crossterm::event::KeyEventKind::Press
        {
            handle_key(
                &mut app,
                key,
                &cancel,
                &pending_map,
                &bus,
                &ctx,
                &spec,
                &history,
                &store,
            );
        }

        if app.should_quit {
            cancel.cancel();
            break;
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn handle_key(
    app: &mut App,
    key: crossterm::event::KeyEvent,
    cancel: &CancellationToken,
    pending_map: &Arc<Mutex<HashMap<uuid::Uuid, tokio::sync::oneshot::Sender<bool>>>>,
    _bus: &EventBus,
    ctx: &AgentContext,
    spec: &AgentSpec,
    history: &Arc<Mutex<Vec<Message>>>,
    store: &Option<Arc<luna_core::storage::Store>>,
) {
    use crossterm::event::{KeyCode, KeyModifiers};

    // Global keys.
    match (key.code, key.modifiers) {
        (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
            cancel.cancel();
            app.should_quit = true;
            return;
        }
        (KeyCode::Char('d'), KeyModifiers::CONTROL) if app.running => {
            cancel.cancel();
            return;
        }
        _ => {}
    }

    // Approval modal takes precedence.
    if app.approval.is_some() {
        match key.code {
            KeyCode::Char('y') | KeyCode::Enter => {
                if let Some(p) = app.approval.take() {
                    resolve(pending_map, p.request_id, true);
                }
            }
            KeyCode::Char('n') | KeyCode::Esc => {
                if let Some(p) = app.approval.take() {
                    resolve(pending_map, p.request_id, false);
                }
            }
            _ => {}
        }
        return;
    }

    // Session picker overlay.
    if app.session_picker {
        match key.code {
            KeyCode::Esc => app.session_picker = false,
            KeyCode::Up => app.session_index = app.session_index.saturating_sub(1),
            KeyCode::Down => {
                if app.session_index + 1 < app.sessions.len() {
                    app.session_index += 1;
                }
            }
            KeyCode::Enter => {
                if let Some((id, _)) = app.sessions.get(app.session_index).cloned() {
                    load_session(app, store, &id);
                }
                app.session_picker = false;
            }
            _ => {}
        }
        return;
    }

    // Slash menu navigation.
    if app.slash_index.is_some() {
        let matches = app.slash_matches();
        match key.code {
            KeyCode::Esc => {
                app.slash_index = None;
                return;
            }
            KeyCode::Up => {
                let i = app.slash_index.unwrap_or(0);
                app.slash_index = Some(i.saturating_sub(1));
                return;
            }
            KeyCode::Down => {
                let i = app.slash_index.unwrap_or(0);
                if i + 1 < matches.len() {
                    app.slash_index = Some(i + 1);
                }
                return;
            }
            KeyCode::Tab => {
                if let Some(cmd) = matches.get(app.slash_index.unwrap_or(0)) {
                    app.input = cmd.name.to_string();
                    app.cursor = app.input.len();
                    app.slash_index = None;
                }
                return;
            }
            KeyCode::Enter => {
                if let Some(cmd) = matches.get(app.slash_index.unwrap_or(0)) {
                    let name = cmd.name.to_string();
                    app.input.clear();
                    app.cursor = 0;
                    app.slash_index = None;
                    run_slash(app, &name, store);
                    return;
                }
            }
            _ => {}
        }
    }

    // Transcript scrolling.
    match key.code {
        KeyCode::PageUp => {
            app.scroll = app.scroll.saturating_add(5);
            return;
        }
        KeyCode::PageDown => {
            app.scroll = app.scroll.saturating_sub(5);
            return;
        }
        KeyCode::Tab => {
            app.focus = match app.focus {
                Focus::Composer => Focus::Transcript,
                Focus::Transcript => Focus::Composer,
            };
            return;
        }
        _ => {}
    }

    // Composer editing.
    match key.code {
        KeyCode::Esc => {
            if !app.running {
                app.should_quit = true;
            }
        }
        KeyCode::Enter => {
            if app.running {
                return;
            }
            let text = app.input.trim().to_string();
            app.input.clear();
            app.cursor = 0;
            app.slash_index = None;
            if text.is_empty() {
                return;
            }
            if text.starts_with('/') {
                run_slash(app, &text, store);
                return;
            }
            start_run(app, text, cancel, ctx, spec, history);
        }
        KeyCode::Backspace => {
            if app.cursor > 0 {
                app.cursor -= 1;
                app.input.remove(app.cursor);
                app.sync_slash_menu();
            }
        }
        KeyCode::Left => app.cursor = app.cursor.saturating_sub(1),
        KeyCode::Right => {
            if app.cursor < app.input.len() {
                app.cursor += 1;
            }
        }
        KeyCode::Char(c) => {
            app.input.insert(app.cursor, c);
            app.cursor += 1;
            app.sync_slash_menu();
        }
        _ => {}
    }
}

fn run_slash(app: &mut App, cmd: &str, store: &Option<Arc<luna_core::storage::Store>>) {
    match cmd {
        "/quit" | "/q" => app.should_quit = true,
        "/clear" => {
            app.items.clear();
            app.push(Item::Note("transcript cleared".into()));
        }
        "/new" => {
            app.items.clear();
            app.push(Item::Note("new session".into()));
        }
        "/model" => {
            let label = app.model_label.clone();
            app.push(Item::Note(format!("active: {label}")));
        }
        "/help" => {
            app.push(Item::Note(
                "keys: Enter send · Esc quit · Ctrl+D cancel · PgUp/PgDn scroll · Tab focus".into(),
            ));
            app.push(Item::Note(
                "commands: /new /sessions /model /clear /help /quit".into(),
            ));
        }
        "/sessions" => {
            app.sessions.clear();
            if let Some(store) = store {
                match store.list_sessions(30) {
                    Ok(list) => {
                        for s in list {
                            let preview = s.first_user_message.unwrap_or_else(|| "(empty)".into());
                            app.sessions.push((s.id, preview));
                        }
                    }
                    Err(e) => app.push(Item::Error(format!("sessions: {e}"))),
                }
            }
            if app.sessions.is_empty() {
                app.push(Item::Note("no saved sessions".into()));
            } else {
                app.session_index = 0;
                app.session_picker = true;
            }
        }
        other => app.push(Item::Error(format!("unknown command: {other}"))),
    }
}

fn load_session(app: &mut App, store: &Option<Arc<luna_core::storage::Store>>, id: &str) {
    let Some(store) = store else { return };
    match store.load_events(id) {
        Ok(events) => {
            app.items.clear();
            app.push(Item::Note(format!("resumed session {id}")));
            for ev in events {
                absorb(app, &ev);
            }
        }
        Err(e) => app.push(Item::Error(format!("load session: {e}"))),
    }
}

fn start_run(
    app: &mut App,
    text: String,
    cancel: &CancellationToken,
    ctx: &AgentContext,
    spec: &AgentSpec,
    history: &Arc<Mutex<Vec<Message>>>,
) {
    app.push(Item::User(text.clone()));
    app.running = true;
    app.status = "running".into();

    let ctx2 = clone_ctx(ctx);
    let spec2 = spec.clone();
    let cancel2 = cancel.clone();
    let history2 = history.clone();
    let (done_tx, done_rx) = std::sync::mpsc::channel::<Result<String, String>>();
    history2.lock().unwrap().push(Message::user(text));
    let history3 = history2.clone();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
        let out = rt.block_on(async move {
            let mut hist = history3.lock().unwrap().clone();
            let runner = AgentRunner::new(spec2);
            let res = runner.run(&ctx2, &mut hist, cancel2).await;
            *history3.lock().unwrap() = hist;
            res.map_err(|e| e.to_string())
        });
        let _ = done_tx.send(out);
    });

    // Block until the run finishes, pumping events so the UI stays live.
    // (The outer loop redraws; this just waits for completion.)
    let _ = done_rx.recv();
    app.running = false;
    app.status = "idle".into();
}

fn resolve(
    map: &Arc<Mutex<HashMap<uuid::Uuid, tokio::sync::oneshot::Sender<bool>>>>,
    id: uuid::Uuid,
    approved: bool,
) {
    if let Some(tx) = map.lock().unwrap().remove(&id) {
        let _ = tx.send(approved);
    }
}

fn absorb(app: &mut App, ev: &LunaEvent) {
    match &ev.kind {
        EventKind::TokenDelta(t) => {
            let agent = ev.agent_id.clone().unwrap_or_default();
            app.touch_agent(&agent).last_output.push_str(t);
            app.tokens += (t.len() / 4) as u64;
            match app.items.last_mut() {
                Some(Item::Assistant(s)) => s.push_str(t),
                _ => app.push(Item::Assistant(t.clone())),
            }
        }
        EventKind::ToolCallRequested {
            name, arguments, ..
        } => {
            if let Some(a) = &ev.agent_id {
                app.touch_agent(a).tool_calls += 1;
                app.touch_agent(a).status = format!("tool: {name}");
            }
            app.push(Item::Tool(ToolItem {
                name: name.clone(),
                args: serde_json::to_string_pretty(arguments).unwrap_or_default(),
                output: None,
                is_error: false,
                running: true,
                expanded: false,
            }));
        }
        EventKind::ToolCallFinished { outcome, .. } => {
            if let Some(a) = &ev.agent_id {
                app.touch_agent(a).status = "running".into();
            }
            // Attach to the most recent running tool card.
            if let Some(Item::Tool(t)) = app
                .items
                .iter_mut()
                .rev()
                .find(|i| matches!(i, Item::Tool(t) if t.running))
            {
                t.output = Some(outcome.content.clone());
                t.is_error = outcome.is_error;
                t.running = false;
            }
        }
        EventKind::ApprovalRequested {
            request_id,
            tool_name,
            arguments,
            ..
        } => {
            app.approval = Some(PendingApproval {
                request_id: *request_id,
                tool_name: tool_name.clone(),
                arguments: serde_json::to_string_pretty(arguments).unwrap_or_default(),
            });
        }
        EventKind::AgentSpawned {
            agent_id, parent, ..
        } => {
            let st = app.touch_agent(agent_id);
            st.parent = parent.clone();
            st.status = "spawned".into();
            app.push(Item::Note(format!(
                "spawned {agent_id}{}",
                parent
                    .as_ref()
                    .map(|p| format!(" (child of {p})"))
                    .unwrap_or_default()
            )));
        }
        EventKind::OrchestratorTransition(d) => app.push(Item::Note(d.clone())),
        EventKind::RunFinished { summary, error } => {
            if let Some(a) = &ev.agent_id {
                let st = app.touch_agent(a);
                if let Some(s) = summary {
                    st.last_output = s.clone();
                }
                st.status = if error.is_some() {
                    "failed".into()
                } else {
                    "done".into()
                };
            }
            if let Some(e) = error {
                if e != "cancelled" {
                    app.push(Item::Error(e.clone()));
                } else {
                    app.push(Item::Note("cancelled".into()));
                }
            }
        }
        EventKind::UserMessage(_)
        | EventKind::RunStarted
        | EventKind::AssistantMessage(_)
        | EventKind::ApprovalResolved { .. } => {}
    }
}

fn clone_ctx(ctx: &AgentContext) -> AgentContext {
    AgentContext {
        provider: ctx.provider.clone(),
        tools: ctx.tools.clone(),
        gate: ctx.gate.clone(),
        bus: ctx.bus.clone(),
        session_id: ctx.session_id,
        approvals: ctx.approvals.clone(),
    }
}
