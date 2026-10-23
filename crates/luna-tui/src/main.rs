//! Luna TUI: terminal interface (ratatui + crossterm).

mod app;
mod ui;

use luna_core::config::Config;
use luna_core::orchestrator::AgentRegistry;
use luna_core::orchestrator::agent_loop::{AgentContext, AgentSpec, AutoApprove};
use luna_core::orchestrator::registry::ProviderSettings;
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
            println!();
            println!("In the TUI: type a message and press Enter; Esc quits;");
            println!("Ctrl+D cancels a running agent; y/n answers approval prompts.");
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

    let cfg = Config::load()?;
    let (provider_key, provider, settings) = pick_provider(&cfg).ok_or_else(|| {
        std::io::Error::other(
            "no provider enabled: set ANTHROPIC_API_KEY / OPENAI_API_KEY or enable ollama in config.toml",
        )
    })?;

    // Resolve the agent spec (first configured agent, or a default assistant).
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
        registry.resolve(&agent_cfg.name)?
    } else {
        AgentSpec {
            name: "assistant".into(),
            provider_key: provider_key.clone(),
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
    // MCP servers connect before the terminal takes over (failures skipped).
    if !cfg.mcp_servers.is_empty() {
        let rt = tokio::runtime::Runtime::new()?;
        let (ok, failed) = rt.block_on(async {
            luna_core::tool::mcp::register_from_config(&mut tools, &cfg.mcp_servers).await
        });
        if ok + failed > 0 {
            eprintln!("mcp: {ok} connected, {failed} failed");
        }
    }

    let bus = luna_core::EventBus::default();

    // SQLite persistence (best-effort; failures degrade to no persistence).
    let store = {
        let path = cfg.storage.data_dir.join("sessions.db");
        match luna_core::storage::Store::open(&path) {
            Ok(s) => Arc::new(s),
            Err(e) => {
                tracing::warn!(error = %e, "opening session store failed; persistence disabled");
                Arc::new(luna_core::storage::Store::open_in_memory().expect("in-memory store"))
            }
        }
    };
    luna_core::storage::attach_store(&bus, store);

    let ctx = AgentContext {
        provider,
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
    let result = app::run(&mut terminal, bus, ctx, spec, history.clone());
    tui_restore()?;

    result
}

// Minimal error alias so main stays simple without pulling anyhow in.
mod anyhow_noop {
    pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
}

/// Resolved provider settings tuple: (model, max_tokens, temperature).
pub type ProviderSettingsTuple = (String, u32, f32);

fn pick_provider(
    cfg: &Config,
) -> Option<(
    String,
    Arc<dyn luna_core::provider::Provider>,
    ProviderSettingsTuple,
)> {
    if cfg.providers.anthropic.enabled
        && let Ok(key) = std::env::var("ANTHROPIC_API_KEY")
        && let Ok(p) = luna_core::provider::anthropic::AnthropicProvider::new(key)
    {
        let c = &cfg.providers.anthropic;
        return Some((
            "anthropic".into(),
            Arc::new(p),
            (c.model.clone(), c.max_tokens, c.temperature),
        ));
    }
    if cfg.providers.openai.enabled
        && let Ok(key) = std::env::var("OPENAI_API_KEY")
        && let Ok(p) = luna_core::provider::openai_compat::OpenAiCompatProvider::openai(key)
    {
        let c = &cfg.providers.openai;
        return Some((
            "openai".into(),
            Arc::new(p),
            (c.model.clone(), c.max_tokens, c.temperature),
        ));
    }
    if cfg.providers.ollama.enabled
        && let Ok(p) =
            luna_core::provider::ollama::OllamaProvider::new(&cfg.providers.ollama.base_url)
    {
        let c = &cfg.providers.ollama;
        return Some((
            "ollama".into(),
            Arc::new(p),
            (c.model.clone(), c.max_tokens, c.temperature),
        ));
    }
    None
}

fn tracing_log_file() -> std::fs::File {
    let dir = directories::ProjectDirs::from("dev", "luna", "Luna")
        .map(|d| d.data_dir().join("logs"))
        .unwrap_or_else(|| std::path::PathBuf::from(".luna/logs"));
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
