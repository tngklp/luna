//! Luna GUI: Tauri backend hosting the luna-core agent harness.
//!
//! Events flow core → webview via Tauri's event system; commands flow
//! webview → core via the `luna_*` commands below.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use luna_core::event::{EventKind, LunaEvent};
use luna_core::orchestrator::agent_loop::{AgentContext, AgentRunner, AgentSpec, ApprovalSink};
use luna_core::orchestrator::registry::ProviderSettings;
use luna_core::orchestrator::AgentRegistry;
use luna_core::provider::types::Message;
use luna_core::tool::builtin;
use luna_core::tool::permissions::PermissionGate;
use luna_core::tool::registry::ToolRegistry;
use luna_core::{Command, EventBus};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio_util::sync::CancellationToken;

/// Shared state bridging the webview and the core.
pub struct LunaState {
    pub bus: EventBus,
    pub commands: tokio::sync::mpsc::Sender<Command>,
    pub cancel: CancellationToken,
    pub session_id: uuid::Uuid,
    pub history: Mutex<Vec<Message>>,
    /// Pending approval resolvers, keyed by request id.
    pub approvals: Arc<Mutex<HashMap<uuid::Uuid, tokio::sync::oneshot::Sender<bool>>>>,
}

/// Interactive approval sink: parks until the webview resolves the request.
struct GuiApprovals {
    pending: Arc<Mutex<HashMap<uuid::Uuid, tokio::sync::oneshot::Sender<bool>>>>,
    cancel: CancellationToken,
}

#[async_trait::async_trait]
impl ApprovalSink for GuiApprovals {
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

/// A JSON-serializable snapshot of a core event for the webview.
#[derive(serde::Serialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
enum UiEvent {
    RunStarted,
    UserMessage { text: String },
    AssistantMessage { text: String },
    TokenDelta { text: String },
    ToolCallRequested { call_id: String, name: String, arguments: serde_json::Value },
    ToolCallFinished { call_id: String, name: String, outcome: String, is_error: bool },
    ApprovalRequested { request_id: String, agent_id: String, tool_name: String, arguments: serde_json::Value },
    ApprovalResolved { request_id: String, approved: bool },
    AgentSpawned { agent_id: String, parent: Option<String>, spec_name: String },
    OrchestratorTransition { description: String },
    RunFinished { summary: Option<String>, error: Option<String> },
}

impl UiEvent {
    fn from_core(ev: &LunaEvent) -> Option<(String, Self)> {
        let agent = ev.agent_id.clone().unwrap_or_default();
        let payload = match &ev.kind {
            EventKind::RunStarted => UiEvent::RunStarted,
            EventKind::UserMessage(text) => UiEvent::UserMessage { text: text.clone() },
            EventKind::AssistantMessage(text) => UiEvent::AssistantMessage { text: text.clone() },
            EventKind::TokenDelta(text) => UiEvent::TokenDelta { text: text.clone() },
            EventKind::ToolCallRequested { call_id, name, arguments } => UiEvent::ToolCallRequested {
                call_id: call_id.clone(),
                name: name.clone(),
                arguments: arguments.clone(),
            },
            EventKind::ToolCallFinished { call_id, name, outcome } => UiEvent::ToolCallFinished {
                call_id: call_id.clone(),
                name: name.clone(),
                outcome: outcome.content.clone(),
                is_error: outcome.is_error,
            },
            EventKind::ApprovalRequested { request_id, agent_id, tool_name, arguments } => {
                UiEvent::ApprovalRequested {
                    request_id: request_id.to_string(),
                    agent_id: agent_id.clone(),
                    tool_name: tool_name.clone(),
                    arguments: arguments.clone(),
                }
            }
            EventKind::ApprovalResolved { request_id, approved } => UiEvent::ApprovalResolved {
                request_id: request_id.to_string(),
                approved: *approved,
            },
            EventKind::AgentSpawned { agent_id, parent, spec_name } => UiEvent::AgentSpawned {
                agent_id: agent_id.clone(),
                parent: parent.clone(),
                spec_name: spec_name.clone(),
            },
            EventKind::OrchestratorTransition(d) => {
                UiEvent::OrchestratorTransition { description: d.clone() }
            }
            EventKind::RunFinished { summary, error } => UiEvent::RunFinished {
                summary: summary.clone(),
                error: error.clone(),
            },
        };
        Some((agent, payload))
    }
}

/// Background task: forward core events to the webview.
fn spawn_event_forwarder(app: AppHandle, bus: EventBus) {
    tokio::spawn(async move {
        let mut rx = bus.subscribe();
        loop {
            match rx.recv().await {
                Ok(ev) => {
                    if let Some((agent, payload)) = UiEvent::from_core(&ev) {
                        let event_name = if agent.is_empty() {
                            "luna://event".to_string()
                        } else {
                            format!("luna://event/{agent}")
                        };
                        let _ = app.emit(&event_name, payload);
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!(skipped = n, "ui event receiver lagged");
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}

#[tauri::command]
async fn luna_send_message(state: State<'_, LunaState>, text: String) -> Result<(), String> {
    state
        .commands
        .send(Command::UserMessage(text))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn luna_cancel(state: State<'_, LunaState>) -> Result<(), String> {
    state.cancel.cancel();
    Ok(())
}

#[tauri::command]
async fn luna_respond_approval(
    state: State<'_, LunaState>,
    request_id: String,
    approved: bool,
) -> Result<(), String> {
    let rid = uuid::Uuid::parse_str(&request_id).map_err(|e| e.to_string())?;
    if let Some(tx) = state.approvals.lock().unwrap().remove(&rid) {
        let _ = tx.send(approved);
    }
    Ok(())
}

/// Run one agent turn end-to-end, streaming events to the webview.
#[tauri::command]
async fn luna_run_agent(
    _app: AppHandle,
    state: State<'_, LunaState>,
    text: String,
) -> Result<String, String> {
    // Build the core stack (single enabled provider for now; orchestrator
    // registry lands with multi-agent UI).
    let cfg = luna_core::config::Config::load().map_err(|e| e.to_string())?;
    let (provider_key, provider, settings) = pick_provider(&cfg).ok_or_else(|| {
        "no provider enabled: set ANTHROPIC_API_KEY / OPENAI_API_KEY or enable ollama in config.toml".to_string()
    })?;

    let mut registry = AgentRegistry::new();
    registry.register_provider(
        &provider_key,
        ProviderSettings {
            model: settings.0.clone(),
            max_tokens: settings.1,
            temperature: settings.2,
        },
    );
    let spec = if let Some(agent_cfg) = cfg.agents.first() {
        registry.register_agent(agent_cfg.clone());
        registry.resolve(&agent_cfg.name).map_err(|e| e.to_string())?
    } else {
        AgentSpec {
            name: "assistant".into(),
            provider_key: provider_key.clone(),
            model: settings.0,
            system: Some("You are Luna, a helpful coding agent.".into()),
            tools: vec![],
            max_iterations: 25,
            max_tokens: settings.1,
            temperature: settings.2,
        }
    };

    let mut tools = ToolRegistry::new();
    builtin::register_all(&mut tools);

    let ctx = AgentContext {
        provider,
        tools,
        gate: Arc::new(PermissionGate::new(
            PermissionGate::default_rules(),
            cfg.permissions.approval_tools.clone(),
        )),
        bus: state.bus.clone(),
        session_id: state.session_id,
        approvals: Arc::new(GuiApprovals {
            pending: state.approvals.clone(),
            cancel: state.cancel.clone(),
        }),
    };

    // Record the user message and run the loop.
    {
        let mut history = state.history.lock().unwrap();
        history.push(Message::user(text.clone()));
    }
    state
        .commands
        .send(Command::UserMessage(text.clone()))
        .await
        .map_err(|e| e.to_string())?;

    let mut history = state.history.lock().unwrap().clone();
    let runner = AgentRunner::new(spec);
    let cancel = state.cancel.clone();
    let result = runner.run(&ctx, &mut history, cancel).await;
    {
        let mut h = state.history.lock().unwrap();
        *h = history;
    }
    match result {
        Ok(text) => Ok(text),
        Err(e) if e.is_cancelled() => Ok("(cancelled)".into()),
        Err(e) => Err(e.to_string()),
    }
}

/// Ensure `luna` (the TUI CLI) is runnable from a terminal.
///
/// The GUI setup installs both binaries side by side; this shim adds the
/// install dir to the user PATH on Windows if it's missing (best-effort,
/// failures are non-fatal).
fn install_cli_shim() {
    #[cfg(windows)]
    {
        let Some(exe_dir) = std::env::current_exe().ok().and_then(|p| p.parent().map(|p| p.to_path_buf())) else {
            return;
        };
        if !exe_dir.join("luna.exe").exists() {
            return;
        }
        let dir_str = exe_dir.to_string_lossy().to_string();
        let user_path = std::env::var("PATH").unwrap_or_default();
        if user_path.split(';').any(|p| p.eq_ignore_ascii_case(&dir_str)) {
            return;
        }
        // setx truncates >1024 chars; guard against clobbering long PATHs.
        if user_path.len() > 900 {
            tracing::warn!("user PATH too long; skipping CLI PATH registration");
            return;
        }
        let _ = std::process::Command::new("setx")
            .arg("Path")
            .arg(format!("{user_path};{dir_str}"))
            .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
            .output();
    }
}

#[cfg(windows)]
trait CreationFlagsExt {
    fn creation_flags(&mut self, flags: u32) -> &mut Self;
}

#[cfg(windows)]
impl CreationFlagsExt for std::process::Command {
    fn creation_flags(&mut self, flags: u32) -> &mut Self {
        std::os::windows::process::CommandExt::creation_flags(self, flags)
    }
}

fn pick_provider(cfg: &luna_core::config::Config) -> Option<(String, std::sync::Arc<dyn luna_core::provider::Provider>, (String, u32, f32))> {    // Env-var API keys take precedence in provider selection order.
    if cfg.providers.anthropic.enabled {
        if let Ok(key) = std::env::var("ANTHROPIC_API_KEY") {
            if let Ok(p) = luna_core::provider::anthropic::AnthropicProvider::new(key) {
                let c = &cfg.providers.anthropic;
                return Some(("anthropic".into(), std::sync::Arc::new(p), (c.model.clone(), c.max_tokens, c.temperature)));
            }
        }
    }
    if cfg.providers.openai.enabled {
        if let Ok(key) = std::env::var("OPENAI_API_KEY") {
            if let Ok(p) = luna_core::provider::openai_compat::OpenAiCompatProvider::openai(key) {
                let c = &cfg.providers.openai;
                return Some(("openai".into(), std::sync::Arc::new(p), (c.model.clone(), c.max_tokens, c.temperature)));
            }
        }
    }
    if cfg.providers.ollama.enabled {
        if let Ok(p) = luna_core::provider::openai_compat::OpenAiCompatProvider::ollama() {
            let c = &cfg.providers.ollama;
            return Some((
                "ollama".into(),
                std::sync::Arc::new(p),
                (c.model.clone(), c.max_tokens, c.temperature),
            ));
        }
    }
    None
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "luna=info".into()),
        )
        .init();

    install_cli_shim();

    let bus = EventBus::default();
    let (tx, rx) = tokio::sync::mpsc::channel(256);
    let cancel = CancellationToken::new();
    let session_id = uuid::Uuid::new_v4();

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(LunaState {
            bus: bus.clone(),
            commands: tx,
            cancel,
            session_id,
            history: Mutex::new(Vec::new()),
            approvals: Arc::new(Mutex::new(HashMap::new())),
        })
        .setup(move |app| {
            let handle = app.handle().clone();
            spawn_event_forwarder(handle, bus);
            // Drain commands so senders never block; the run path is
            // `luna_run_agent`, this loop just mirrors events/cancel.
            let state: tauri::State<LunaState> = app.state();
            let bus2 = state.bus.clone();
            let cancel2 = state.cancel.clone();
            let sid2 = state.session_id;
            tokio::spawn(async move {
                let mut rx = rx;
                while let Some(cmd) = rx.recv().await {
                    match cmd {
                        Command::UserMessage(text) => {
                            bus2.publish(LunaEvent::new(sid2, None, EventKind::UserMessage(text)));
                        }
                        Command::Approval { request_id, approved } => {
                            bus2.publish(LunaEvent::new(
                                sid2,
                                None,
                                EventKind::ApprovalResolved { request_id, approved },
                            ));
                        }
                        Command::Cancel => cancel2.cancel(),
                    }
                }
            });            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            luna_send_message,
            luna_cancel,
            luna_respond_approval,
            luna_run_agent
        ])
        .run(tauri::generate_context!())
        .expect("error while running Luna");
}
