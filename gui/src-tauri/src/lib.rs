//! Luna's desktop application.
//!
//! The window is a Tauri webview running the ported piui chat surface; the agent
//! engine is `luna-core`, linked in-process so a streamed token costs one event
//! hop rather than a subprocess round trip.

mod commands;
mod dto;
mod state;
mod terminal;
mod update;

use std::sync::Arc;

use tauri::Manager;

#[global_allocator]
static ALLOC: mimalloc::MiMalloc = mimalloc::MiMalloc;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .with_writer(std::io::sink)
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let handle = app.handle().clone();
            let state = state::AppState::new(handle)?;
            app.manage(state);
            app.manage(terminal::TerminalState::new());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_app_info,
            commands::get_runtime_info,
            commands::get_status,
            commands::get_messages,
            commands::get_models,
            commands::get_model_defs,
            commands::get_commands,
            commands::prompt,
            commands::steer,
            commands::follow_up,
            commands::abort,
            commands::new_session,
            commands::compact,
            commands::edit_message,
            commands::retry_message,
            commands::set_model,
            commands::set_thinking_level,
            commands::respond_to_ui,
            commands::get_approval_config,
            commands::set_approval_config,
            commands::list_sessions,
            commands::list_all_sessions,
            commands::search_sessions,
            commands::switch_session,
            commands::rename_session,
            commands::fork_session,
            commands::delete_session,
            commands::get_workspace,
            commands::pick_workspace,
            commands::set_workspace,
            commands::get_changes,
            commands::keep_changes,
            commands::undo_changes,
            commands::reveal_path,
            commands::read_file,
            commands::get_models_config,
            commands::set_models_config,
            commands::get_update_state,
            commands::check_for_updates,
            commands::download_update,
            commands::install_update,
            commands::update_url,
            commands::debug_snapshot,
            commands::get_extensions,
            commands::terminal_start,
            commands::terminal_write,
            commands::terminal_stop,
            terminal::terminal_running,
        ])
        .run(tauri::generate_context!())
        .expect("Luna could not start");
}

/// The library entry point, so the binary stays a one-liner.
pub fn main() {
    run();
}

/// Silence the unused-import warning for `Arc` on platforms where the mobile
/// entry point is compiled out.
#[allow(dead_code)]
fn _anchor(_: Arc<()>) {}
