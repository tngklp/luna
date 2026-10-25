//! Provider abstraction.
//!
//! Every backend is reduced to one operation: given a conversation, produce a
//! stream of deltas. Everything provider-specific — retries, auth headers,
//! request shape, tool-call encoding — stays behind [`Provider`], so the agent
//! loop never branches on which vendor is in use.

pub mod anthropic;
pub mod gemini;
pub mod openai;
pub mod stream;

use std::collections::BTreeMap;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use futures_util::Stream;

use crate::catalog;
use crate::config::{Config, ProviderKind};
use crate::error::{LunaError, Result};
use crate::types::{Message, ModelInfo, StopReason, ThinkingLevel, ToolCall, ToolSchema, Usage};

/// Everything needed to make one model call.
#[derive(Debug, Clone)]
pub struct ChatRequest {
    pub model: String,
    pub system: Option<String>,
    pub messages: Vec<Message>,
    pub tools: Vec<ToolSchema>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub thinking: ThinkingLevel,
}

impl ChatRequest {
    pub fn new(model: impl Into<String>, messages: Vec<Message>) -> Self {
        ChatRequest {
            model: model.into(),
            system: None,
            messages,
            tools: Vec::new(),
            temperature: None,
            max_tokens: None,
            thinking: ThinkingLevel::Medium,
        }
    }
}

/// What a provider stream can yield.
#[derive(Debug, Clone)]
pub enum ProviderEvent {
    TextDelta(String),
    ThinkingDelta(String),
    /// A tool call began. `index` is the provider's own ordering.
    ToolCallStart {
        index: usize,
        id: String,
        name: String,
    },
    /// A fragment of the tool call's JSON arguments.
    ToolCallArgsDelta {
        index: usize,
        delta: String,
    },
    ToolCallEnd {
        index: usize,
        call: ToolCall,
    },
    Usage(Usage),
    Done(StopReason),
}

pub type ProviderStream = Pin<Box<dyn Stream<Item = Result<ProviderEvent>> + Send>>;

#[async_trait]
pub trait Provider: Send + Sync {
    fn name(&self) -> &str;

    fn kind(&self) -> ProviderKind;

    /// Start a streaming completion.
    ///
    /// Dropping the returned stream must abort the network request; the agent
    /// loop relies on that for cancellation.
    async fn stream(&self, request: ChatRequest) -> Result<ProviderStream>;

    /// Models the provider advertises, when the API supports listing.
    async fn list_models(&self) -> Result<Vec<String>> {
        Ok(Vec::new())
    }

    /// Whether this provider can be called at all (a key is present, or none is
    /// needed because it is a local endpoint).
    fn ready(&self) -> bool {
        true
    }
}

const MAX_ATTEMPTS: u32 = 4;

fn backoff(attempt: u32) -> Duration {
    // 250ms, 500ms, 1s, 2s with a little jitter to avoid thundering herds.
    let base = 250u64 << attempt.min(4);
    let jitter = (std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_millis() as u64)
        .unwrap_or(0))
        % 100;
    Duration::from_millis(base + jitter)
}

/// Pull a human-readable message out of a provider error body.
pub(crate) fn extract_error_message(body: &str) -> String {
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(body) {
        if let Some(message) = value
            .get("error")
            .and_then(|error| error.get("message"))
            .and_then(|message| message.as_str())
        {
            return message.to_string();
        }
        if let Some(message) = value.get("message").and_then(|message| message.as_str()) {
            return message.to_string();
        }
        if let Some(errors) = value.get("errors").and_then(|errors| errors.as_array()) {
            if let Some(first) = errors.first().and_then(|error| error.as_str()) {
                return first.to_string();
            }
        }
    }
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return "the provider returned an empty error body".to_string();
    }
    trimmed.chars().take(400).collect()
}

fn retryable_status(status: u16) -> bool {
    matches!(status, 408 | 409 | 425 | 429 | 500 | 502 | 503 | 504 | 529)
}

/// Send a request, retrying transient failures with exponential backoff.
pub(crate) async fn send_retrying(
    builder: reqwest::RequestBuilder,
    provider: &str,
) -> Result<reqwest::Response> {
    let mut last: Option<LunaError> = None;
    for attempt in 0..MAX_ATTEMPTS {
        let attempt_builder = match builder.try_clone() {
            Some(clone) => clone,
            None => {
                return Err(LunaError::Provider {
                    provider: provider.to_string(),
                    status: 0,
                    message: "request body could not be cloned for a retry".to_string(),
                    retryable: false,
                })
            }
        };
        match attempt_builder.send().await {
            Ok(response) if response.status().is_success() => return Ok(response),
            Ok(response) => {
                let status = response.status().as_u16();
                let body = response.text().await.unwrap_or_default();
                let error = LunaError::Provider {
                    provider: provider.to_string(),
                    status,
                    message: extract_error_message(&body),
                    retryable: retryable_status(status),
                };
                if error.retryable() && attempt + 1 < MAX_ATTEMPTS {
                    tracing::warn!("{provider}: {status}, retrying (attempt {})", attempt + 1);
                    last = Some(error);
                    tokio::time::sleep(backoff(attempt)).await;
                    continue;
                }
                return Err(error);
            }
            Err(err) => {
                let retryable = err.is_timeout() || err.is_connect();
                let error = LunaError::Provider {
                    provider: provider.to_string(),
                    status: 0,
                    message: err.to_string(),
                    retryable,
                };
                if retryable && attempt + 1 < MAX_ATTEMPTS {
                    last = Some(error);
                    tokio::time::sleep(backoff(attempt)).await;
                    continue;
                }
                return Err(error);
            }
        }
    }
    Err(last.unwrap_or_else(|| LunaError::Provider {
        provider: provider.to_string(),
        status: 0,
        message: "request failed after every retry".to_string(),
        retryable: false,
    }))
}

/// A shared HTTP client. One pool for the whole process.
pub fn http_client() -> reqwest::Client {
    static CLIENT: once_cell::sync::Lazy<reqwest::Client> = once_cell::sync::Lazy::new(|| {
        reqwest::Client::builder()
            // Streaming responses can idle between deltas; a short idle timeout
            // would cut long reasoning turns off mid-sentence.
            .connect_timeout(Duration::from_secs(20))
            .pool_idle_timeout(Duration::from_secs(90))
            .pool_max_idle_per_host(8)
            .tcp_nodelay(true)
            .build()
            .unwrap_or_default()
    });
    CLIENT.clone()
}

/// Build every configured provider once, up front.
pub struct ProviderRegistry {
    providers: BTreeMap<String, Arc<dyn Provider>>,
    config: Config,
}

impl ProviderRegistry {
    pub fn new(config: &Config) -> ProviderRegistry {
        let mut providers: BTreeMap<String, Arc<dyn Provider>> = BTreeMap::new();
        for (name, settings) in &config.providers {
            let provider: Arc<dyn Provider> = match settings.kind {
                ProviderKind::Openai => Arc::new(openai::OpenAiProvider::new(name, settings)),
                ProviderKind::Anthropic => {
                    Arc::new(anthropic::AnthropicProvider::new(name, settings))
                }
                ProviderKind::Gemini => Arc::new(gemini::GeminiProvider::new(name, settings)),
            };
            providers.insert(name.clone(), provider);
        }
        ProviderRegistry { providers, config: config.clone() }
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Register a provider directly, replacing any configured one with the same
    /// name. Used by tests and by embedders that supply their own transport.
    pub fn with_provider(mut self, name: impl Into<String>, provider: Arc<dyn Provider>) -> Self {
        self.providers.insert(name.into(), provider);
        self
    }

    pub fn names(&self) -> Vec<String> {
        self.providers.keys().cloned().collect()
    }

    pub fn get(&self, name: &str) -> Result<Arc<dyn Provider>> {
        self.providers.get(name).cloned().ok_or_else(|| {
            LunaError::Config(format!(
                "unknown provider `{name}`; configured: {}",
                self.providers.keys().cloned().collect::<Vec<_>>().join(", ")
            ))
        })
    }

    /// Resolve a `provider/model` reference to a provider and bare model id.
    pub fn resolve(&self, reference: &str) -> Result<(Arc<dyn Provider>, String)> {
        let (provider_name, model) = Config::split_model(reference);
        Ok((self.get(&provider_name)?, model))
    }

    /// Everything the model picker should offer: the catalogue plus anything the
    /// user declared in config.
    pub fn models(&self) -> Vec<ModelInfo> {
        let mut models: Vec<ModelInfo> = Vec::new();
        let mut seen: std::collections::HashSet<(String, String)> =
            std::collections::HashSet::new();
        for (provider_name, settings) in &self.config.providers {
            for model_id in &settings.models {
                if seen.insert((provider_name.clone(), model_id.clone())) {
                    models.push(catalog::describe(provider_name, model_id));
                }
            }
            for known in catalog::for_provider(provider_name) {
                if seen.insert((provider_name.clone(), known.id.clone())) {
                    models.push(known);
                }
            }
        }
        models.sort_by(|a, b| a.provider.cmp(&b.provider).then(a.name.cmp(&b.name)));
        models
    }

    pub fn model_info(&self, reference: &str) -> ModelInfo {
        let (provider, model) = Config::split_model(reference);
        catalog::describe(&provider, &model)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_provider_prefixed_references() {
        assert_eq!(
            Config::split_model("anthropic/claude-sonnet-4-5"),
            ("anthropic".to_string(), "claude-sonnet-4-5".to_string())
        );
    }

    #[test]
    fn bare_model_names_become_local() {
        assert_eq!(Config::split_model("llama3.2"), ("local".to_string(), "llama3.2".to_string()));
    }

    #[test]
    fn error_bodies_are_reduced_to_a_message() {
        let body = r#"{"error":{"message":"rate limited"}}"#;
        assert_eq!(extract_error_message(body), "rate limited");
    }

    #[test]
    fn plain_text_errors_pass_through() {
        assert_eq!(extract_error_message("  boom  "), "boom");
    }

    #[test]
    fn registry_resolves_configured_providers() {
        let registry = ProviderRegistry::new(&Config::default());
        assert!(registry.get("anthropic").is_ok());
        assert!(registry.get("nope").is_err());
    }
}
