//! The application state the commands operate on.
//!
//! One `AppState` owns the live session, the provider registry and the session
//! store. Commands take it from Tauri's managed state, so there is exactly one
//! agent per window and no global mutable state.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use luna_core::agent::{build_system, AgentDeps, AgentSession, UiAnswer, UiBridge, UiRequest};
use luna_core::config::{Config, Paths};
use luna_core::event::{AgentEvent, AgentRef, Emitted, EventSink, SinkRef};
use luna_core::provider::ProviderRegistry;
use luna_core::session::{SessionHeader, SessionStore, Transcript};
use luna_core::types::ThinkingLevel;
use tauri::{AppHandle, Emitter};

use crate::dto::{ApprovalConfigDto, ChatItemDto, SessionStatusDto, WorkspaceDto};

pub const EVENT_CHANNEL: &str = "luna://agent-event";
pub const UI_REQUEST_CHANNEL: &str = "luna://ui-request";

/// Forwards core events to the webview.
pub struct TauriSink {
    app: AppHandle,
}

impl EventSink for TauriSink {
    fn emit(&self, event: Emitted) {
        // A closed window makes this fail; there is nothing useful to do about
        // it, and the agent must not block on a UI that is gone.
        let _ = self.app.emit(EVENT_CHANNEL, event.to_value());
    }
}

/// Asks the user for approval through the webview.
///
/// The request is emitted as an event and the answer arrives later through
/// `respond_to_ui`, so the agent loop suspends on a oneshot rather than polling.
pub struct TauriUi {
    app: AppHandle,
    pending: Arc<Mutex<std::collections::HashMap<String, tokio::sync::oneshot::Sender<UiAnswer>>>>,
}

#[async_trait::async_trait]
impl UiBridge for TauriUi {
    async fn ask(&self, request: UiRequest) -> UiAnswer {
        let (reply, answer) = tokio::sync::oneshot::channel();
        let id = uuid::Uuid::new_v4().to_string();
        let payload = match request {
            UiRequest::Approval { tool, title, detail } => {
                crate::dto::UiRequestDto::Approval { id: id.clone(), tool, title, detail }
            }
        };
        if let Ok(mut pending) = self.pending.lock() {
            pending.insert(id.clone(), reply);
        }
        if self.app.emit(UI_REQUEST_CHANNEL, payload).is_err() {
            return UiAnswer::Deny;
        }
        answer.await.unwrap_or(UiAnswer::Deny)
    }
}

impl TauriUi {
    pub fn answer(&self, id: &str, answer: UiAnswer) {
        if let Ok(mut pending) = self.pending.lock() {
            if let Some(sender) = pending.remove(id) {
                let _ = sender.send(answer);
            }
        }
    }
}

/// Everything one window needs.
pub struct AppState {
    pub config: Mutex<Config>,
    pub paths: Paths,
    pub providers: Mutex<Arc<ProviderRegistry>>,
    pub store: SessionStore,
    pub session: Mutex<Option<AgentSession>>,
    pub transcript: Mutex<Option<Transcript>>,
    pub cwd: Mutex<PathBuf>,
    pub sink: SinkRef,
    pub ui: Arc<TauriUi>,
    pub streaming: AtomicBool,
    pub changes_revision: Mutex<u64>,
    pub approvals: Mutex<ApprovalConfigDto>,
}

impl AppState {
    pub fn new(app: AppHandle) -> anyhow::Result<Arc<AppState>> {
        let paths = Paths::resolve();
        paths.ensure()?;
        let config = Config::load_or_default(&paths);
        let providers = Arc::new(ProviderRegistry::new(&config));
        let store = SessionStore::open(&paths.data_dir)?;
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

        let sink: SinkRef = Arc::new(TauriSink { app: app.clone() });
        let ui = Arc::new(TauriUi {
            app,
            pending: Arc::new(Mutex::new(std::collections::HashMap::new())),
        });

        let state = Arc::new(AppState {
            approvals: Mutex::new(config.approvals.into()),
            config: Mutex::new(config),
            paths,
            providers: Mutex::new(providers),
            store,
            session: Mutex::new(None),
            transcript: Mutex::new(None),
            cwd: Mutex::new(cwd),
            sink,
            ui,
            streaming: AtomicBool::new(false),
            changes_revision: Mutex::new(0),
        });

        state.start_session()?;
        Ok(state)
    }

    pub fn is_streaming(&self) -> bool {
        self.streaming.load(Ordering::Relaxed)
    }

    pub fn set_streaming(&self, value: bool) {
        self.streaming.store(value, Ordering::Relaxed);
    }

    /// Build a fresh session and agent for the current working directory.
    pub fn start_session(&self) -> anyhow::Result<()> {
        let cwd = self.cwd.lock().map(|cwd| cwd.clone()).unwrap_or_else(|_| PathBuf::from("."));
        let config = self.config.lock().map(|config| config.clone()).unwrap_or_default();
        let providers = self
            .providers
            .lock()
            .map(|providers| providers.clone())
            .unwrap_or_else(|_| Arc::new(ProviderRegistry::new(&config)));

        let id = uuid::Uuid::new_v4().to_string();
        let header = SessionHeader::new(&id, cwd.display().to_string(), &config.general.default_model);
        let transcript = self.store.create(&header)?;
        let system = build_system(&config, &cwd, None);

        let deps = Arc::new(
            AgentDeps::new(providers, self.sink.clone(), &cwd).with_ui(self.ui.clone()),
        );
        let session = AgentSession::new(
            deps,
            AgentRef::root(&id),
            &config.general.default_model,
            system,
        );

        if let Ok(mut slot) = self.session.lock() {
            *slot = Some(session);
        }
        if let Ok(mut slot) = self.transcript.lock() {
            *slot = Some(transcript);
        }
        Ok(())
    }

    /// The current session's messages, as the renderer wants them.
    pub fn chat_items(&self) -> Vec<ChatItemDto> {
        let Ok(slot) = self.session.lock() else { return Vec::new() };
        match slot.as_ref() {
            Some(session) => crate::dto::to_chat_items(&session.messages),
            None => Vec::new(),
        }
    }

    pub fn status(&self) -> SessionStatusDto {
        let cwd = self.cwd.lock().map(|cwd| cwd.display().to_string()).unwrap_or_default();
        let Ok(slot) = self.session.lock() else {
            return empty_status(cwd);
        };
        let Some(session) = slot.as_ref() else {
            return empty_status(cwd);
        };

        let info = session.model_info();
        let used = luna_core::tokens::estimate_messages(&session.messages);
        let percent = (used as f32 / info.context_window.max(1) as f32) * 100.0;
        let session_file = self
            .transcript
            .lock()
            .ok()
            .and_then(|transcript| transcript.as_ref().map(|t| t.path().display().to_string()));

        SessionStatusDto {
            cwd,
            model: Some(info.clone().into()),
            thinking_level: session.thinking.as_str().to_string(),
            available_thinking_levels: crate::dto::thinking_levels(),
            supports_thinking: info.reasoning,
            is_streaming: self.is_streaming(),
            is_compacting: false,
            is_retrying: false,
            session_id: session.agent.agent_id.clone(),
            session_file,
            session_name: None,
            message_count: session.messages.len(),
            pending_message_count: session.queues.pending(),
            steering: session.queues.steering(),
            follow_up: session.queues.follow_up(),
            context_usage: Some(crate::dto::ContextUsageDto {
                tokens: Some(used),
                context_window: info.context_window,
                percent: Some(percent),
            }),
            cost: session.stats.cost,
            tokens: session.stats.usage.into(),
        }
    }

    pub fn workspace(&self) -> WorkspaceDto {
        let path = self.cwd.lock().map(|cwd| cwd.clone()).unwrap_or_default();
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| path.display().to_string());
        WorkspaceDto { path: path.display().to_string(), name }
    }

    /// Persist anything new in the session to its transcript.
    pub fn persist(&self) {
        let Ok(session_slot) = self.session.lock() else { return };
        let Some(session) = session_slot.as_ref() else { return };
        let Ok(mut transcript_slot) = self.transcript.lock() else { return };
        let Some(transcript) = transcript_slot.as_mut() else { return };

        let already = transcript.messages().len();
        for message in session.messages.iter().skip(already) {
            if message.role == luna_core::types::Role::System {
                continue;
            }
            if let Err(err) = transcript.append_message(message) {
                tracing::warn!("could not save the session: {err}");
                return;
            }
        }
    }

    /// Emit a notice to the webview.
    pub fn notice(&self, level: &str, message: impl Into<String>) {
        self.sink.emit(Emitted::notice(level, message));
    }

    /// Emit a settled event so the UI clears its streaming state.
    pub fn settle(&self) {
        self.set_streaming(false);
        let agent = self
            .session
            .lock()
            .ok()
            .and_then(|slot| slot.as_ref().map(|session| session.agent.clone()))
            .unwrap_or_else(|| AgentRef::root("root"));
        self.sink.emit(Emitted::new(agent, AgentEvent::AgentSettled {}));
    }

    pub fn thinking(&self) -> ThinkingLevel {
        self.session
            .lock()
            .ok()
            .and_then(|slot| slot.as_ref().map(|session| session.thinking))
            .unwrap_or_default()
    }

    /// The model the session is running on, as `provider/model`.
    pub fn model(&self) -> String {
        self.session
            .lock()
            .ok()
            .and_then(|slot| slot.as_ref().map(|session| session.model.clone()))
            .unwrap_or_default()
    }

    /// A one-line summary of the live session, for the debug snapshot.
    pub fn summary(&self) -> serde_json::Value {
        serde_json::json!({
            "model": self.model(),
            "thinking": self.thinking().as_str(),
            "streaming": self.is_streaming(),
            "workspace": self.workspace(),
        })
    }
}

fn empty_status(cwd: String) -> SessionStatusDto {
    SessionStatusDto {
        cwd,
        model: None,
        thinking_level: ThinkingLevel::Medium.as_str().to_string(),
        available_thinking_levels: crate::dto::thinking_levels(),
        supports_thinking: false,
        is_streaming: false,
        is_compacting: false,
        is_retrying: false,
        session_id: String::new(),
        session_file: None,
        session_name: None,
        message_count: 0,
        pending_message_count: 0,
        steering: Vec::new(),
        follow_up: Vec::new(),
        context_usage: None,
        cost: 0.0,
        tokens: luna_core::types::Usage::default().into(),
    }
}
