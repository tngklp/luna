//! Luna's terminal interface.
//!
//! The TUI is a thin view over `luna-core`: it owns no agent state of its own,
//! only the widgets and the input mode. Everything it shows arrives as an
//! [`AgentEvent`], which is what keeps it in lockstep with the GUI.

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

use luna_tui::app::App;

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

    let model_override = parse_model(&args)?;

    let paths = Paths::resolve();
    paths.ensure()?;

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .with_writer(std::io::sink)
        .init();

    let mut config = Config::load_or_default(&paths);
    if let Some(model) = model_override {
        config.general.default_model = model;
    }
    let config_path = paths.config_file().display().to_string();
    let project_config = paths.uses_project_config();
    let store = SessionStore::open(&paths.data_dir)?;
    let providers = Arc::new(ProviderRegistry::new(&config));

    let runtime =
        tokio::runtime::Builder::new_multi_thread().enable_all().worker_threads(2).build()?;

    let sink: SinkRef = Arc::new(luna_core::event::NullSink);

    let mut app = App::new(
        config,
        providers,
        store,
        sink,
        runtime.handle().clone(),
        config_path,
        project_config,
    )?;

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
    -m, --model <ref>   Model to run, as provider/model (e.g. local/llama3.2)
    -h, --help          Show this help
    -V, --version       Print the version

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

/// Read `--model <ref>` or `--model=<ref>`.
///
/// A bare model name is accepted too: `Config::split_model` treats it as a local
/// model, so `luna -m llama3.2` does the obvious thing.
fn parse_model(args: &[String]) -> Result<Option<String>> {
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        if arg == "--model" || arg == "-m" {
            let value = args.get(index + 1).ok_or_else(|| {
                anyhow::anyhow!("--model needs a value, e.g. --model local/llama3.2")
            })?;
            return Ok(Some(value.clone()));
        }
        if let Some(value) = arg.strip_prefix("--model=") {
            return Ok(Some(value.to_string()));
        }
        index += 1;
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn reads_a_separate_model_argument() {
        assert_eq!(
            parse_model(&args(&["--model", "local/x"])).unwrap().as_deref(),
            Some("local/x")
        );
        assert_eq!(parse_model(&args(&["-m", "local/x"])).unwrap().as_deref(), Some("local/x"));
    }

    #[test]
    fn reads_an_inline_model_argument() {
        assert_eq!(parse_model(&args(&["--model=local/x"])).unwrap().as_deref(), Some("local/x"));
    }

    #[test]
    fn no_model_flag_means_no_override() {
        assert!(parse_model(&args(&[])).unwrap().is_none());
        assert!(parse_model(&args(&["--help"])).unwrap().is_none());
    }

    #[test]
    fn a_missing_value_is_an_error() {
        assert!(parse_model(&args(&["--model"])).is_err());
    }
}
