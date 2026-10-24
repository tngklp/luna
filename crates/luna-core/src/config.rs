//! TOML configuration: providers, agents, tool permissions.
//!
//! Everything lives under `~/.luna/`:
//!   ~/.luna/config.toml   — settings
//!   ~/.luna/sessions.db   — session history
//!   ~/.luna/logs/         — logs

use crate::error::{Error, Result};
use crate::tool::permissions::Rule;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// The `.luna` directory in the user's home directory.
pub fn luna_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".luna")
}

pub fn config_path() -> PathBuf {
    luna_dir().join("config.toml")
}

pub fn sessions_db_path() -> PathBuf {
    luna_dir().join("sessions.db")
}

pub fn logs_dir() -> PathBuf {
    luna_dir().join("logs")
}

/// Top-level `config.toml`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    /// Which enabled provider to use when none is specified.
    #[serde(default)]
    pub default_provider: String,
    #[serde(default)]
    pub providers: ProvidersConfig,
    #[serde(default)]
    pub agents: Vec<AgentSpecConfig>,
    #[serde(default)]
    pub permissions: PermissionsConfig,
    /// MCP servers to connect at startup: name → { command, args }.
    #[serde(default)]
    pub mcp_servers: std::collections::BTreeMap<String, McpServerConfig>,
}

/// One MCP stdio server definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProvidersConfig {
    #[serde(default)]
    pub anthropic: ProviderConfig,
    #[serde(default)]
    pub openai: ProviderConfig,
    #[serde(default)]
    pub google: ProviderConfig,
    #[serde(default)]
    pub xai: ProviderConfig,
    #[serde(default)]
    pub deepseek: ProviderConfig,
    #[serde(default)]
    pub mistral: ProviderConfig,
    #[serde(default)]
    pub openrouter: ProviderConfig,
    #[serde(default)]
    pub groq: ProviderConfig,
    #[serde(default)]
    pub ollama: OllamaConfig,
    /// Any additional OpenAI-compatible endpoints, under [providers.custom.<name>].
    #[serde(default)]
    pub custom: std::collections::BTreeMap<String, CustomProviderConfig>,
}

impl ProvidersConfig {
    /// All provider entries as a flattened view for UI listing.
    pub fn entries(&self) -> Vec<ProviderEntry> {
        let mut v = vec![
            entry("anthropic", &self.anthropic),
            entry("openai", &self.openai),
            entry("google", &self.google),
            entry("xai", &self.xai),
            entry("deepseek", &self.deepseek),
            entry("mistral", &self.mistral),
            entry("openrouter", &self.openrouter),
            entry("groq", &self.groq),
        ];
        if self.ollama.enabled {
            v.push(ProviderEntry {
                key: "ollama".into(),
                enabled: true,
                model: self.ollama.model.clone(),
                has_api_key: true, // local, no key needed
                needs_api_key: false,
            });
        }
        for (name, c) in &self.custom {
            v.push(ProviderEntry {
                key: name.clone(),
                enabled: true,
                model: c.model.clone(),
                has_api_key: !c.api_key.is_empty() || c.api_key_env.is_some(),
                needs_api_key: true,
            });
        }
        v
    }

    /// Look up a provider by key: built-ins first, then custom.
    pub fn get<'a>(&'a self, key: &str) -> Option<ProviderRef<'a>> {
        match key {
            "anthropic" => Some(ProviderRef::Builtin(&self.anthropic)),
            "openai" => Some(ProviderRef::Builtin(&self.openai)),
            "google" => Some(ProviderRef::Builtin(&self.google)),
            "xai" => Some(ProviderRef::Builtin(&self.xai)),
            "deepseek" => Some(ProviderRef::Builtin(&self.deepseek)),
            "mistral" => Some(ProviderRef::Builtin(&self.mistral)),
            "openrouter" => Some(ProviderRef::Builtin(&self.openrouter)),
            "groq" => Some(ProviderRef::Builtin(&self.groq)),
            "ollama" => self
                .ollama
                .enabled
                .then_some(ProviderRef::Ollama(&self.ollama)),
            other => self
                .custom
                .get(other)
                .map(|c| ProviderRef::Custom(other.to_string(), c)),
        }
    }
}

fn entry(key: &str, p: &ProviderConfig) -> ProviderEntry {
    ProviderEntry {
        key: key.into(),
        enabled: p.enabled,
        model: p.model.clone(),
        has_api_key: !p.api_key.is_empty(),
        needs_api_key: true,
    }
}

/// A flattened view of a provider for UIs.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ProviderEntry {
    pub key: String,
    pub enabled: bool,
    pub model: String,
    pub has_api_key: bool,
    pub needs_api_key: bool,
}

/// Borrowed provider reference for resolution.
#[derive(Debug, Clone)]
pub enum ProviderRef<'a> {
    Builtin(&'a ProviderConfig),
    Ollama(&'a OllamaConfig),
    /// Custom providers own their key (it comes from the map lookup).
    Custom(String, &'a CustomProviderConfig),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub model: String,
    /// API key. Falls back to the provider's standard env var when empty.
    #[serde(default)]
    pub api_key: String,
    /// Optional base URL override (defaults to the provider's official API).
    #[serde(default)]
    pub base_url: String,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
    #[serde(default = "default_temperature")]
    pub temperature: f32,
}

impl Default for ProviderConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            model: String::new(),
            api_key: String::new(),
            base_url: String::new(),
            max_tokens: default_max_tokens(),
            temperature: default_temperature(),
        }
    }
}

impl ProviderConfig {
    /// Effective API key: config value first, then the standard env var.
    pub fn effective_api_key(&self, env_var: &str) -> String {
        if !self.api_key.is_empty() {
            self.api_key.clone()
        } else {
            std::env::var(env_var).unwrap_or_default()
        }
    }

    /// Effective base URL: config override first, then the provider default.
    pub fn effective_base_url(&self, default_url: &str) -> String {
        if self.base_url.is_empty() {
            default_url.to_string()
        } else {
            self.base_url.clone()
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OllamaConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_ollama_url")]
    pub base_url: String,
    #[serde(default)]
    pub model: String,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
    #[serde(default = "default_temperature")]
    pub temperature: f32,
}

impl Default for OllamaConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            base_url: default_ollama_url(),
            model: String::new(),
            max_tokens: default_max_tokens(),
            temperature: default_temperature(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CustomProviderConfig {
    pub base_url: String,
    /// API key directly, or via `api_key_env` environment variable.
    #[serde(default)]
    pub api_key: String,
    pub api_key_env: Option<String>,
    #[serde(default)]
    pub model: String,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
    #[serde(default = "default_temperature")]
    pub temperature: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSpecConfig {
    pub name: String,
    /// Provider key: any enabled provider (built-in, ollama, or custom).
    pub provider: String,
    /// Optional model override; defaults to the provider's model.
    pub model: Option<String>,
    pub system: Option<String>,
    /// Tool names this agent may use; empty = all registered tools.
    #[serde(default)]
    pub tools: Vec<String>,
    #[serde(default = "default_max_iterations")]
    pub max_iterations: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PermissionsConfig {
    /// Extra rules merged after the built-in defaults.
    #[serde(default)]
    pub rules: Vec<Rule>,
    /// Tools that always require interactive approval.
    #[serde(default)]
    pub approval_tools: Vec<String>,
}

fn default_max_tokens() -> u32 {
    8192
}
fn default_temperature() -> f32 {
    0.7
}
fn default_ollama_url() -> String {
    "http://localhost:11434".into()
}
fn default_max_iterations() -> u32 {
    25
}

/// Known provider metadata: default base URL and env var name.
pub struct ProviderMeta {
    pub key: &'static str,
    pub label: &'static str,
    pub default_base_url: &'static str,
    pub env_var: &'static str,
    pub default_model: &'static str,
}

/// Metadata for every built-in provider (all OpenAI-compatible except Anthropic).
pub const PROVIDER_META: &[ProviderMeta] = &[
    ProviderMeta {
        key: "openai",
        label: "OpenAI",
        default_base_url: "https://api.openai.com/v1",
        env_var: "OPENAI_API_KEY",
        default_model: "gpt-5",
    },
    ProviderMeta {
        key: "google",
        label: "Google Gemini",
        default_base_url: "https://generativelanguage.googleapis.com/v1beta/openai",
        env_var: "GEMINI_API_KEY",
        default_model: "gemini-2.5-pro",
    },
    ProviderMeta {
        key: "xai",
        label: "xAI (Grok)",
        default_base_url: "https://api.x.ai/v1",
        env_var: "XAI_API_KEY",
        default_model: "grok-4",
    },
    ProviderMeta {
        key: "deepseek",
        label: "DeepSeek",
        default_base_url: "https://api.deepseek.com/v1",
        env_var: "DEEPSEEK_API_KEY",
        default_model: "deepseek-chat",
    },
    ProviderMeta {
        key: "mistral",
        label: "Mistral",
        default_base_url: "https://api.mistral.ai/v1",
        env_var: "MISTRAL_API_KEY",
        default_model: "mistral-large-latest",
    },
    ProviderMeta {
        key: "openrouter",
        label: "OpenRouter",
        default_base_url: "https://openrouter.ai/api/v1",
        env_var: "OPENROUTER_API_KEY",
        default_model: "anthropic/claude-sonnet-4.5",
    },
    ProviderMeta {
        key: "groq",
        label: "Groq",
        default_base_url: "https://api.groq.com/openai/v1",
        env_var: "GROQ_API_KEY",
        default_model: "llama-3.3-70b-versatile",
    },
];

impl Config {
    /// Load from `~/.luna/config.toml`; missing file = defaults.
    pub fn load() -> Result<Self> {
        Self::load_from(&config_path())
    }

    pub fn load_from(path: &std::path::Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = std::fs::read_to_string(path)?;
        let cfg: Config = toml::from_str(&text)?;
        cfg.validate()?;
        Ok(cfg)
    }

    /// Save back to `~/.luna/config.toml`.
    pub fn save(&self) -> Result<()> {
        self.save_to(&config_path())
    }

    pub fn save_to(&self, path: &std::path::Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = toml::to_string_pretty(self).map_err(|e| Error::Config(e.to_string()))?;
        std::fs::write(path, text)?;
        Ok(())
    }

    pub fn validate(&self) -> Result<()> {
        let mut names = std::collections::HashSet::new();
        for a in &self.agents {
            if !names.insert(a.name.as_str()) {
                return Err(Error::Config(format!("duplicate agent name `{}`", a.name)));
            }
            if a.provider.is_empty() {
                return Err(Error::Config(format!("agent `{}` has no provider", a.name)));
            }
        }
        Ok(())
    }

    /// The provider to use when none is specified: `default_provider` if
    /// enabled, else the first enabled provider.
    pub fn active_provider_key(&self) -> Option<String> {
        if !self.default_provider.is_empty() {
            let enabled = match self.providers.get(&self.default_provider) {
                Some(ProviderRef::Builtin(p)) => p.enabled,
                Some(ProviderRef::Ollama(o)) => o.enabled,
                Some(ProviderRef::Custom(_, _)) => true,
                None => false,
            };
            if enabled {
                return Some(self.default_provider.clone());
            }
        }
        // First enabled provider in listing order.
        for e in self.providers.entries() {
            if e.enabled {
                return Some(e.key);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_full_config() {
        let cfg: Config = toml::from_str(
            r#"
default_provider = "deepseek"

[providers.anthropic]
enabled = true
model = "claude-sonnet-4-5"
api_key = "sk-ant-test"

[providers.deepseek]
enabled = true
model = "deepseek-chat"
api_key = "sk-ds-test"

[providers.custom.custom_llama]
base_url = "http://my-server:8080/v1"
api_key = "none"
model = "llama3.2"

[providers.ollama]
enabled = true
model = "qwen3"
"#,
        )
        .unwrap();
        assert_eq!(cfg.default_provider, "deepseek");
        assert_eq!(cfg.providers.anthropic.api_key, "sk-ant-test");
        assert_eq!(cfg.providers.custom.len(), 1);
        assert_eq!(cfg.active_provider_key(), Some("deepseek".into()));
        let entries = cfg.providers.entries();
        assert!(entries.iter().any(|e| e.key == "custom_llama" && e.enabled));
        assert!(entries.iter().any(|e| e.key == "ollama" && e.enabled));
    }

    #[test]
    fn active_provider_falls_back_to_first_enabled() {
        let cfg: Config = toml::from_str(
            r#"
[providers.openai]
enabled = true
"#,
        )
        .unwrap();
        assert_eq!(cfg.active_provider_key(), Some("openai".into()));
    }

    #[test]
    fn missing_file_yields_defaults() {
        let cfg =
            Config::load_from(std::path::Path::new("Z:/definitely/missing/luna.toml")).unwrap();
        assert!(cfg.agents.is_empty());
        assert_eq!(cfg.active_provider_key(), None);
    }

    #[test]
    fn save_and_reload_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let cfg = Config {
            default_provider: "groq".into(),
            providers: ProvidersConfig {
                groq: ProviderConfig {
                    enabled: true,
                    model: "llama-3.3-70b-versatile".into(),
                    api_key: "gsk_test".into(),
                    ..Default::default()
                },
                ..Default::default()
            },
            ..Default::default()
        };
        cfg.save_to(&path).unwrap();
        let loaded = Config::load_from(&path).unwrap();
        assert_eq!(loaded.default_provider, "groq");
        assert_eq!(loaded.providers.groq.api_key, "gsk_test");
        assert_eq!(loaded.active_provider_key(), Some("groq".into()));
    }

    #[test]
    fn effective_key_prefers_config_over_env() {
        let p = ProviderConfig {
            api_key: "from-config".into(),
            ..Default::default()
        };
        // Note: not touching env vars here (remove_var is unsafe in edition 2024
        // and parallel tests make env mutation flaky).
        assert_eq!(p.effective_api_key("LUNA_TEST_KEY_XYZ"), "from-config");
    }
}
