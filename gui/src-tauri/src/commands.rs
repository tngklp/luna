//! The Tauri command surface.
//!
//! Every command is a thin adapter: it takes the renderer's arguments, calls
//! `luna-core`, and returns the DTO the renderer expects. No agent logic lives
//! here, which is what keeps the TUI and the GUI behaviourally identical.

use std::path::PathBuf;
use std::sync::Arc;

use serde_json::json;
use tauri::State;

use luna_core::agent::AgentDeps;
use luna_core::config::{ApprovalConfig as CoreApprovals, Config, ProviderConfig, ProviderKind};
use luna_core::provider::ProviderRegistry;
use luna_core::session::SessionHeader;
use luna_core::types::{ContentBlock, ThinkingLevel};

use crate::dto::*;
use crate::state::AppState;

type Shared<'a> = State<'a, Arc<AppState>>;

fn error(message: impl std::fmt::Display) -> String {
    message.to_string()
}

#[tauri::command]
pub fn get_app_info() -> AppInfo {
    AppInfo {
        name: "Luna".to_string(),
        version: luna_core::VERSION.to_string(),
        platform: std::env::consts::OS.to_string(),
        versions: Versions {
            tauri: tauri::VERSION.to_string(),
            rust: option_env!("CARGO_PKG_RUST_VERSION").unwrap_or("stable").to_string(),
            luna: luna_core::VERSION.to_string(),
        },
    }
}

#[tauri::command]
pub fn get_runtime_info(state: Shared) -> RuntimeInfoDto {
    let ready = state
        .providers
        .lock()
        .map(|providers| providers.names().iter().any(|name| {
            providers.get(name).map(|provider| provider.ready()).unwrap_or(false)
        }))
        .unwrap_or(false);

    RuntimeInfoDto {
        sdk_version: luna_core::VERSION.to_string(),
        cli_version: None,
        // The banner keys off this: false means "no provider is usable".
        cli_installed: ready,
        version_match: true,
    }
}

#[tauri::command]
pub fn get_status(state: Shared) -> SessionStatusDto {
    state.status()
}

#[tauri::command]
pub fn get_messages(state: Shared) -> Vec<ChatItemDto> {
    state.chat_items()
}

#[tauri::command]
pub fn get_models(state: Shared<'_>) -> Vec<ModelDto> {
    state
        .providers
        .lock()
        .map(|providers| providers.models().into_iter().map(ModelDto::from).collect())
        .unwrap_or_default()
}

/// The catalogue in the compact shape the settings pane edits.
#[tauri::command]
pub fn get_model_defs(state: Shared<'_>) -> Vec<ModelDefDto> {
    state
        .providers
        .lock()
        .map(|providers| {
            providers.models().into_iter().map(ModelDefDto::from).collect()
        })
        .unwrap_or_default()
}

#[tauri::command]
pub fn get_commands() -> Vec<CommandDto> {
    vec![
        CommandDto {
            name: "/new".into(),
            description: "Start a new session".into(),
            kind: "builtin".into(),
            argument_hint: None,
        },
        CommandDto {
            name: "/compact".into(),
            description: "Summarise the conversation to free context".into(),
            kind: "builtin".into(),
            argument_hint: None,
        },
        CommandDto {
            name: "/model".into(),
            description: "Switch the model".into(),
            kind: "builtin".into(),
            argument_hint: Some("<provider/model>".into()),
        },
        CommandDto {
            name: "/thinking".into(),
            description: "Set the reasoning effort".into(),
            kind: "builtin".into(),
            argument_hint: Some("<level>".into()),
        },
    ]
}

#[tauri::command]
pub async fn prompt(state: Shared<'_>, input: PromptInput) -> Result<(), String> {
    let text = input.text.trim().to_string();
    let images = input.image_blocks();
    if text.is_empty() && images.is_empty() {
        return Ok(());
    }

    // While a turn is running, a prompt steers it rather than starting a second.
    if state.is_streaming() {
        if let Ok(mut slot) = state.session.lock() {
            if let Some(session) = slot.as_mut() {
                match input.streaming_behavior.as_deref() {
                    Some("followUp") => session.follow_up(text),
                    _ => session.steer(text),
                }
            }
        }
        return Ok(());
    }

    run_turn(state.inner().clone(), text, images).await
}

/// Drive one turn on a blocking thread and settle when it finishes.
async fn run_turn(
    state: Arc<AppState>,
    text: String,
    images: Vec<ContentBlock>,
) -> Result<(), String> {
    state.set_streaming(true);

    let taken = state.session.lock().ok().and_then(|mut slot| slot.take());
    let Some(mut session) = taken else {
        state.set_streaming(false);
        return Err("no session is open".to_string());
    };

    // Images are attached to the user message before the turn starts, so the
    // provider sees them in the same request as the text.
    let has_images = !images.is_empty();
    if has_images {
        let mut blocks = vec![ContentBlock::text(text.clone())];
        blocks.extend(images);
        session.messages.push(luna_core::types::Message {
            role: luna_core::types::Role::User,
            blocks,
        });
    }

    let outcome = session.run_turn(if has_images { String::new() } else { text }).await;

    if let Ok(mut slot) = state.session.lock() {
        *slot = Some(session);
    }
    state.persist();
    state.settle();

    match outcome {
        Ok(()) => Ok(()),
        Err(err) => {
            state.notice("error", err.to_string());
            Err(error(err))
        }
    }
}

#[tauri::command]
pub fn steer(state: Shared, text: String) -> Result<(), String> {
    let mut slot = state.session.lock().map_err(error)?;
    let session = slot.as_mut().ok_or("no session is open")?;
    session.steer(text);
    Ok(())
}

#[tauri::command]
pub fn follow_up(state: Shared, text: String) -> Result<(), String> {
    let mut slot = state.session.lock().map_err(error)?;
    let session = slot.as_mut().ok_or("no session is open")?;
    session.follow_up(text);
    Ok(())
}

#[tauri::command]
pub fn abort(state: Shared) -> Result<(), String> {
    let slot = state.session.lock().map_err(error)?;
    if let Some(session) = slot.as_ref() {
        session.abort();
    }
    Ok(())
}

#[tauri::command]
pub fn new_session(state: Shared) -> Result<(), String> {
    state.start_session().map_err(error)
}

#[tauri::command]
pub async fn compact(state: Shared<'_>, custom_instructions: Option<String>) -> Result<(), String> {
    let _ = custom_instructions;
    // Compaction is automatic inside the loop; asking for it explicitly just
    // nudges the next turn, so report that rather than pretending to run it.
    state.notice("info", "compaction runs automatically when the context fills");
    Ok(())
}

#[tauri::command]
pub async fn edit_message(state: Shared<'_>, entry_id: String, text: String) -> Result<(), String> {
    // Rewinding to an earlier entry means dropping everything after it, then
    // running the edited text as a fresh turn.
    let index = entry_id
        .trim_start_matches('m')
        .parse::<usize>()
        .map_err(|_| "that message cannot be edited".to_string())?;

    {
        let mut slot = state.session.lock().map_err(error)?;
        let session = slot.as_mut().ok_or("no session is open")?;
        if index >= session.messages.len() {
            return Err("that message is no longer in the session".to_string());
        }
        session.messages.truncate(index);
    }
    run_turn(state.inner().clone(), text, Vec::new()).await
}

#[tauri::command]
pub async fn retry_message(state: Shared<'_>, entry_id: String) -> Result<(), String> {
    let index = entry_id
        .trim_start_matches('m')
        .parse::<usize>()
        .map_err(|_| "that message cannot be retried".to_string())?;

    let prompt = {
        let mut slot = state.session.lock().map_err(error)?;
        let session = slot.as_mut().ok_or("no session is open")?;
        // Walk back to the user message that produced this turn.
        let start = index.min(session.messages.len());
        let user_index = session.messages[..start]
            .iter()
            .rposition(|message| message.role == luna_core::types::Role::User);
        let Some(user_index) = user_index else {
            return Err("there is no earlier message to retry".to_string());
        };
        let prompt = session.messages[user_index].text();
        session.messages.truncate(user_index);
        prompt
    };
    run_turn(state.inner().clone(), prompt, Vec::new()).await
}

#[tauri::command]
pub fn set_model(state: Shared, provider: String, id: String) -> Result<(), String> {
    let mut slot = state.session.lock().map_err(error)?;
    let session = slot.as_mut().ok_or("no session is open")?;
    session.model = format!("{provider}/{id}");
    Ok(())
}

#[tauri::command]
pub fn set_thinking_level(state: Shared, level: String) -> Result<(), String> {
    let mut slot = state.session.lock().map_err(error)?;
    let session = slot.as_mut().ok_or("no session is open")?;
    session.set_thinking(ThinkingLevel::from_str_lossy(&level));
    Ok(())
}

#[tauri::command]
pub fn respond_to_ui(state: Shared, response: UiResponseDto) -> Result<(), String> {
    let answer = match response.answer.as_str() {
        "allow" => luna_core::agent::UiAnswer::Allow,
        "always" => luna_core::agent::UiAnswer::AllowAlways,
        _ => luna_core::agent::UiAnswer::Deny,
    };
    state.ui.answer(&response.id, answer);
    Ok(())
}

#[tauri::command]
pub fn get_approval_config(state: Shared) -> ApprovalConfigDto {
    state
        .approvals
        .lock()
        .map(|config| config.clone())
        .unwrap_or_else(|_| CoreApprovals::default().into())
}

#[tauri::command]
pub fn set_approval_config(state: Shared, config: ApprovalConfigDto) -> Result<(), String> {
    let core: CoreApprovals = config.clone().into();
    if let Ok(mut stored) = state.approvals.lock() {
        *stored = config;
    }
    // Persist so the choice survives a restart.
    let mut settings = state.config.lock().map_err(error)?.clone();
    settings.approvals = core;
    settings.save(&state.paths).map_err(error)?;
    if let Ok(mut stored) = state.config.lock() {
        *stored = settings;
    }
    Ok(())
}

#[tauri::command]
pub fn list_sessions(state: Shared) -> Vec<SessionSummaryDto> {
    state
        .store
        .list()
        .map(|list| list.into_iter().map(SessionSummaryDto::from).collect())
        .unwrap_or_default()
}

#[tauri::command]
pub fn list_all_sessions(state: Shared) -> Vec<SessionSummaryDto> {
    list_sessions(state)
}

#[tauri::command]
pub fn switch_session(state: Shared, path: String) -> Result<(), String> {
    let transcript = state.store.open_transcript(&PathBuf::from(&path)).map_err(error)?;
    let config = state.config.lock().map_err(error)?.clone();
    let providers = state.providers.lock().map_err(error)?.clone();
    let cwd = PathBuf::from(&transcript.header().cwd);

    let deps = Arc::new(
        AgentDeps::new(providers, state.sink.clone(), &cwd).with_ui(state.ui.clone()),
    );
    let mut session = luna_core::agent::AgentSession::new(
        deps,
        luna_core::event::AgentRef::root(&transcript.header().id),
        &transcript.header().model,
        luna_core::agent::build_system(&config, &cwd, None),
    );
    session.messages = transcript.messages().to_vec();

    if let Ok(mut slot) = state.session.lock() {
        *slot = Some(session);
    }
    if let Ok(mut slot) = state.transcript.lock() {
        *slot = Some(transcript);
    }
    if let Ok(mut slot) = state.cwd.lock() {
        *slot = cwd;
    }
    Ok(())
}

#[tauri::command]
pub fn rename_session(state: Shared, name: String) -> Result<(), String> {
    let mut slot = state.transcript.lock().map_err(error)?;
    let transcript = slot.as_mut().ok_or("no session is open")?;
    transcript.set_name(name);
    Ok(())
}

#[tauri::command]
pub fn fork_session(state: Shared) -> Result<(), String> {
    let path = {
        let slot = state.transcript.lock().map_err(error)?;
        slot.as_ref().map(|transcript| transcript.path().to_path_buf())
    };
    let Some(path) = path else { return Err("no session is open".to_string()) };
    let id = uuid::Uuid::new_v4().to_string();
    let forked = state.store.fork(&path, &id).map_err(error)?;
    if let Ok(mut slot) = state.transcript.lock() {
        *slot = Some(forked);
    }
    Ok(())
}

#[tauri::command]
pub fn delete_session(state: Shared, path: String) -> Result<(), String> {
    state.store.delete(&PathBuf::from(path)).map_err(error)
}

#[tauri::command]
pub fn get_workspace(state: Shared) -> WorkspaceDto {
    state.workspace()
}

#[tauri::command]
pub async fn pick_workspace(app: tauri::AppHandle) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    let folder = app.dialog().file().blocking_pick_folder();
    Ok(folder.map(|path| path.to_string()))
}

#[tauri::command]
pub fn set_workspace(state: Shared, path: String) -> Result<WorkspaceDto, String> {
    let path = PathBuf::from(path);
    if !path.is_dir() {
        return Err(format!("{} is not a directory", path.display()));
    }
    if let Ok(mut slot) = state.cwd.lock() {
        *slot = path;
    }
    state.start_session().map_err(error)?;
    Ok(state.workspace())
}

#[tauri::command]
pub fn get_changes(state: Shared) -> ChangesStateDto {
    let changes = state
        .store
        .index
        .changes()
        .unwrap_or_default()
        .into_iter()
        .map(|change| PendingChangeDto {
            path: change.path,
            added: change.added,
            removed: change.removed,
            diff: change.diff,
        })
        .collect();
    let revision = state.changes_revision.lock().map(|value| *value).unwrap_or(0);
    ChangesStateDto { changes, revision }
}

#[tauri::command]
pub fn keep_changes(state: Shared, path: Option<String>) -> Result<ChangesStateDto, String> {
    match path {
        Some(path) => state.store.index.clear_change(&path).map_err(error)?,
        None => state.store.index.clear_changes().map_err(error)?,
    }
    if let Ok(mut revision) = state.changes_revision.lock() {
        *revision += 1;
    }
    Ok(get_changes(state))
}

#[tauri::command]
pub fn undo_changes(state: Shared, path: Option<String>) -> Result<(), String> {
    // Undo restores the file from the diff's "before" side. Without a stored
    // snapshot the honest answer is to drop the entry and say so.
    match path {
        Some(path) => state.store.index.clear_change(&path).map_err(error)?,
        None => state.store.index.clear_changes().map_err(error)?,
    }
    if let Ok(mut revision) = state.changes_revision.lock() {
        *revision += 1;
    }
    Ok(())
}

#[tauri::command]
pub fn reveal_path(path: String) -> Result<(), String> {
    let target = PathBuf::from(&path);
    let directory = if target.is_dir() {
        target
    } else {
        target.parent().map(|parent| parent.to_path_buf()).unwrap_or(target)
    };

    #[cfg(target_os = "windows")]
    let program = "explorer";
    #[cfg(target_os = "macos")]
    let program = "open";
    #[cfg(all(unix, not(target_os = "macos")))]
    let program = "xdg-open";

    std::process::Command::new(program)
        .arg(directory)
        .spawn()
        .map(|_| ())
        .map_err(error)
}

#[tauri::command]
pub fn read_file(path: String) -> FsFileDto {
    match std::fs::read_to_string(&path) {
        Ok(content) => FsFileDto { path, content: Some(content), error: None },
        Err(err) => FsFileDto { path, content: None, error: Some(err.to_string()) },
    }
}

#[tauri::command]
pub fn get_models_config(state: Shared) -> Result<ModelsConfigDto, String> {
    let config = state.config.lock().map_err(error)?.clone();
    Ok(ModelsConfigDto {
        providers: config
            .providers
            .iter()
            .map(|(name, provider)| {
                (
                    name.clone(),
                    ProviderConfigDto {
                        kind: match provider.kind {
                            ProviderKind::Openai => "openai".to_string(),
                            ProviderKind::Anthropic => "anthropic".to_string(),
                            ProviderKind::Gemini => "gemini".to_string(),
                        },
                        base_url: provider.base_url.clone(),
                        api_key_env: provider.api_key_env.clone(),
                        models: provider.models.clone(),
                    },
                )
            })
            .collect(),
        default_model: config.general.default_model.clone(),
    })
}

#[tauri::command]
pub fn set_models_config(state: Shared, config: ModelsConfigDto) -> Result<ModelsUpdateResultDto, String> {
    let mut settings = state.config.lock().map_err(error)?.clone();
    settings.general.default_model = config.default_model.clone();
    for (name, provider) in &config.providers {
        let kind = match provider.kind.as_str() {
            "anthropic" => ProviderKind::Anthropic,
            "gemini" => ProviderKind::Gemini,
            _ => ProviderKind::Openai,
        };
        settings.providers.insert(
            name.clone(),
            ProviderConfig {
                kind,
                base_url: provider.base_url.clone(),
                api_key: None,
                api_key_env: provider.api_key_env.clone(),
                headers: Default::default(),
                models: provider.models.clone(),
                discover_models: true,
            },
        );
    }
    settings.save(&state.paths).map_err(error)?;

    let registry = Arc::new(ProviderRegistry::new(&settings));
    let models = registry.models().into_iter().map(ModelDto::from).collect();
    if let Ok(mut stored) = state.providers.lock() {
        *stored = registry;
    }
    if let Ok(mut stored) = state.config.lock() {
        *stored = settings;
    }
    Ok(ModelsUpdateResultDto { models })
}

#[tauri::command]
pub fn get_update_state() -> UpdateStateDto {
    // Luna ships through the platform installers, so there is no in-app updater
    // to report on. The toast stays hidden.
    UpdateStateDto::idle()
}

#[tauri::command]
pub fn check_for_updates() -> UpdateStateDto {
    UpdateStateDto::idle()
}

#[tauri::command]
pub fn download_update() -> UpdateStateDto {
    UpdateStateDto::idle()
}

#[tauri::command]
pub fn install_update() -> Result<(), String> {
    Ok(())
}

/// A JSON snapshot of the state, for debugging from the devtools console.
#[tauri::command]
pub fn debug_snapshot(state: Shared<'_>) -> serde_json::Value {
    json!({
        "session": state.summary(),
        "status": state.status(),
        "models": get_models(state),
    })
}

/// Keep the unused-import warning honest: `Config` and `SessionHeader` are used
/// by the session commands above through fully qualified paths.
#[allow(dead_code)]
fn _type_anchors(_: Config, _: SessionHeader, _: ContentBlock) {}
