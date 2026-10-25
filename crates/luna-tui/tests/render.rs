//! Rendering tests.
//!
//! These drive the real `draw` function against a `TestBackend`, so a layout
//! regression fails here rather than in a terminal. They are the only way to
//! assert on what the TUI actually puts on screen.

use luna_core::config::Config;
use luna_core::provider::ProviderRegistry;
use luna_core::session::SessionStore;
use luna_core::types::ThinkingLevel;
use ratatui::backend::TestBackend;
use ratatui::Terminal;

use luna_tui::app::{App, Line};
use luna_tui::commands::Palette;

/// Build an app with no session, for rendering only.
///
/// Each call gets its own data directory: the session index is a SQLite file,
/// and two tests sharing one would contend for the write lock.
fn app() -> App {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("luna-render-{}-{unique}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let config = Config::default();
    let providers = std::sync::Arc::new(ProviderRegistry::new(&config));
    let store = SessionStore::open(&dir).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    App::new(
        config,
        providers,
        store,
        std::sync::Arc::new(luna_core::event::NullSink),
        runtime.handle().clone(),
        "/home/dev/.luna/config.toml".to_string(),
    )
    .unwrap()
}

fn render(app: &App, width: u16, height: u16) -> String {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| luna_tui::ui::draw(frame, app)).unwrap();
    let buffer = terminal.backend().buffer().clone();
    let mut out = String::new();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            out.push_str(buffer[(x, y)].symbol());
        }
        out.push('\n');
    }
    out
}

#[test]
fn the_transcript_starts_empty() {
    let app = app();
    assert!(app.lines.is_empty(), "the transcript should have no lines on launch");
}

#[test]
fn the_empty_state_points_at_commands() {
    let screen = render(&app(), 100, 24);
    assert!(screen.contains("Ask Luna to build something."), "{screen}");
    assert!(screen.contains("for commands"), "{screen}");
}

#[test]
fn the_header_shows_the_model_and_config_source() {
    let screen = render(&app(), 120, 24);
    assert!(screen.contains("luna"), "{screen}");
    assert!(screen.contains("anthropic/claude-sonnet-4-5"), "{screen}");
    assert!(screen.contains(".luna/config.toml"), "{screen}");
}

#[test]
fn the_status_bar_shows_the_run_state() {
    let screen = render(&app(), 120, 24);
    assert!(screen.contains("idle"), "{screen}");
    assert!(screen.contains("thinking medium"), "{screen}");
    assert!(screen.contains("ctx"), "{screen}");
}

#[test]
fn a_user_message_is_rendered() {
    let mut app = app();
    app.lines.push(Line::User("fix the parser".into()));
    let screen = render(&app, 100, 24);
    assert!(screen.contains("fix the parser"), "{screen}");
}

#[test]
fn an_assistant_message_is_rendered() {
    let mut app = app();
    app.lines.push(Line::Assistant("Here is the fix.".into()));
    let screen = render(&app, 100, 24);
    assert!(screen.contains("Here is the fix."), "{screen}");
}

#[test]
fn a_tool_line_shows_its_state() {
    let mut app = app();
    app.lines.push(Line::Tool {
        name: "bash".into(),
        detail: "cargo test".into(),
        running: false,
        is_error: false,
    });
    let screen = render(&app, 100, 24);
    assert!(screen.contains("bash"), "{screen}");
    assert!(screen.contains("cargo test"), "{screen}");
    assert!(screen.contains("✓"), "{screen}");
}

#[test]
fn a_failed_tool_is_marked() {
    let mut app = app();
    app.lines.push(Line::Tool {
        name: "bash".into(),
        detail: "false".into(),
        running: false,
        is_error: true,
    });
    let screen = render(&app, 100, 24);
    assert!(screen.contains("✗"), "{screen}");
}

#[test]
fn the_palette_lists_commands_when_open() {
    let mut app = app();
    app.editor.set("/");
    app.palette = Palette::from_input("/");
    let screen = render(&app, 100, 30);
    assert!(screen.contains("/help"), "{screen}");
    assert!(screen.contains("/model"), "{screen}");
    assert!(screen.contains("Show the keys and commands"), "{screen}");
}

#[test]
fn the_palette_filters_as_you_type() {
    let mut app = app();
    app.editor.set("/mo");
    app.palette = Palette::from_input("/mo");
    let screen = render(&app, 100, 30);
    assert!(screen.contains("/model"), "{screen}");
    assert!(!screen.contains("/quit"), "{screen}");
}

#[test]
fn the_palette_says_when_nothing_matches() {
    let mut app = app();
    app.editor.set("/zzz");
    app.palette = Palette::from_input("/zzz");
    let screen = render(&app, 100, 30);
    assert!(screen.contains("no command matches"), "{screen}");
}

#[test]
fn a_narrow_terminal_still_renders() {
    let mut app = app();
    app.lines.push(Line::Assistant("a fairly long line of text that must wrap".into()));
    let screen = render(&app, 40, 20);
    assert!(screen.contains("luna"), "{screen}");
}

#[test]
fn the_thinking_level_is_reflected() {
    let mut app = app();
    app.thinking = ThinkingLevel::High;
    let screen = render(&app, 120, 24);
    assert!(screen.contains("thinking high"), "{screen}");
}
