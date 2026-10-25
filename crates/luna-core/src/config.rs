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
    /// A built-in preset to inherit from, e.g. `groq`. Any field set here wins.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset: Option<String>,
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
            preset: None,
            kind: ProviderKind::Openai,
            base_url: Some(base_url.into()),
            api_key: None,
            api_key_env: None,
            headers: BTreeMap::new(),
            models: Vec::new(),
            discover_models: true,
        }
    }

    /// Fill in anything this entry left unset from its preset.
    ///
    /// Explicit fields always win, so `preset = "groq"` plus a `base_url` points
    /// Groq's key and models at a proxy.
    pub fn apply_preset(&mut self) {
        let Some(id) = self.preset.clone() else { return };
        let Some(preset) = crate::presets::find(&id) else {
            tracing::warn!("unknown provider preset `{id}`; using the entry as written");
            return;
        };
        if self.base_url.is_none() {
            self.base_url = Some(preset.base_url.to_string());
        }
        if self.api_key_env.is_none() && !preset.key_env.is_empty() {
            self.api_key_env = Some(preset.key_env.to_string());
        }
        if self.models.is_empty() {
            self.models = preset.models.iter().map(|model| model.to_string()).collect();
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
    /// Cost ceiling for this role across the whole tree. Zero disables it.
    #[serde(default)]
    pub max_cost_usd: Option<f64>,
}

impl Default for RoleConfig {
    fn default() -> Self {
        RoleConfig {
            model: None,
            tools: Vec::new(),
            system: None,
            max_steps: Some(40),
            max_cost_usd: None,
        }
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
    /// MCP servers to connect to.
    #[serde(default)]
    pub mcp: Vec<crate::mcp::ServerConfig>,
    /// Where `web_search` sends its queries.
    #[serde(default)]
    pub search_backend: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        let providers = crate::presets::default_providers();

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
            mcp: Vec::new(),
            search_backend: None,
        }
    }
}

/// Where Luna keeps its state.
///
/// One directory, `~/.luna`, holds everything: the config, the session
/// transcripts and the index. Keeping it in one obvious place means a user can
/// back it up, inspect it, or delete it without hunting through platform
/// directories.
#[derive(Debug, Clone)]
pub struct Paths {
    /// `~/.luna`
    pub root: PathBuf,
}

impl Paths {
    pub fn resolve() -> Paths {
        Paths::resolve_in(home_dir())
    }

    /// Resolve against a specific home directory. Used by tests.
    pub fn resolve_in(home: impl Into<PathBuf>) -> Paths {
        Paths { root: home.into().join(".luna") }
    }

    /// The config file, `~/.luna/config.toml`.
    pub fn config_file(&self) -> PathBuf {
        self.root.join("config.toml")
    }

    pub fn sessions_dir(&self) -> PathBuf {
        self.root.join("sessions")
    }

    pub fn db_file(&self) -> PathBuf {
        self.root.join("luna.db")
    }

    pub fn log_file(&self) -> PathBuf {
        self.root.join("luna.log")
    }

    pub fn ensure(&self) -> Result<()> {
        for dir in [&self.root, &self.sessions_dir()] {
            std::fs::create_dir_all(dir)
                .map_err(|source| LunaError::Io { path: dir.clone(), source })?;
        }
        Ok(())
    }
}

/// The user's home directory.
///
/// `HOME` first, then `USERPROFILE` for Windows, then the platform API as a
/// last resort.
pub fn home_dir() -> PathBuf {
    if let Some(home) = std::env::var_os("HOME").filter(|value| !value.is_empty()) {
        return PathBuf::from(home);
    }
    if let Some(home) = std::env::var_os("USERPROFILE").filter(|value| !value.is_empty()) {
        return PathBuf::from(home);
    }
    directories::BaseDirs::new()
        .map(|dirs| dirs.home_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
}

impl Config {
    /// Load the config, writing a default one when no file exists.
    ///
    /// The file lives at `~/.luna/config.toml`. A first run creates it, so the
    /// settings a session used are always inspectable.
    pub fn load(paths: &Paths) -> Result<Config> {
        paths.ensure()?;

        let path = paths.config_file();
        if !path.exists() {
            let config = Config::default();
            config.save(paths)?;
            return Ok(config);
        }

        let mut config = Config::load_file(&path)?;
        config.normalize();
        Ok(config)
    }

    /// Read one config file, without any of the path resolution.
    fn load_file(path: &Path) -> Result<Config> {
        let text = std::fs::read_to_string(path)
            .map_err(|source| LunaError::Io { path: path.to_path_buf(), source })?;
        toml::from_str(&text).map_err(|err| LunaError::Config(format!("{}: {err}", path.display())))
    }

    /// Fill in preset defaults and keep the `local` alias working.
    ///
    /// `local` predates the named local presets, so a config that still uses it
    /// keeps working: it is pointed at Ollama unless it already has a URL.
    pub fn normalize(&mut self) {
        for provider in self.providers.values_mut() {
            provider.apply_preset();
        }
        if let Some(local) = self.providers.get_mut("local") {
            if local.base_url.is_none() {
                local.base_url = Some("http://localhost:11434/v1".to_string());
            }
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_home(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("luna-cfg-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn everything_lives_under_dot_luna_in_the_home_directory() {
        let paths = Paths::resolve_in("/home/dev");
        assert_eq!(paths.root, PathBuf::from("/home/dev/.luna"));
        assert_eq!(paths.config_file(), PathBuf::from("/home/dev/.luna/config.toml"));
        assert_eq!(paths.sessions_dir(), PathBuf::from("/home/dev/.luna/sessions"));
        assert_eq!(paths.db_file(), PathBuf::from("/home/dev/.luna/luna.db"));
    }

    #[test]
    fn loading_writes_a_default_config_when_none_exists() {
        let home = temp_home("default");
        let paths = Paths::resolve_in(&home);
        let config = Config::load(&paths).unwrap();
        assert_eq!(config.general.default_model, "anthropic/claude-sonnet-4-5");
        assert!(paths.config_file().exists());
        assert!(paths.config_file().ends_with(".luna/config.toml"));
        std::fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn a_fresh_working_directory_gets_nothing() {
        // The config belongs to the user, not the project, so launching in a new
        // directory must not litter it.
        let home = temp_home("fresh-home");
        let work = temp_home("fresh-work");
        let paths = Paths::resolve_in(&home);
        Config::load(&paths).unwrap();

        let entries: Vec<_> = std::fs::read_dir(&work).unwrap().flatten().collect();
        assert!(entries.is_empty(), "the working directory should stay untouched");
        std::fs::remove_dir_all(&home).ok();
        std::fs::remove_dir_all(&work).ok();
    }

    #[test]
    fn an_existing_config_is_read() {
        let home = temp_home("read");
        let paths = Paths::resolve_in(&home);
        std::fs::create_dir_all(&paths.root).unwrap();
        std::fs::write(
            paths.config_file(),
            "[general]\ndefault_model = \"groq/llama-3.3-70b-versatile\"\n",
        )
        .unwrap();

        let config = Config::load(&paths).unwrap();
        assert_eq!(config.general.default_model, "groq/llama-3.3-70b-versatile");
        std::fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn saving_writes_back_to_the_same_file() {
        let home = temp_home("save");
        let paths = Paths::resolve_in(&home);
        let mut config = Config::load(&paths).unwrap();
        config.general.max_steps = 7;
        config.save(&paths).unwrap();

        let text = std::fs::read_to_string(paths.config_file()).unwrap();
        assert!(text.contains("max_steps = 7"));
        std::fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn a_preset_fills_in_the_endpoint_and_key() {
        let mut provider = ProviderConfig {
            preset: Some("groq".into()),
            kind: ProviderKind::Openai,
            base_url: None,
            api_key: None,
            api_key_env: None,
            headers: BTreeMap::new(),
            models: Vec::new(),
            discover_models: true,
        };
        provider.apply_preset();
        assert_eq!(provider.base_url.as_deref(), Some("https://api.groq.com/openai/v1"));
        assert_eq!(provider.api_key_env.as_deref(), Some("GROQ_API_KEY"));
        assert!(!provider.models.is_empty());
    }

    #[test]
    fn explicit_fields_beat_the_preset() {
        let mut provider = ProviderConfig {
            preset: Some("groq".into()),
            kind: ProviderKind::Openai,
            base_url: Some("https://proxy.internal/v1".into()),
            api_key: None,
            api_key_env: Some("MY_KEY".into()),
            headers: BTreeMap::new(),
            models: vec!["my-model".into()],
            discover_models: true,
        };
        provider.apply_preset();
        assert_eq!(provider.base_url.as_deref(), Some("https://proxy.internal/v1"));
        assert_eq!(provider.api_key_env.as_deref(), Some("MY_KEY"));
        assert_eq!(provider.models, vec!["my-model"]);
    }

    #[test]
    fn an_unknown_preset_leaves_the_entry_alone() {
        let mut provider = ProviderConfig {
            preset: Some("nope".into()),
            kind: ProviderKind::Openai,
            base_url: Some("https://example.test/v1".into()),
            api_key: None,
            api_key_env: None,
            headers: BTreeMap::new(),
            models: Vec::new(),
            discover_models: true,
        };
        provider.apply_preset();
        assert_eq!(provider.base_url.as_deref(), Some("https://example.test/v1"));
    }

    #[test]
    fn the_local_alias_still_points_at_ollama() {
        let mut config = Config::default();
        config.providers.insert(
            "local".into(),
            ProviderConfig {
                preset: None,
                kind: ProviderKind::Openai,
                base_url: None,
                api_key: None,
                api_key_env: None,
                headers: BTreeMap::new(),
                models: Vec::new(),
                discover_models: true,
            },
        );
        config.normalize();
        assert_eq!(
            config.providers["local"].base_url.as_deref(),
            Some("http://localhost:11434/v1")
        );
    }

    #[test]
    fn a_custom_provider_needs_no_preset() {
        let mut config = Config::default();
        config.providers.insert(
            "my-gateway".into(),
            ProviderConfig {
                preset: None,
                kind: ProviderKind::Openai,
                base_url: Some("https://gateway.internal/v1".into()),
                api_key: None,
                api_key_env: Some("GATEWAY_KEY".into()),
                headers: BTreeMap::from([("X-Team".to_string(), "luna".to_string())]),
                models: vec!["house-model".into()],
                discover_models: false,
            },
        );
        config.normalize();
        let provider = &config.providers["my-gateway"];
        assert_eq!(provider.base_url.as_deref(), Some("https://gateway.internal/v1"));
        assert_eq!(provider.headers["X-Team"], "luna");
        assert_eq!(provider.models, vec!["house-model"]);
    }

    #[test]
    fn model_references_split_on_the_first_slash() {
        assert_eq!(
            Config::split_model("openrouter/anthropic/claude-sonnet-4.5"),
            ("openrouter".to_string(), "anthropic/claude-sonnet-4.5".to_string())
        );
    }
}
