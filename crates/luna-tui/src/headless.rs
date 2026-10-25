//! Headless mode and diagnostics.
//!
//! `luna -p "…"` runs one prompt without a terminal, which is what CI and
//! scripts need. `luna doctor` checks the things that usually go wrong.

use std::sync::Arc;

use anyhow::Result;
use luna_core::agent::{build_system, AgentDeps, AgentSession, UiAnswer, UiBridge, UiRequest};
use luna_core::config::{Config, Paths};
use luna_core::event::{AgentEvent, AgentRef, Emitted, EventSink, SinkRef};
use luna_core::provider::ProviderRegistry;
use luna_core::session::{SessionHeader, SessionStore};
use serde_json::json;

/// How headless output is shaped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    /// Just the assistant's text.
    Text,
    /// One JSON object per event, then a final summary.
    Json,
}

/// Forwards events to stdout as JSON lines.
struct JsonSink;

impl EventSink for JsonSink {
    fn emit(&self, event: Emitted) {
        println!("{}", event.to_json());
    }
}

/// Approves nothing: a headless run has no one to ask.
struct DenyAll;

#[async_trait::async_trait]
impl UiBridge for DenyAll {
    async fn ask(&self, _request: UiRequest) -> UiAnswer {
        UiAnswer::Deny
    }
}

/// Approves everything, for `--yes`.
struct AllowAll;

#[async_trait::async_trait]
impl UiBridge for AllowAll {
    async fn ask(&self, _request: UiRequest) -> UiAnswer {
        UiAnswer::Allow
    }
}

/// Run one prompt and print the result.
pub async fn run_prompt(
    prompt: &str,
    model: Option<String>,
    format: OutputFormat,
    yes: bool,
) -> Result<i32> {
    let paths = Paths::resolve();
    paths.ensure()?;
    let mut config = Config::load_or_default(&paths);
    if let Some(model) = model {
        config.general.default_model = model;
    }

    let providers = Arc::new(ProviderRegistry::new(&config));
    let store = SessionStore::open(&paths.root)?;
    let cwd = std::env::current_dir().unwrap_or_else(|_| ".".into());

    let sink: SinkRef = match format {
        OutputFormat::Json => Arc::new(JsonSink),
        OutputFormat::Text => Arc::new(luna_core::event::NullSink),
    };

    let ui: Arc<dyn UiBridge> = if yes { Arc::new(AllowAll) } else { Arc::new(DenyAll) };
    let deps = Arc::new(AgentDeps::new(providers, sink.clone(), &cwd).with_ui(ui));

    let id = uuid::Uuid::new_v4().to_string();
    let header = SessionHeader::new(&id, cwd.display().to_string(), &config.general.default_model);
    let mut transcript = store.create(&header)?;

    let mut session = AgentSession::new(
        deps,
        AgentRef::root(&id),
        &config.general.default_model,
        build_system(&config, &cwd, None),
    );

    let outcome = session.run_turn(prompt).await;

    for message in &session.messages {
        if message.role != luna_core::types::Role::System {
            let _ = transcript.append_message(message);
        }
    }

    match format {
        OutputFormat::Text => {
            let answer = session
                .messages
                .iter()
                .rev()
                .find(|message| message.role == luna_core::types::Role::Assistant)
                .map(|message| message.text())
                .unwrap_or_default();
            println!("{answer}");
        }
        OutputFormat::Json => {
            println!(
                "{}",
                json!({
                    "type": "result",
                    "session": transcript.path().display().to_string(),
                    "steps": session.stats.steps,
                    "toolCalls": session.stats.tool_calls,
                    "cost": session.stats.cost,
                    "tokens": {
                        "input": session.stats.usage.input,
                        "output": session.stats.usage.output,
                    },
                    "ok": outcome.is_ok(),
                })
            );
        }
    }

    match outcome {
        Ok(()) => Ok(0),
        Err(err) => {
            eprintln!("luna: {err}");
            Ok(1)
        }
    }
}

/// One diagnostic result.
pub struct Check {
    pub name: String,
    pub ok: bool,
    pub detail: String,
}

/// Check the things that usually go wrong.
pub async fn doctor() -> Result<i32> {
    let paths = Paths::resolve();
    let mut checks: Vec<Check> = Vec::new();

    // Config
    let config_path = paths.config_file();
    checks.push(Check {
        name: "config".into(),
        ok: config_path.exists(),
        detail: if config_path.exists() {
            config_path.display().to_string()
        } else {
            format!("{} is missing; run luna once to create it", config_path.display())
        },
    });

    let config = Config::load_or_default(&paths);

    // Data directory
    checks.push(Check {
        name: "data directory".into(),
        ok: paths.root.exists(),
        detail: paths.root.display().to_string(),
    });

    // Providers
    let registry = ProviderRegistry::new(&config);
    let mut ready = Vec::new();
    let mut missing = Vec::new();
    for name in registry.names() {
        match registry.get(&name) {
            Ok(provider) if provider.ready() => ready.push(name),
            _ => missing.push(name),
        }
    }
    checks.push(Check {
        name: "providers".into(),
        ok: !ready.is_empty(),
        detail: if ready.is_empty() {
            format!("none usable; set a key for one of: {}", missing.join(", "))
        } else {
            format!("ready: {}", ready.join(", "))
        },
    });

    // Default model
    let (provider_name, model) = Config::split_model(&config.general.default_model);
    let model_ok = registry.get(&provider_name).is_ok();
    checks.push(Check {
        name: "default model".into(),
        ok: model_ok,
        detail: if model_ok {
            format!("{provider_name}/{model}")
        } else {
            format!("`{provider_name}` is not a configured provider")
        },
    });

    // Reachability of each ready provider
    for name in &ready {
        if let Ok(provider) = registry.get(name) {
            let started = std::time::Instant::now();
            let reachable = provider.list_models().await.is_ok();
            checks.push(Check {
                name: format!("{name} reachable"),
                ok: reachable,
                detail: if reachable {
                    format!("answered in {}ms", started.elapsed().as_millis())
                } else {
                    "could not list models".to_string()
                },
            });
        }
    }

    // Extensions
    let project = std::env::current_dir().unwrap_or_else(|_| ".".into());
    let extensions = luna_core::extensions::Extensions::load(&project, &paths.root);
    checks.push(Check {
        name: "extensions".into(),
        ok: extensions.problems.is_empty(),
        detail: if extensions.problems.is_empty() {
            format!(
                "{} skills, {} commands, {} hooks, {} plugins",
                extensions.skills.len(),
                extensions.commands.len(),
                extensions.hooks.len(),
                extensions.plugins.len()
            )
        } else {
            extensions.problems.join("; ")
        },
    });

    // MCP
    if config.mcp.is_empty() {
        checks.push(Check { name: "mcp".into(), ok: true, detail: "no servers configured".into() });
    } else {
        let (tools, problems) = luna_core::mcp::connect_all(&config.mcp).await;
        checks.push(Check {
            name: "mcp".into(),
            ok: problems.is_empty(),
            detail: if problems.is_empty() {
                format!("{} tools from {} servers", tools.len(), config.mcp.len())
            } else {
                problems.join("; ")
            },
        });
    }

    // Sessions
    let sessions = SessionStore::open(&paths.root).map(|store| store.list().unwrap_or_default());
    checks.push(Check {
        name: "sessions".into(),
        ok: sessions.is_ok(),
        detail: match &sessions {
            Ok(list) => format!("{} saved", list.len()),
            Err(err) => err.to_string(),
        },
    });

    let failed = checks.iter().filter(|check| !check.ok).count();
    for check in &checks {
        let mark = if check.ok { "✓" } else { "✗" };
        println!("{mark} {:<22} {}", check.name, check.detail);
    }
    println!();
    if failed == 0 {
        println!("everything looks good");
        Ok(0)
    } else {
        println!("{failed} check(s) need attention");
        Ok(1)
    }
}

/// Print the assistant's answer from a finished session, for tests.
pub fn last_answer(session: &AgentSession) -> String {
    session
        .messages
        .iter()
        .rev()
        .find(|message| message.role == luna_core::types::Role::Assistant)
        .map(|message| message.text())
        .unwrap_or_default()
}

/// Emit a settled event, so a JSON consumer sees the run end.
pub fn settle(sink: &SinkRef, agent: &AgentRef) {
    sink.emit(Emitted::new(agent.clone(), AgentEvent::AgentSettled {}));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_output_formats_are_distinct() {
        assert_ne!(OutputFormat::Text, OutputFormat::Json);
    }
}
