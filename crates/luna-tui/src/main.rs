//! Luna's terminal interface.
//!
//! The TUI is a thin view over `luna-core`: it owns no agent state of its own,
//! only the widgets and the input mode. Everything it shows arrives as an
//! [`AgentEvent`], which is what keeps it in lockstep with the GUI.

mod app;
mod highlight;
mod theme;
mod ui;

use std::io::Write;
use std::sync::Arc;

use anyhow::Result;
use crossterm::event::{DisableBracketedPaste, EnableBracketedPaste};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use luna_core::config::{Config, Paths};
use luna_core::event::SinkRef;
use luna_core::provider::ProviderRegistry;
use luna_core::session::SessionStore;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use app::App;

#[global_allocator]
static ALLOC: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn main() -> Result<()> {
    // The installers verify the binary with `--version`, so this must work
    // before any terminal setup happens.
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "--version" || arg == "-V") {
        println!("{}", luna_core::VERSION);
        return Ok(());
    }
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print_help();
        return Ok(());
    }

    let paths = Paths::resolve();
    paths.ensure()?;

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .with_writer(std::io::sink)
        .init();

    let config = Config::load_or_default(&paths);
    let store = SessionStore::open(&paths.data_dir)?;
    let providers = Arc::new(ProviderRegistry::new(&config));

    let runtime =
        tokio::runtime::Builder::new_multi_thread().enable_all().worker_threads(2).build()?;

    let sink: SinkRef = Arc::new(luna_core::event::NullSink);

    let mut app = App::new(config, providers, store, sink, runtime.handle().clone())?;

    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableBracketedPaste)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    let result = app.run(&mut terminal, &runtime);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableBracketedPaste)?;
    terminal.show_cursor()?;
    let _ = std::io::stdout().flush();

    result
}

fn print_help() {
    println!(
        "\
Luna {version} — AI coding agent

USAGE
    luna [OPTIONS]

OPTIONS
    -h, --help       Show this help
    -V, --version    Print the version

KEYS
    Enter            Send the message
    Shift+Enter      New line
    Esc              Abort the running turn
    Ctrl+C           Quit
    Ctrl+N           New session
    Ctrl+S           Toggle the session list
    Ctrl+L           Clear the transcript
    Up / Down        Scroll
    y / a / n        Answer an approval prompt

CONFIG
    {config}
",
        version = luna_core::VERSION,
        config = Paths::resolve().config_file().display()
    );
}
