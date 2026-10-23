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

/// What the conversation pane renders.
#[derive(Debug, Clone)]
pub enum Line {
    User(String),
    Assistant(String),
    Tool {
        name: String,
        detail: String,
        is_error: bool,
    },
    Transition(String),
    Error(String),
    Status(String),
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

pub struct App {
    pub lines: Vec<Line>,
    pub input: String,
    pub scroll: u16,
    pub should_quit: bool,
    pub running: bool,
    pub status: String,
    pub approval: Option<PendingApproval>,
    /// Per-agent dashboard state, keyed by agent id.
    pub agents: HashMap<String, AgentState>,
    /// Which agent pane is focused (None = main conversation).
    pub focus_agent: Option<String>,
}

impl App {
    fn new() -> Self {
        Self {
            lines: Vec::new(),
            input: String::new(),
            scroll: 0,
            should_quit: false,
            running: false,
            status: "idle".into(),
            approval: None,
            agents: HashMap::new(),
            focus_agent: None,
        }
    }

    fn push(&mut self, line: Line) {
        self.lines.push(line);
        self.scroll = 0;
    }

    fn touch_agent(&mut self, agent_id: &str) -> &mut AgentState {
        self.agents.entry(agent_id.to_string()).or_default()
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
) -> Result<(), Box<dyn std::error::Error>> {
    let mut app = App::new();
    let mut rx = bus.subscribe();
    let cancel = CancellationToken::new();

    // Swap the context's approval sink for the interactive one.
    let pending_map: Arc<Mutex<HashMap<uuid::Uuid, tokio::sync::oneshot::Sender<bool>>>> =
        Arc::new(Mutex::new(HashMap::new()));
    let approvals = Arc::new(UiApprovals {
        pending: pending_map.clone(),
        cancel: cancel.clone(),
    });
    let ctx = AgentContext { approvals, ..ctx };

    app.push(Line::Status(format!(
        "Luna ready — provider `{}`, model `{}`. Type a message; Esc quits; Ctrl+D cancels a run.",
        spec.provider_key, spec.model
    )));

    loop {
        // Drain core events into lines.
        loop {
            match rx.try_recv() {
                Ok(ev) => absorb(&mut app, &ev),
                Err(tokio::sync::broadcast::error::TryRecvError::Empty) => break,
                Err(tokio::sync::broadcast::error::TryRecvError::Lagged(n)) => {
                    app.push(Line::Status(format!("(skipped {n} events — UI lag)")));
                }
                Err(tokio::sync::broadcast::error::TryRecvError::Closed) => break,
            }
        }

        terminal.draw(|f| ui::draw(f, &app))?;

        if crossterm::event::poll(std::time::Duration::from_millis(50))?
            && let crossterm::event::Event::Key(key) = crossterm::event::read()?
            && key.kind == crossterm::event::KeyEventKind::Press
        {
            use crossterm::event::{KeyCode, KeyModifiers};
            match (key.code, key.modifiers) {
                (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                    cancel.cancel();
                    app.should_quit = true;
                }
                (KeyCode::Char('d'), KeyModifiers::CONTROL) if app.running => {
                    cancel.cancel();
                }
                (KeyCode::Tab, _) => {
                    // Cycle focus: main → each agent → main.
                    let ids: Vec<String> = {
                        let mut v: Vec<String> = app.agents.keys().cloned().collect();
                        v.sort();
                        v
                    };
                    app.focus_agent = match &app.focus_agent {
                        None => ids.first().cloned(),
                        Some(cur) => {
                            let idx = ids
                                .iter()
                                .position(|i| i == cur)
                                .map(|i| i + 1)
                                .unwrap_or(usize::MAX);
                            if idx < ids.len() {
                                Some(ids[idx].clone())
                            } else {
                                None
                            }
                        }
                    };
                }
                (KeyCode::Esc, _) => {
                    if let Some(p) = app.approval.take() {
                        resolve(&pending_map, p.request_id, false);
                    } else if !app.running {
                        app.should_quit = true;
                    }
                }
                // Approval shortcuts.
                (KeyCode::Char('y'), _) if app.approval.is_some() => {
                    if let Some(p) = app.approval.take() {
                        resolve(&pending_map, p.request_id, true);
                    }
                }
                (KeyCode::Char('n'), _) if app.approval.is_some() => {
                    if let Some(p) = app.approval.take() {
                        resolve(&pending_map, p.request_id, false);
                    }
                }
                (KeyCode::Enter, _) if app.approval.is_some() => {
                    if let Some(p) = app.approval.take() {
                        resolve(&pending_map, p.request_id, true);
                    }
                }
                (KeyCode::Enter, _) if !app.running => {
                    let text = app.input.trim().to_string();
                    app.input.clear();
                    if !text.is_empty() {
                        app.push(Line::User(text.clone()));
                        app.running = true;
                        app.status = "running…".into();

                        let ctx2 = clone_ctx(&ctx);
                        let spec2 = spec.clone();
                        let cancel2 = cancel.clone();
                        let history2 = history.clone();
                        let (done_tx, done_rx) =
                            std::sync::mpsc::channel::<Result<String, String>>();
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

                        // Keep drawing while the run goes.
                        loop {
                            terminal.draw(|f| ui::draw(f, &app))?;
                            if done_rx
                                .recv_timeout(std::time::Duration::from_millis(50))
                                .is_ok()
                            {
                                break;
                            }
                            while let Ok(ev) = rx.try_recv() {
                                absorb(&mut app, &ev);
                            }
                            if crossterm::event::poll(std::time::Duration::from_millis(0))?
                                && let crossterm::event::Event::Key(k) = crossterm::event::read()?
                                && k.kind == crossterm::event::KeyEventKind::Press
                            {
                                use crossterm::event::{KeyCode as K, KeyModifiers as M};
                                match (k.code, k.modifiers) {
                                    (K::Char('d'), m) if m.contains(M::CONTROL) => cancel.cancel(),
                                    (K::Char('c'), m) if m.contains(M::CONTROL) => {
                                        cancel.cancel();
                                        app.should_quit = true;
                                    }
                                    (K::Char('y'), _) | (K::Enter, _) => {
                                        if let Some(p) = app.approval.take() {
                                            resolve(&pending_map, p.request_id, true);
                                        }
                                    }
                                    (K::Char('n'), _) | (K::Esc, _) => {
                                        if let Some(p) = app.approval.take() {
                                            resolve(&pending_map, p.request_id, false);
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }

                        app.running = false;
                        app.status = "idle".into();
                    }
                }
                (KeyCode::Backspace, _) => {
                    app.input.pop();
                }
                (KeyCode::Char(c), _) => {
                    app.input.push(c);
                }
                _ => {}
            }
        }

        if app.should_quit {
            cancel.cancel();
            break;
        }
    }
    Ok(())
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
            match app.lines.last_mut() {
                Some(Line::Assistant(s)) => s.push_str(t),
                _ => app.push(Line::Assistant(t.clone())),
            }
        }
        EventKind::ToolCallRequested {
            name, arguments, ..
        } => {
            if let Some(a) = &ev.agent_id {
                app.touch_agent(a).tool_calls += 1;
                app.touch_agent(a).status = format!("tool: {name}");
            }
            app.push(Line::Tool {
                name: name.clone(),
                detail: serde_json::to_string(arguments).unwrap_or_default(),
                is_error: false,
            });
        }
        EventKind::ToolCallFinished { name, outcome, .. } => {
            if let Some(a) = &ev.agent_id {
                app.touch_agent(a).status = "running".into();
            }
            app.push(Line::Tool {
                name: format!("{name} →"),
                detail: if outcome.content.len() > 400 {
                    format!("{}…", &outcome.content[..400])
                } else {
                    outcome.content.clone()
                },
                is_error: outcome.is_error,
            });
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
            app.push(Line::Transition(format!(
                "spawned {agent_id}{}",
                parent
                    .as_ref()
                    .map(|p| format!(" (child of {p})"))
                    .unwrap_or_default()
            )));
        }
        EventKind::OrchestratorTransition(d) => app.push(Line::Transition(d.clone())),
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
                    app.push(Line::Error(e.clone()));
                } else {
                    app.push(Line::Status("cancelled".into()));
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
