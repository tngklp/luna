//! Built-in provider presets.
//!
//! Most providers speak the OpenAI Chat Completions dialect, so they need only a
//! base URL, a key variable and a model list. Keeping them here means a user can
//! write `preset = "groq"` instead of remembering the endpoint, and the config
//! file stays readable.
//!
//! Anything not listed is still supported: a `[providers.<name>]` table with
//! `kind`, `base_url` and `api_key_env` is a custom provider.

use std::collections::BTreeMap;

use crate::config::{ProviderConfig, ProviderKind};

/// One known provider.
#[derive(Debug, Clone, Copy)]
pub struct Preset {
    /// The name used in `provider/model` references.
    pub id: &'static str,
    /// Human-readable name, for the picker.
    pub label: &'static str,
    pub kind: ProviderKind,
    pub base_url: &'static str,
    /// Environment variable the key is read from. Empty means no key is needed.
    pub key_env: &'static str,
    /// Models to offer before discovery runs.
    pub models: &'static [&'static str],
    /// Whether the endpoint can list its own models.
    pub discover: bool,
}

const fn preset(
    id: &'static str,
    label: &'static str,
    kind: ProviderKind,
    base_url: &'static str,
    key_env: &'static str,
    models: &'static [&'static str],
    discover: bool,
) -> Preset {
    Preset { id, label, kind, base_url, key_env, models, discover }
}

/// Every provider Luna knows how to talk to out of the box.
pub const PRESETS: &[Preset] = &[
    preset(
        "anthropic",
        "Anthropic",
        ProviderKind::Anthropic,
        "https://api.anthropic.com",
        "ANTHROPIC_API_KEY",
        &["claude-sonnet-4-5", "claude-opus-4-1", "claude-haiku-4-5"],
        true,
    ),
    preset(
        "openai",
        "OpenAI",
        ProviderKind::Openai,
        "https://api.openai.com/v1",
        "OPENAI_API_KEY",
        &["gpt-5", "gpt-5-mini", "gpt-4.1"],
        true,
    ),
    preset(
        "gemini",
        "Google Gemini",
        ProviderKind::Gemini,
        "https://generativelanguage.googleapis.com/v1beta",
        "GEMINI_API_KEY",
        &["gemini-2.5-pro", "gemini-2.5-flash"],
        true,
    ),
    preset(
        "openrouter",
        "OpenRouter",
        ProviderKind::Openai,
        "https://openrouter.ai/api/v1",
        "OPENROUTER_API_KEY",
        &[
            "anthropic/claude-sonnet-4.5",
            "openai/gpt-5",
            "google/gemini-2.5-pro",
            "deepseek/deepseek-chat",
        ],
        true,
    ),
    preset(
        "groq",
        "Groq",
        ProviderKind::Openai,
        "https://api.groq.com/openai/v1",
        "GROQ_API_KEY",
        &["llama-3.3-70b-versatile", "moonshotai/kimi-k2-instruct"],
        true,
    ),
    preset(
        "deepseek",
        "DeepSeek",
        ProviderKind::Openai,
        "https://api.deepseek.com/v1",
        "DEEPSEEK_API_KEY",
        &["deepseek-chat", "deepseek-reasoner"],
        true,
    ),
    preset(
        "mistral",
        "Mistral",
        ProviderKind::Openai,
        "https://api.mistral.ai/v1",
        "MISTRAL_API_KEY",
        &["mistral-large-latest", "codestral-latest"],
        true,
    ),
    preset(
        "xai",
        "xAI",
        ProviderKind::Openai,
        "https://api.x.ai/v1",
        "XAI_API_KEY",
        &["grok-4", "grok-code-fast-1"],
        true,
    ),
    preset(
        "together",
        "Together AI",
        ProviderKind::Openai,
        "https://api.together.xyz/v1",
        "TOGETHER_API_KEY",
        &["moonshotai/Kimi-K2-Instruct", "Qwen/Qwen3-Coder-480B-A35B-Instruct"],
        true,
    ),
    preset(
        "fireworks",
        "Fireworks AI",
        ProviderKind::Openai,
        "https://api.fireworks.ai/inference/v1",
        "FIREWORKS_API_KEY",
        &["accounts/fireworks/models/kimi-k2-instruct"],
        true,
    ),
    preset(
        "cerebras",
        "Cerebras",
        ProviderKind::Openai,
        "https://api.cerebras.ai/v1",
        "CEREBRAS_API_KEY",
        &["llama-3.3-70b"],
        true,
    ),
    preset(
        "perplexity",
        "Perplexity",
        ProviderKind::Openai,
        "https://api.perplexity.ai",
        "PERPLEXITY_API_KEY",
        &["sonar-pro", "sonar-reasoning-pro"],
        false,
    ),
    preset(
        "zai",
        "Z.ai (GLM)",
        ProviderKind::Openai,
        "https://api.z.ai/api/paas/v4",
        "ZAI_API_KEY",
        &["glm-4.6", "glm-4.5-air"],
        false,
    ),
    preset(
        "moonshot",
        "Moonshot (Kimi)",
        ProviderKind::Openai,
        "https://api.moonshot.ai/v1",
        "MOONSHOT_API_KEY",
        &["kimi-k2-0905-preview"],
        true,
    ),
    preset(
        "ollama",
        "Ollama (local)",
        ProviderKind::Openai,
        "http://localhost:11434/v1",
        "",
        &["llama3.2", "qwen2.5-coder"],
        true,
    ),
    preset(
        "llamacpp",
        "llama.cpp (local)",
        ProviderKind::Openai,
        "http://localhost:8080/v1",
        "",
        &[],
        true,
    ),
    preset(
        "lmstudio",
        "LM Studio (local)",
        ProviderKind::Openai,
        "http://localhost:1234/v1",
        "",
        &[],
        true,
    ),
    preset("vllm", "vLLM (local)", ProviderKind::Openai, "http://localhost:8000/v1", "", &[], true),
];

/// Look a preset up by id.
pub fn find(id: &str) -> Option<&'static Preset> {
    PRESETS.iter().find(|preset| preset.id == id)
}

impl Preset {
    /// Build the config entry for this preset.
    pub fn to_config(self) -> ProviderConfig {
        ProviderConfig {
            preset: Some(self.id.to_string()),
            kind: self.kind,
            base_url: Some(self.base_url.to_string()),
            api_key: None,
            api_key_env: if self.key_env.is_empty() {
                None
            } else {
                Some(self.key_env.to_string())
            },
            headers: BTreeMap::new(),
            models: self.models.iter().map(|model| model.to_string()).collect(),
            discover_models: self.discover,
        }
    }

    /// Whether this preset needs a key to work.
    pub fn needs_key(self) -> bool {
        !self.key_env.is_empty()
    }
}

/// The provider table a fresh config starts with.
///
/// Only the three first-party providers and the local one are enabled by
/// default: listing every preset would make the file noisy and imply the user
/// has keys for all of them. The rest are one `preset = "..."` line away.
pub fn default_providers() -> BTreeMap<String, ProviderConfig> {
    let mut providers = BTreeMap::new();
    for id in ["anthropic", "openai", "gemini", "ollama"] {
        if let Some(preset) = find(id) {
            providers.insert(id.to_string(), preset.to_config());
        }
    }
    providers
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_preset_has_a_unique_id() {
        let mut seen = std::collections::HashSet::new();
        for preset in PRESETS {
            assert!(seen.insert(preset.id), "duplicate preset id {}", preset.id);
        }
    }

    #[test]
    fn every_preset_has_a_usable_base_url() {
        for preset in PRESETS {
            assert!(preset.base_url.starts_with("http"), "{} has a bad base url", preset.id);
        }
    }

    #[test]
    fn local_presets_need_no_key() {
        for id in ["ollama", "llamacpp", "lmstudio", "vllm"] {
            let preset = find(id).unwrap();
            assert!(!preset.needs_key(), "{id} should not need a key");
            assert!(preset.to_config().api_key_env.is_none());
        }
    }

    #[test]
    fn hosted_presets_name_a_key_variable() {
        for id in ["anthropic", "openai", "gemini", "groq", "deepseek", "xai"] {
            let preset = find(id).unwrap();
            assert!(preset.needs_key(), "{id} should need a key");
            assert!(preset.key_env.ends_with("_API_KEY"));
        }
    }

    #[test]
    fn the_default_table_covers_the_first_party_providers() {
        let providers = default_providers();
        for id in ["anthropic", "openai", "gemini", "ollama"] {
            assert!(providers.contains_key(id), "missing {id}");
        }
    }

    #[test]
    fn presets_convert_to_config() {
        let config = find("groq").unwrap().to_config();
        assert_eq!(config.kind, ProviderKind::Openai);
        assert_eq!(config.base_url.as_deref(), Some("https://api.groq.com/openai/v1"));
        assert_eq!(config.api_key_env.as_deref(), Some("GROQ_API_KEY"));
        assert!(!config.models.is_empty());
    }

    #[test]
    fn unknown_presets_are_absent() {
        assert!(find("not-a-provider").is_none());
    }
}
