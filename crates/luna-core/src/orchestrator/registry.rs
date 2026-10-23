//! Agent registry: named agent specs, TOML-backed.

use super::agent_loop::AgentSpec;
use crate::config::AgentSpecConfig;
use crate::error::{Error, Result};
use std::collections::HashMap;

/// Resolved runtime settings for a provider key.
#[derive(Debug, Clone)]
pub struct ProviderSettings {
    pub model: String,
    pub max_tokens: u32,
    pub temperature: f32,
}

/// Maps agent names to concrete specs using config + provider settings.
#[derive(Debug, Default, Clone)]
pub struct AgentRegistry {
    specs: HashMap<String, AgentSpecConfig>,
    providers: HashMap<String, ProviderSettings>,
}

impl AgentRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register_provider(&mut self, key: &str, settings: ProviderSettings) {
        self.providers.insert(key.to_string(), settings);
    }

    pub fn register_agent(&mut self, cfg: AgentSpecConfig) {
        self.specs.insert(cfg.name.clone(), cfg);
    }

    pub fn agent_names(&self) -> Vec<String> {
        let mut v: Vec<String> = self.specs.keys().cloned().collect();
        v.sort();
        v
    }

    /// Resolve an agent spec to a runnable [`AgentSpec`].
    pub fn resolve(&self, name: &str) -> Result<AgentSpec> {
        let cfg = self
            .specs
            .get(name)
            .ok_or_else(|| Error::Orchestrator(format!("unknown agent `{name}`")))?;
        let ps = self.providers.get(&cfg.provider).ok_or_else(|| {
            Error::Orchestrator(format!(
                "agent `{}` references provider `{}` which is not enabled",
                cfg.name, cfg.provider
            ))
        })?;
        Ok(AgentSpec {
            name: cfg.name.clone(),
            provider_key: cfg.provider.clone(),
            model: cfg.model.clone().unwrap_or_else(|| ps.model.clone()),
            system: cfg.system.clone(),
            tools: cfg.tools.clone(),
            max_iterations: cfg.max_iterations,
            max_tokens: ps.max_tokens,
            temperature: ps.temperature,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(name: &str, provider: &str) -> AgentSpecConfig {
        AgentSpecConfig {
            name: name.into(),
            provider: provider.into(),
            model: None,
            system: Some("be careful".into()),
            tools: vec!["read_file".into()],
            max_iterations: 10,
        }
    }

    #[test]
    fn resolves_agent_with_provider_settings() {
        let mut reg = AgentRegistry::new();
        reg.register_provider(
            "openai",
            ProviderSettings { model: "gpt-5".into(), max_tokens: 4096, temperature: 0.5 },
        );
        reg.register_agent(cfg("coder", "openai"));
        let spec = reg.resolve("coder").unwrap();
        assert_eq!(spec.model, "gpt-5");
        assert_eq!(spec.max_tokens, 4096);
        assert_eq!(spec.tools, vec!["read_file".to_string()]);
    }

    #[test]
    fn unknown_agent_errors() {
        let reg = AgentRegistry::new();
        assert!(reg.resolve("nope").is_err());
    }

    #[test]
    fn missing_provider_errors() {
        let mut reg = AgentRegistry::new();
        reg.register_agent(cfg("coder", "anthropic"));
        assert!(reg.resolve("coder").is_err());
    }
}
