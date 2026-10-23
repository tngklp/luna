//! TOML configuration: providers, agents, tool permissions.

use crate::error::{Error, Result};
use crate::tool::permissions::Rule;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Top-level `config.toml`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub providers: ProvidersConfig,
    #[serde(default)]
    pub agents: Vec<AgentSpecConfig>,
    #[serde(default)]
    pub permissions: PermissionsConfig,
    #[serde(default)]
    pub storage: StorageConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProvidersConfig {
    #[serde(default)]
    pub anthropic: ProviderConfig,
    #[serde(default)]
    pub openai: ProviderConfig,
    #[serde(default)]
    pub ollama: OllamaConfig,
    /// Any additional OpenAI-compatible endpoints.
    #[serde(default)]
    pub custom: std::collections::BTreeMap<String, CustomProviderConfig>,
}

impl Default for ProvidersConfig {
    fn default() -> Self {
        Self {
            anthropic: ProviderConfig { enabled: false, model: "claude-sonnet-4-5".into(), max_tokens: 8192, temperature: 0.7 },
            openai: ProviderConfig { enabled: false, model: "gpt-5".into(), max_tokens: 8192, temperature: 0.7 },
            ollama: OllamaConfig { enabled: false, base_url: "http://localhost:11434".into(), model: "llama3.2".into(), max_tokens: 8192, temperature: 0.7 },
            custom: Default::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub model: String,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
    #[serde(default = "default_temperature")]
    pub temperature: f32,
}

impl Default for ProviderConfig {
    fn default() -> Self {
        Self { enabled: false, model: String::new(), max_tokens: default_max_tokens(), temperature: default_temperature() }
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomProviderConfig {
    pub base_url: String,
    pub api_key_env: Option<String>,
    pub model: String,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
    #[serde(default = "default_temperature")]
    pub temperature: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSpecConfig {
    pub name: String,
    /// Provider key: "anthropic" | "openai" | "ollama" | custom key.
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    /// Directory for SQLite session databases.
    #[serde(default = "default_data_dir")]
    pub data_dir: PathBuf,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self { data_dir: default_data_dir() }
    }
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
fn default_data_dir() -> PathBuf {
    directories::ProjectDirs::from("dev", "luna", "Luna")
        .map(|d| d.data_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from(".luna"))
}

impl Config {
    /// Load from `config.toml` in the platform config dir; missing file = defaults.
    pub fn load() -> Result<Self> {
        let path = Self::default_path();
        Self::load_from(&path)
    }

    pub fn default_path() -> PathBuf {
        directories::ProjectDirs::from("dev", "luna", "Luna")
            .map(|d| d.config_dir().join("config.toml"))
            .unwrap_or_else(|| PathBuf::from("config.toml"))
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

    /// Write a commented starter config to `path` if it doesn't exist.
    pub fn write_default(path: &std::path::Path) -> Result<()> {
        if path.exists() {
            return Ok(());
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, DEFAULT_CONFIG_TOML)?;
        Ok(())
    }
}

const DEFAULT_CONFIG_TOML: &str = r#"# Luna configuration.
# Providers are enabled here; API keys come from environment variables:
#   ANTHROPIC_API_KEY, OPENAI_API_KEY

[providers.anthropic]
enabled = false
model = "claude-sonnet-4-5"

[providers.openai]
enabled = false
model = "gpt-5"

[providers.ollama]
enabled = false
base_url = "http://localhost:11434"
model = "llama3.2"

# Example agent:
# [[agents]]
# name = "coder"
# provider = "anthropic"
# system = "You are a careful coding agent."
# tools = ["read_file", "write_file", "glob", "shell"]

[permissions]
approval_tools = ["shell", "write_file"]

[storage]
data_dir = "C:\\Users\\Tuna\\AppData\\Local\\luna\\data"
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_default_toml() {
        let cfg: Config = toml::from_str(DEFAULT_CONFIG_TOML).unwrap();
        assert!(!cfg.providers.anthropic.enabled);
        assert_eq!(cfg.providers.ollama.model, "llama3.2");
        assert!(cfg.permissions.approval_tools.contains(&"shell".into()));
    }

    #[test]
    fn missing_file_yields_defaults() {
        let cfg = Config::load_from(std::path::Path::new("Z:/definitely/missing/luna.toml")).unwrap();
        assert!(cfg.agents.is_empty());
    }

    #[test]
    fn rejects_duplicate_agents() {
        let cfg: Config = toml::from_str(
            r#"
[[agents]]
name = "a"
provider = "openai"

[[agents]]
name = "a"
provider = "openai"
"#,
        )
        .unwrap();
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn parses_agent_specs() {
        let cfg: Config = toml::from_str(
            r#"
[providers.openai]
enabled = true
model = "gpt-5"

[[agents]]
name = "coder"
provider = "openai"
tools = ["read_file"]
"#,
        )
        .unwrap();
        assert_eq!(cfg.agents.len(), 1);
        assert_eq!(cfg.agents[0].name, "coder");
        assert_eq!(cfg.agents[0].max_iterations, 25);
    }
}
