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
    /// SQLite store for session persistence.
    pub store: Arc<luna_core::storage::Store>,
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
/// Every payload carries `agent_id` (empty string = orchestrator/global).
#[derive(serde::Serialize, Clone)]
struct UiEvent {
    agent_id: String,
    #[serde(flatten)]
    kind: UiEventKind,
}

#[derive(serde::Serialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
enum UiEventKind {
    RunStarted,
    UserMessage { text: String },
    AssistantMessage { text: String },
    TokenDelta { text: String },
    ToolCallRequested { call_id: String, name: String, arguments: serde_json::Value },
    ToolCallFinished { call_id: String, name: String, outcome: String, is_error: bool },
    ApprovalRequested { request_id: String, tool_name: String, arguments: serde_json::Value },
    ApprovalResolved { request_id: String, approved: bool },
    AgentSpawned { agent_id: String, parent: Option<String>, spec_name: String },
    OrchestratorTransition { description: String },
    RunFinished { summary: Option<String>, error: Option<String> },
}

impl UiEvent {
    fn from_core(ev: &LunaEvent) -> Option<Self> {
        let agent = ev.agent_id.clone().unwrap_or_default();
        let kind = match &ev.kind {
            EventKind::RunStarted => UiEventKind::RunStarted,
            EventKind::UserMessage(text) => UiEventKind::UserMessage { text: text.clone() },
            EventKind::AssistantMessage(text) => UiEventKind::AssistantMessage { text: text.clone() },
            EventKind::TokenDelta(text) => UiEventKind::TokenDelta { text: text.clone() },
            EventKind::ToolCallRequested { call_id, name, arguments } => UiEventKind::ToolCallRequested {
                call_id: call_id.clone(),
                name: name.clone(),
                arguments: arguments.clone(),
            },
            EventKind::ToolCallFinished { call_id, name, outcome } => UiEventKind::ToolCallFinished {
                call_id: call_id.clone(),
                name: name.clone(),
                outcome: outcome.content.clone(),
                is_error: outcome.is_error,
            },
            EventKind::ApprovalRequested { request_id, tool_name, arguments, .. } => UiEventKind::ApprovalRequested {
                request_id: request_id.to_string(),
                tool_name: tool_name.clone(),
                arguments: arguments.clone(),
            },
            EventKind::ApprovalResolved { request_id, approved } => UiEventKind::ApprovalResolved {
                request_id: request_id.to_string(),
                approved: *approved,
            },
            EventKind::AgentSpawned { agent_id, parent, spec_name } => UiEventKind::AgentSpawned {
                agent_id: agent_id.clone(),
                parent: parent.clone(),
                spec_name: spec_name.clone(),
            },
            EventKind::OrchestratorTransition(d) => {
                UiEventKind::OrchestratorTransition { description: d.clone() }
            }
            EventKind::RunFinished { summary, error } => UiEventKind::RunFinished {
                summary: summary.clone(),
                error: error.clone(),
            },
        };
        Some(Self { agent_id: agent, kind })
    }
}

/// Background task: forward core events to the webview.
fn spawn_event_forwarder(app: AppHandle, bus: EventBus) {
    tokio::spawn(async move {
        let mut rx = bus.subscribe();
        loop {
            match rx.recv().await {
                Ok(ev) => {
                    if let Some(payload) = UiEvent::from_core(&ev) {
                        // Emit on the global channel; the payload carries
                        // agent_id so the UI can route it.
                        let _ = app.emit("luna://event", payload);
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

/// The shared core stack built once per run command.
struct CoreStack {
    provider: Arc<dyn luna_core::provider::Provider>,
    registry: AgentRegistry,
    tools: ToolRegistry,
    default_spec: AgentSpec,
}

/// Build the provider + registry + tools stack shared by all run commands.
fn build_stack(cfg: &luna_core::config::Config) -> Result<CoreStack, String> {
    let (_key, provider, settings) = pick_provider(cfg).ok_or_else(|| {
        "no provider enabled: set ANTHROPIC_API_KEY / OPENAI_API_KEY or enable ollama in config.toml".to_string()
    })?;

    let mut registry = AgentRegistry::new();
    registry.register_provider(
        &_key,
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
            provider_key: _key.clone(),
            model: settings.0.clone(),
            system: Some("You are Luna, a helpful coding agent.".into()),
            tools: vec![],
            max_iterations: 25,
            max_tokens: settings.1,
            temperature: settings.2,
        }
    };

    let mut tools = ToolRegistry::new();
    builtin::register_all(&mut tools);
    if let Ok(web) = luna_core::tool::web::WebFetch::new() {
        tools.register(Arc::new(web));
    }

    Ok(CoreStack { provider, registry, tools, default_spec: spec })
}

fn build_ctx(state: &LunaState, provider: Arc<dyn luna_core::provider::Provider>, tools: ToolRegistry, cfg: &luna_core::config::Config) -> AgentContext {
    AgentContext {
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
    }
}

/// Run one agent turn end-to-end, streaming events to the webview.
#[tauri::command]
async fn luna_run_agent(
    _app: AppHandle,
    state: State<'_, LunaState>,
    text: String,
) -> Result<String, String> {
    let cfg = luna_core::config::Config::load().map_err(|e| e.to_string())?;
    let stack = build_stack(&cfg)?;
    let ctx = build_ctx(&state, stack.provider, stack.tools, &cfg);

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
    let runner = AgentRunner::new(stack.default_spec);
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

/// A multi-agent task request from the webview.
#[derive(serde::Deserialize)]
struct PlanRequest {
    /// "single" | "fanout" | "supervised"
    mode: String,
    /// For single: the agent name. For supervised: the supervisor name.
    agent: Option<String>,
    /// (agent, task) pairs for fanout/supervised.
    workers: Option<Vec<(String, String)>>,
}

/// Execute a multi-agent plan, streaming events to the webview.
#[tauri::command]
async fn luna_run_plan(
    _app: AppHandle,
    state: State<'_, LunaState>,
    request: PlanRequest,
) -> Result<PlanSummary, String> {
    let cfg = luna_core::config::Config::load().map_err(|e| e.to_string())?;
    let stack = build_stack(&cfg)?;
    let ctx = build_ctx(&state, stack.provider, stack.tools, &cfg);

    let plan = match request.mode.as_str() {
        "single" => {
            let agent = request.agent.ok_or("single plan requires `agent`")?;
            let task = request
                .workers
                .and_then(|w| w.into_iter().next())
                .map(|(_, t)| t)
                .ok_or("single plan requires one worker task")?;
            luna_core::orchestrator::runner::TaskPlan::Single { agent, task }
        }
        "fanout" => {
            let workers = request.workers.ok_or("fanout plan requires `workers`")?;
            luna_core::orchestrator::runner::TaskPlan::FanOut { agents: workers, join_prompt: None }
        }
        "supervised" => {
            let supervisor = request.agent.ok_or("supervised plan requires `agent` (supervisor)")?;
            let workers = request.workers.ok_or("supervised plan requires `workers`")?;
            luna_core::orchestrator::runner::TaskPlan::Supervised { supervisor, workers }
        }
        other => return Err(format!("unknown plan mode `{other}`")),
    };

    let orch = luna_core::orchestrator::runner::Orchestrator::new(stack.registry, state.bus.clone());
    let cancel = state.cancel.clone();
    let result = orch
        .execute(plan, &ctx, Arc::new(luna_core::orchestrator::agent_loop::AutoApprove), cancel)
        .await
        .map_err(|e| e.to_string())?;

    Ok(PlanSummary {
        outputs: result.outputs,
        errors: result.errors,
    })
}

#[derive(serde::Serialize)]
struct PlanSummary {
    outputs: std::collections::HashMap<String, String>,
    errors: std::collections::HashMap<String, String>,
}

/// List persisted sessions, most recent first.
#[tauri::command]
async fn luna_list_sessions(
    state: State<'_, LunaState>,
    limit: Option<u32>,
) -> Result<Vec<luna_core::storage::SessionSummary>, String> {
    state
        .store
        .list_sessions(limit.unwrap_or(20))
        .map_err(|e| e.to_string())
}

/// Load a persisted session's events as UI events (for resume).
#[tauri::command]
async fn luna_load_session(
    state: State<'_, LunaState>,
    session_id: String,
) -> Result<Vec<UiEvent>, String> {
    let events = state.store.load_events(&session_id).map_err(|e| e.to_string())?;
    Ok(events.iter().filter_map(UiEvent::from_core).collect())
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

/// Resolved provider settings tuple: (model, max_tokens, temperature).
type ProviderSettingsTuple = (String, u32, f32);

fn pick_provider(
    cfg: &luna_core::config::Config,
) -> Option<(String, std::sync::Arc<dyn luna_core::provider::Provider>, ProviderSettingsTuple)> {
    // Env-var API keys take precedence in provider selection order.
    if cfg.providers.anthropic.enabled
        && let Ok(key) = std::env::var("ANTHROPIC_API_KEY")
            && let Ok(p) = luna_core::provider::anthropic::AnthropicProvider::new(key) {
                let c = &cfg.providers.anthropic;
                return Some(("anthropic".into(), std::sync::Arc::new(p), (c.model.clone(), c.max_tokens, c.temperature)));
            }
    if cfg.providers.openai.enabled
        && let Ok(key) = std::env::var("OPENAI_API_KEY")
            && let Ok(p) = luna_core::provider::openai_compat::OpenAiCompatProvider::openai(key) {
                let c = &cfg.providers.openai;
                return Some(("openai".into(), std::sync::Arc::new(p), (c.model.clone(), c.max_tokens, c.temperature)));
            }
    if cfg.providers.ollama.enabled
        && let Ok(p) = luna_core::provider::openai_compat::OpenAiCompatProvider::ollama() {
            let c = &cfg.providers.ollama;
            return Some((
                "ollama".into(),
                std::sync::Arc::new(p),
                (c.model.clone(), c.max_tokens, c.temperature),
            ));
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

    // SQLite persistence: data dir from config, store attached to the bus.
    let store = {
        let data_dir = luna_core::config::Config::load()
            .map(|c| c.storage.data_dir)
            .unwrap_or_else(|_| std::path::PathBuf::from(".luna"));
        let path = data_dir.join("sessions.db");
        match luna_core::storage::Store::open(&path) {
            Ok(s) => Arc::new(s),
            Err(e) => {
                tracing::warn!(error = %e, "opening session store failed; persistence disabled");
                Arc::new(luna_core::storage::Store::open_in_memory().expect("in-memory store"))
            }
        }
    };
    luna_core::storage::attach_store(&bus, store.clone());

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(LunaState {
            bus: bus.clone(),
            commands: tx,
            cancel,
            session_id,
            history: Mutex::new(Vec::new()),
            approvals: Arc::new(Mutex::new(HashMap::new())),
            store,
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
            luna_run_agent,
            luna_run_plan,
            luna_list_sessions,
            luna_load_session
        ])
        .run(tauri::generate_context!())
        .expect("error while running Luna");
}
