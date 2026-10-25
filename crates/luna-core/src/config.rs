//! Configuration: providers, models, roles, approvals.
//!
//! Stored as TOML at `<config dir>/luna/config.toml`. A missing file is not an
//! error — Luna writes a commented default and carries on, so a first run never
//! fails just because nothing has been configured yet.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{LunaError, Result};

/// Which wire protocol a provider speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    /// OpenAI Chat Completions. Also used by Ollama, llama.cpp and any custom
    /// endpoint that speaks the same dialect.
    Openai,
    Anthropic,
    Gemini,
}

impl ProviderKind {
    pub fn default_base_url(self, name: &str) -> String {
        match self {
            ProviderKind::Openai => match name {
                "local" | "ollama" => "http://localhost:11434/v1".into(),
                _ => "https://api.openai.com/v1".into(),
            },
            ProviderKind::Anthropic => "https://api.anthropic.com".into(),
            ProviderKind::Gemini => "https://generativelanguage.googleapis.com/v1beta".into(),
        }
    }

    pub fn default_key_env(self) -> &'static str {
        match self {
            ProviderKind::Openai => "OPENAI_API_KEY",
            ProviderKind::Anthropic => "ANTHROPIC_API_KEY",
            ProviderKind::Gemini => "GEMINI_API_KEY",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ProviderConfig {
    pub kind: ProviderKind,
    #[serde(default)]
    pub base_url: Option<String>,
    /// Inline key. Prefer `api_key_env` so the key never lands in a dotfile.
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub api_key_env: Option<String>,
    /// Extra request headers, for proxies and gateways.
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    /// Models this provider exposes when discovery is unavailable.
    #[serde(default)]
    pub models: Vec<String>,
    /// Whether provider-side model listing should be attempted.
    #[serde(default = "yes")]
    pub discover_models: bool,
}

fn yes() -> bool {
    true
}

impl ProviderConfig {
    pub fn openai(base_url: impl Into<String>) -> Self {
        ProviderConfig {
            kind: ProviderKind::Openai,
            base_url: Some(base_url.into()),
            api_key: None,
            api_key_env: None,
            headers: BTreeMap::new(),
            models: Vec::new(),
            discover_models: true,
        }
    }

    pub fn base_url(&self, name: &str) -> String {
        self.base_url.clone().unwrap_or_else(|| self.kind.default_base_url(name))
    }

    /// Resolve the key from the inline value, then the named env var, then the
    /// provider's conventional env var.
    pub fn api_key(&self) -> Option<String> {
        if let Some(key) = &self.api_key {
            if !key.is_empty() {
                return Some(key.clone());
            }
        }
        let env = self.api_key_env.as_deref().unwrap_or(self.kind.default_key_env());
        std::env::var(env).ok().filter(|value| !value.is_empty())
    }
}

/// What Luna should do before running a class of tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ApprovalAction {
    Allow,
    Ask,
    Deny,
}

impl ApprovalAction {
    pub fn as_str(self) -> &'static str {
        match self {
            ApprovalAction::Allow => "allow",
            ApprovalAction::Ask => "ask",
            ApprovalAction::Deny => "deny",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ApprovalConfig {
    pub read: ApprovalAction,
    pub write: ApprovalAction,
    pub bash: ApprovalAction,
    pub network: ApprovalAction,
}

impl Default for ApprovalConfig {
    fn default() -> Self {
        ApprovalConfig {
            read: ApprovalAction::Allow,
            write: ApprovalAction::Ask,
            bash: ApprovalAction::Ask,
            network: ApprovalAction::Allow,
        }
    }
}

/// A named agent role. The supervisor picks one per subagent.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RoleConfig {
    /// Model to run this role on, as `provider/model`. Falls back to the
    /// session model.
    #[serde(default)]
    pub model: Option<String>,
    /// Tool allowlist. Empty means "every tool the parent has".
    #[serde(default)]
    pub tools: Vec<String>,
    #[serde(default)]
    pub system: Option<String>,
    /// Hard cap on agentic steps before the role must report back.
    #[serde(default)]
    pub max_steps: Option<usize>,
}

impl Default for RoleConfig {
    fn default() -> Self {
        RoleConfig { model: None, tools: Vec::new(), system: None, max_steps: Some(40) }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct GeneralConfig {
    /// `provider/model` used when nothing else is selected.
    pub default_model: String,
    /// Maximum subagents running at once.
    #[serde(default = "default_concurrency")]
    pub max_concurrent_agents: usize,
    /// Hard ceiling for one agent tree, in USD. Zero disables the guard.
    #[serde(default)]
    pub max_cost_usd: f64,
    /// Steps a root agent may take in one turn before being stopped.
    #[serde(default = "default_steps")]
    pub max_steps: usize,
    /// Context fraction that triggers automatic compaction.
    #[serde(default = "default_compact_at")]
    pub compact_at: f64,
    /// Shell command timeout, in seconds.
    #[serde(default = "default_timeout")]
    pub bash_timeout_secs: u64,
}

fn default_concurrency() -> usize {
    // Enough fan-out to be useful, low enough to stay readable and cheap.
    4
}

fn default_steps() -> usize {
    60
}

fn default_compact_at() -> f64 {
    0.8
}

fn default_timeout() -> u64 {
    120
}

impl Default for GeneralConfig {
    fn default() -> Self {
        GeneralConfig {
            default_model: "anthropic/claude-sonnet-4-5".into(),
            max_concurrent_agents: default_concurrency(),
            max_cost_usd: 0.0,
            max_steps: default_steps(),
            compact_at: default_compact_at(),
            bash_timeout_secs: default_timeout(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Config {
    #[serde(default)]
    pub general: GeneralConfig,
    #[serde(default)]
    pub providers: BTreeMap<String, ProviderConfig>,
    #[serde(default)]
    pub roles: BTreeMap<String, RoleConfig>,
    #[serde(default)]
    pub approvals: ApprovalConfig,
    /// Extra instructions appended to every system prompt.
    #[serde(default)]
    pub instructions: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        let mut providers = BTreeMap::new();
        providers.insert(
            "anthropic".to_string(),
            ProviderConfig {
                kind: ProviderKind::Anthropic,
                base_url: None,
                api_key: None,
                api_key_env: Some("ANTHROPIC_API_KEY".into()),
                headers: BTreeMap::new(),
                models: vec!["claude-sonnet-4-5".into(), "claude-opus-4-1".into()],
                discover_models: true,
            },
        );
        providers.insert(
            "openai".to_string(),
            ProviderConfig {
                kind: ProviderKind::Openai,
                base_url: None,
                api_key: None,
                api_key_env: Some("OPENAI_API_KEY".into()),
                headers: BTreeMap::new(),
                models: vec!["gpt-5".into(), "gpt-5-mini".into()],
                discover_models: true,
            },
        );
        providers.insert(
            "gemini".to_string(),
            ProviderConfig {
                kind: ProviderKind::Gemini,
                base_url: None,
                api_key: None,
                api_key_env: Some("GEMINI_API_KEY".into()),
                headers: BTreeMap::new(),
                models: vec!["gemini-2.5-pro".into(), "gemini-2.5-flash".into()],
                discover_models: true,
            },
        );
        providers.insert("local".to_string(), ProviderConfig::openai("http://localhost:11434/v1"));

        let roles = BTreeMap::from([
            ("lead".to_string(), RoleConfig { max_steps: Some(80), ..RoleConfig::default() }),
            (
                "planner".to_string(),
                RoleConfig {
                    max_steps: Some(30),
                    tools: vec![
                        "read".into(),
                        "glob".into(),
                        "grep".into(),
                        "ls".into(),
                        "todo_write".into(),
                    ],
                    ..RoleConfig::default()
                },
            ),
            ("worker".to_string(), RoleConfig { max_steps: Some(50), ..RoleConfig::default() }),
            (
                "reviewer".to_string(),
                RoleConfig {
                    max_steps: Some(20),
                    tools: vec!["read".into(), "grep".into(), "glob".into()],
                    ..RoleConfig::default()
                },
            ),
            (
                "researcher".to_string(),
                RoleConfig {
                    max_steps: Some(25),
                    tools: vec![
                        "read".into(),
                        "grep".into(),
                        "glob".into(),
                        "ls".into(),
                        "web_fetch".into(),
                    ],
                    ..RoleConfig::default()
                },
            ),
        ]);

        Config {
            general: GeneralConfig::default(),
            providers,
            roles,
            approvals: ApprovalConfig::default(),
            instructions: None,
        }
    }
}

/// Where Luna keeps its state.
#[derive(Debug, Clone)]
pub struct Paths {
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
}

impl Paths {
    pub fn resolve() -> Paths {
        use directories::ProjectDirs;
        // Luna keeps everything under one root so the installers and the app
        // agree on where to look.
        let base = ProjectDirs::from("", "", "luna");
        let (config_dir, data_dir) = match &base {
            Some(dirs) => (dirs.config_dir().to_path_buf(), dirs.data_dir().to_path_buf()),
            None => {
                let home = std::env::var_os("HOME")
                    .or_else(|| std::env::var_os("USERPROFILE"))
                    .map(PathBuf::from)
                    .unwrap_or_else(|| PathBuf::from("."));
                (home.join(".config").join("luna"), home.join(".local/share/luna"))
            }
        };
        // `ProjectDirs` already appends the application name, and on Windows it
        // also appends `\config` to the roaming directory. Both roots are used
        // exactly as given.
        Paths { config_dir, data_dir }
    }

    pub fn config_file(&self) -> PathBuf {
        self.config_dir.join("config.toml")
    }

    pub fn sessions_dir(&self) -> PathBuf {
        self.data_dir.join("sessions")
    }

    pub fn db_file(&self) -> PathBuf {
        self.data_dir.join("luna.db")
    }

    pub fn log_file(&self) -> PathBuf {
        self.data_dir.join("luna.log")
    }

    pub fn ensure(&self) -> Result<()> {
        for dir in [&self.config_dir, &self.data_dir, &self.sessions_dir()] {
            std::fs::create_dir_all(dir)
                .map_err(|source| LunaError::Io { path: dir.clone(), source })?;
        }
        Ok(())
    }
}

impl Config {
    /// Load the config, writing a default one when the file does not exist.
    pub fn load(paths: &Paths) -> Result<Config> {
        paths.ensure()?;
        let path = paths.config_file();
        if !path.exists() {
            let config = Config::default();
            config.save(paths)?;
            return Ok(config);
        }
        let text = std::fs::read_to_string(&path)
            .map_err(|source| LunaError::Io { path: path.clone(), source })?;
        toml::from_str(&text).map_err(|err| LunaError::Config(format!("{}: {err}", path.display())))
    }

    /// Load, falling back to defaults when the file is unreadable. Used by
    /// front-ends so a broken config never blocks the window from opening.
    pub fn load_or_default(paths: &Paths) -> Config {
        match Config::load(paths) {
            Ok(config) => config,
            Err(err) => {
                tracing::warn!("falling back to default config: {err}");
                paths.ensure().ok();
                Config::default()
            }
        }
    }

    pub fn save(&self, paths: &Paths) -> Result<()> {
        paths.ensure()?;
        let path = paths.config_file();
        let text = toml::to_string_pretty(self)
            .map_err(|err| LunaError::Config(format!("could not serialize config: {err}")))?;
        write_atomic(&path, text.as_bytes())
    }

    /// The provider entry for a `provider/model` reference.
    pub fn split_model(reference: &str) -> (String, String) {
        match reference.split_once('/') {
            Some((provider, model)) => (provider.to_string(), model.to_string()),
            // A bare model name falls back to the local provider so `luna -m
            // llama3.2` does the obvious thing.
            None => ("local".to_string(), reference.to_string()),
        }
    }

    pub fn role(&self, name: &str) -> RoleConfig {
        self.roles.get(name).cloned().unwrap_or_default()
    }
}

/// Write through a temp file so a crash cannot leave a half-written config.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|source| LunaError::Io { path: parent.to_path_buf(), source })?;
    }
    let temp = path.with_extension(format!("tmp{}", std::process::id()));
    std::fs::write(&temp, bytes).map_err(|source| LunaError::Io { path: temp.clone(), source })?;
    std::fs::rename(&temp, path)
        .map_err(|source| LunaError::Io { path: path.to_path_buf(), source })?;
    Ok(())
}
