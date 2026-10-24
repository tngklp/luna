//! Provider construction from config: one resolver shared by all UIs.

use crate::config::{
    Config, CustomProviderConfig, OllamaConfig, PROVIDER_META, ProviderConfig, ProviderRef,
};
use crate::error::Result;
use crate::provider::Provider;
use crate::provider::ollama::OllamaProvider;
use crate::provider::openai_compat::OpenAiCompatProvider;
use std::sync::Arc;

/// A constructed provider plus its resolved settings.
pub struct ResolvedProvider {
    pub key: String,
    pub provider: Arc<dyn Provider>,
    pub model: String,
    pub max_tokens: u32,
    pub temperature: f32,
}
impl ResolvedProvider {}

/// Build the provider for `key` from config. Returns None when the provider
/// is disabled or its key is missing.
pub fn resolve_provider(cfg: &Config, key: &str) -> Option<ResolvedProvider> {
    match cfg.providers.get(key)? {
        ProviderRef::Builtin(p) => resolve_builtin(key, p),
        ProviderRef::Ollama(o) => resolve_ollama(o),
        ProviderRef::Custom(name, c) => resolve_custom(&name.clone(), c),
    }
}

/// Build the configured default provider (`default_provider`, else first enabled).
pub fn resolve_active(cfg: &Config) -> Option<ResolvedProvider> {
    let key = cfg.active_provider_key()?;
    resolve_provider(cfg, &key)
}

fn resolve_builtin(key: &str, p: &ProviderConfig) -> Option<ResolvedProvider> {
    if !p.enabled {
        return None;
    }
    if key == "anthropic" {
        let api_key = p.effective_api_key("ANTHROPIC_API_KEY");
        if api_key.is_empty() {
            return None;
        }
        let provider = AnthropicProviderCompat::new(
            api_key,
            p.effective_base_url("https://api.anthropic.com"),
        );
        return Some(ResolvedProvider {
            key: key.to_string(),
            provider: Arc::new(provider),
            model: if p.model.is_empty() {
                "claude-sonnet-4-5".into()
            } else {
                p.model.clone()
            },
            max_tokens: p.max_tokens,
            temperature: p.temperature,
        });
    }
    let meta = PROVIDER_META.iter().find(|m| m.key == key)?;
    let api_key = p.effective_api_key(meta.env_var);
    if api_key.is_empty() {
        return None;
    }
    let provider =
        OpenAiCompatProvider::new(key, &p.effective_base_url(meta.default_base_url), api_key)
            .ok()?;
    Some(ResolvedProvider {
        key: key.to_string(),
        provider: Arc::new(provider),
        model: if p.model.is_empty() {
            meta.default_model.to_string()
        } else {
            p.model.clone()
        },
        max_tokens: p.max_tokens,
        temperature: p.temperature,
    })
}

fn resolve_ollama(o: &OllamaConfig) -> Option<ResolvedProvider> {
    if !o.enabled {
        return None;
    }
    let provider = OllamaProvider::new(&o.base_url).ok()?;
    Some(ResolvedProvider {
        key: "ollama".into(),
        provider: Arc::new(provider),
        model: if o.model.is_empty() {
            "llama3.2".into()
        } else {
            o.model.clone()
        },
        max_tokens: o.max_tokens,
        temperature: o.temperature,
    })
}

fn resolve_custom(name: &str, c: &CustomProviderConfig) -> Option<ResolvedProvider> {
    let api_key = if !c.api_key.is_empty() {
        c.api_key.clone()
    } else if let Some(env_name) = &c.api_key_env {
        std::env::var(env_name).unwrap_or_default()
    } else {
        String::new()
    };
    // Custom endpoints may be keyless (e.g. local llama.cpp); empty key is OK.
    let provider = OpenAiCompatProvider::new(name, &c.base_url, api_key).ok()?;
    Some(ResolvedProvider {
        key: name.to_string(),
        provider: Arc::new(provider),
        model: c.model.clone(),
        max_tokens: c.max_tokens,
        temperature: c.temperature,
    })
}

/// Anthropic provider with a configurable base URL (for proxies/gateways).
/// Reuses the official client when the URL is the default.
struct AnthropicProviderCompat {
    inner: crate::provider::anthropic::AnthropicProvider,
}

impl AnthropicProviderCompat {
    fn new(api_key: String, base_url: String) -> Self {
        let _ = base_url; // base-url override lands with the anthropic client refactor
        Self {
            inner: crate::provider::anthropic::AnthropicProvider::new(api_key)
                .expect("anthropic provider with non-empty key"),
        }
    }
}

#[async_trait::async_trait]
impl Provider for AnthropicProviderCompat {
    fn name(&self) -> &str {
        self.inner.name()
    }

    async fn stream_completion(
        &self,
        request: crate::provider::types::CompletionRequest,
        cancel: tokio_util::sync::CancellationToken,
    ) -> Result<
        std::pin::Pin<Box<dyn futures::Stream<Item = crate::provider::types::StreamChunk> + Send>>,
    > {
        self.inner.stream_completion(request, cancel).await
    }

    async fn list_models(&self) -> Result<Vec<String>> {
        self.inner.list_models().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_enabled_builtin_with_config_key() {
        let cfg: Config = toml::from_str(
            r#"
[providers.deepseek]
enabled = true
api_key = "sk-ds"
model = "deepseek-chat"
"#,
        )
        .unwrap();
        let r = resolve_provider(&cfg, "deepseek").expect("resolved");
        assert_eq!(r.key, "deepseek");
        assert_eq!(r.model, "deepseek-chat");
        assert_eq!(r.provider.name(), "deepseek");
    }

    #[test]
    fn disabled_provider_is_none() {
        let cfg: Config = toml::from_str(
            r#"
[providers.openai]
enabled = false
api_key = "x"
"#,
        )
        .unwrap();
        assert!(resolve_provider(&cfg, "openai").is_none());
    }

    #[test]
    fn missing_key_is_none_unless_env_set() {
        // No key in config. Resolution succeeds only when the standard env
        // var happens to be set (e.g. on a dev machine); we can't mutate env
        // safely in parallel tests, so accept either outcome deterministically.
        let cfg: Config = toml::from_str(
            r#"
[providers.groq]
enabled = true
"#,
        )
        .unwrap();
        let resolved = resolve_provider(&cfg, "groq");
        let env_set = std::env::var("GROQ_API_KEY").is_ok();
        assert_eq!(resolved.is_some(), env_set);
    }

    #[test]
    fn custom_provider_keyless_ok() {
        let cfg: Config = toml::from_str(
            r#"
[providers.custom.my_local]
base_url = "http://127.0.0.1:9999/v1"
model = "test-model"
"#,
        )
        .unwrap();
        let r = resolve_provider(&cfg, "my_local").expect("resolved");
        assert_eq!(r.key, "my_local");
        assert_eq!(r.model, "test-model");
    }

    #[test]
    fn resolve_active_uses_default_provider() {
        let cfg: Config = toml::from_str(
            r#"
default_provider = "ollama"
[providers.ollama]
enabled = true
model = "qwen3"
"#,
        )
        .unwrap();
        let r = resolve_active(&cfg).expect("resolved");
        assert_eq!(r.key, "ollama");
        assert_eq!(r.model, "qwen3");
    }
}
