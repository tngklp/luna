//! Luna TUI: terminal interface (ratatui + crossterm).

mod app;
mod markdown;
mod setup;
mod ui;

use luna_core::config::Config;
use luna_core::orchestrator::AgentRegistry;
use luna_core::orchestrator::agent_loop::{AgentContext, AgentSpec, AutoApprove};
use luna_core::orchestrator::registry::ProviderSettings;
use luna_core::provider::resolver::resolve_active;
use luna_core::provider::types::Message;
use luna_core::tool::builtin;
use luna_core::tool::permissions::PermissionGate;
use luna_core::tool::registry::ToolRegistry;
use std::sync::{Arc, Mutex};

fn main() -> anyhow_noop::Result<()> {
    // Fast paths before touching the terminal.
    match std::env::args().nth(1).as_deref() {
        Some("--version") | Some("-V") => {
            println!("luna {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        Some("--help") | Some("-h") => {
            println!(
                "luna {} — Luna AI agent harness (TUI)",
                env!("CARGO_PKG_VERSION")
            );
            println!();
            println!("Usage: luna [OPTIONS]");
            println!();
            println!("Options:");
            println!("  -V, --version  Print version");
            println!("  -h, --help     Print help");
            println!("  --check        Validate config and provider setup, then exit");
            println!("  --setup        Re-run the interactive setup wizard");
            println!();
            println!("In the TUI: type a message and press Enter; Esc quits;");
            println!("Ctrl+D cancels a running agent; y/n answers approval prompts.");
            return Ok(());
        }
        Some("--check") => return check_setup(),
        Some("--setup") => {
            setup::run_wizard()?;
            return Ok(());
        }
        Some(other) => {
            eprintln!("unknown option: {other} (try --help)");
            std::process::exit(2);
        }
        None => {}
    }

    run_app()
}

fn run_app() -> anyhow_noop::Result<()> {
    let log_file = tracing_log_file();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "luna=info".into()),
        )
        .with_writer(std::sync::Mutex::new(log_file))
        .init();

    // First run without a config -> interactive setup wizard.
    if !luna_core::config::config_path().exists() {
        setup::run_wizard()?;
        // Reload after the wizard wrote the config.
        let cfg = Config::load()?;
        return start_tui(cfg);
    }

    let cfg = Config::load()?;
    start_tui(cfg)
}

fn start_tui(cfg: Config) -> anyhow_noop::Result<()> {
    // One runtime for the whole app: MCP connect, store task, agent runs.
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(run_app_async(cfg))
}

async fn run_app_async(cfg: Config) -> anyhow_noop::Result<()> {
    let resolved = resolve_active(&cfg).ok_or_else(|| {
        std::io::Error::other(
            "no usable provider: run `luna setup`, or enable a provider with an API key in ~/.luna/config.toml",
        )
    })?;

    // Resolve the agent spec (first configured agent, or a default assistant).
    let mut registry = AgentRegistry::new();
    registry.register_provider(
        &resolved.key,
        ProviderSettings {
            model: resolved.model.clone(),
            max_tokens: resolved.max_tokens,
            temperature: resolved.temperature,
        },
    );
    let spec = if let Some(agent_cfg) = cfg.agents.first() {
        registry.register_agent(agent_cfg.clone());
        registry.resolve(&agent_cfg.name)?
    } else {
        AgentSpec {
            name: "assistant".into(),
            provider_key: resolved.key.clone(),
            model: resolved.model.clone(),
            system: Some("You are Luna, a helpful coding agent.".into()),
            tools: vec![],
            max_iterations: 25,
            max_tokens: resolved.max_tokens,
            temperature: resolved.temperature,
        }
    };

    let mut tools = ToolRegistry::new();
    builtin::register_all(&mut tools);
    if let Ok(web) = luna_core::tool::web::WebFetch::new() {
        tools.register(Arc::new(web));
    }
    // MCP servers connect before the terminal takes over (failures skipped).
    if !cfg.mcp_servers.is_empty() {
        let (ok, failed) =
            luna_core::tool::mcp::register_from_config(&mut tools, &cfg.mcp_servers).await;
        if ok + failed > 0 {
            eprintln!("mcp: {ok} connected, {failed} failed");
        }
    }

    let bus = luna_core::EventBus::default();

    // SQLite persistence (best-effort; failures degrade to no persistence).
    let store = {
        let path = luna_core::config::sessions_db_path();
        match luna_core::storage::Store::open(&path) {
            Ok(s) => Arc::new(s),
            Err(e) => {
                tracing::warn!(error = %e, "opening session store failed; persistence disabled");
                Arc::new(luna_core::storage::Store::open_in_memory().expect("in-memory store"))
            }
        }
    };
    luna_core::storage::attach_store(&bus, store.clone());

    let ctx = AgentContext {
        provider: resolved.provider,
        tools,
        gate: Arc::new(PermissionGate::new(
            PermissionGate::default_rules(),
            cfg.permissions.approval_tools.clone(),
        )),
        bus: bus.clone(),
        session_id: uuid::Uuid::new_v4(),
        approvals: Arc::new(AutoApprove),
    };

    let history = Arc::new(Mutex::new(Vec::<Message>::new()));

    // Terminal setup.
    let mut terminal = tui_init()?;
    let result = app::run(&mut terminal, bus, ctx, spec, history.clone(), Some(store));
    tui_restore()?;

    result
}

// Minimal error alias so main stays simple without pulling anyhow in.
mod anyhow_noop {
    pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
}

/// `luna --check`: validate config, provider resolution, and paths.
fn check_setup() -> anyhow_noop::Result<()> {
    println!("Luna {} — setup check", env!("CARGO_PKG_VERSION"));
    println!();

    let cfg_path = luna_core::config::config_path();
    println!("config:      {}", cfg_path.display());
    println!("  exists:    {}", cfg_path.exists());
    println!(
        "sessions:    {}",
        luna_core::config::sessions_db_path().display()
    );
    println!("logs:        {}", luna_core::config::logs_dir().display());
    println!();

    let cfg = match Config::load() {
        Ok(c) => c,
        Err(e) => {
            println!("CONFIG ERROR: {e}");
            return Ok(());
        }
    };

    println!("default_provider: {:?}", cfg.default_provider);
    println!();
    println!("providers:");
    for entry in cfg.providers.entries() {
        let mark = if entry.enabled {
            "enabled "
        } else {
            "disabled"
        };
        let key = if entry.has_api_key {
            "key:set"
        } else {
            "key:--"
        };
        println!("  {:<12} {mark}  {key}  model={}", entry.key, entry.model);
    }
    println!();

    match luna_core::provider::resolver::resolve_active(&cfg) {
        Some(r) => {
            println!("ACTIVE PROVIDER: {} (model {})", r.key, r.model);
            println!("  max_tokens:  {}", r.max_tokens);
            println!("  temperature: {}", r.temperature);
            println!();
            println!("Ready. Run `luna` to start the TUI.");
        }
        None => {
            println!("NO USABLE PROVIDER.");
            println!("  Enable one with an API key, or run `luna --setup`.");
        }
    }
    Ok(())
}

fn tracing_log_file() -> std::fs::File {
    let dir = luna_core::config::logs_dir();
    let _ = std::fs::create_dir_all(&dir);
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("luna-tui.log"))
        .unwrap_or_else(|_| {
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open("luna-tui.log")
                .unwrap()
        })
}

fn tui_init() -> std::io::Result<ratatui::DefaultTerminal> {
    crossterm::terminal::enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    crossterm::execute!(
        stdout,
        crossterm::terminal::EnterAlternateScreen,
        crossterm::event::EnableMouseCapture
    )?;
    ratatui::try_init()
}

fn tui_restore() -> std::io::Result<()> {
    ratatui::restore();
    crossterm::execute!(
        std::io::stdout(),
        crossterm::event::DisableMouseCapture,
        crossterm::terminal::LeaveAlternateScreen
    )?;
    crossterm::terminal::disable_raw_mode()?;
    Ok(())
}
