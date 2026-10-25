//! End-to-end image tests.
//!
//! These drive the real agent loop with a scripted provider, so they prove the
//! image reaches the request rather than merely that the encoder compiles.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde_json::json;

use luna_core::agent::{AgentDeps, AgentSession};
use luna_core::config::{Config, ProviderConfig, ProviderKind};
use luna_core::event::{AgentRef, Emitted, EventSink};
use luna_core::provider::{ChatRequest, Provider, ProviderEvent, ProviderRegistry, ProviderStream};
use luna_core::types::{ContentBlock, Message, Role, StopReason};

/// Records the requests it receives and answers with fixed text.
struct Recorder {
    seen: Mutex<Vec<ChatRequest>>,
}

impl Recorder {
    fn new() -> Arc<Recorder> {
        Arc::new(Recorder { seen: Mutex::new(Vec::new()) })
    }

    fn last(&self) -> ChatRequest {
        self.seen.lock().unwrap().last().cloned().expect("no request was made")
    }
}

#[async_trait]
impl Provider for Recorder {
    fn name(&self) -> &str {
        "recorder"
    }

    fn kind(&self) -> ProviderKind {
        ProviderKind::Openai
    }

    async fn stream(&self, request: ChatRequest) -> luna_core::Result<ProviderStream> {
        self.seen.lock().unwrap().push(request);
        Ok(Box::pin(futures_util::stream::iter(vec![
            Ok(ProviderEvent::TextDelta("Red".into())),
            Ok(ProviderEvent::Done(StopReason::EndTurn)),
        ])))
    }
}

struct NullEvents;

impl EventSink for NullEvents {
    fn emit(&self, _event: Emitted) {}
}

fn config() -> Config {
    let mut config = Config::default();
    config.general.default_model = "recorder/test".into();
    config.providers.insert(
        "recorder".into(),
        ProviderConfig {
            preset: None,
            kind: ProviderKind::Openai,
            base_url: Some("http://localhost:1".into()),
            api_key: Some("test".into()),
            api_key_env: None,
            headers: Default::default(),
            models: vec!["test".into()],
            discover_models: false,
        },
    );
    config
}

fn session(provider: Arc<Recorder>) -> AgentSession {
    let config = config();
    let registry = Arc::new(ProviderRegistry::new(&config).with_provider("recorder", provider));
    let deps = Arc::new(AgentDeps::new(registry, Arc::new(NullEvents), std::env::temp_dir()));
    AgentSession::new(deps, AgentRef::root("root"), "recorder/test", "system".into())
}

#[tokio::test]
async fn an_image_reaches_the_provider_request() {
    let provider = Recorder::new();
    let mut session = session(provider.clone());

    // The shape the TUI builds: text plus an image in one user message.
    session.messages.push(Message {
        role: Role::User,
        blocks: vec![
            ContentBlock::text("What colour is this?"),
            ContentBlock::Image { mime_type: "image/png".into(), data: "iVBORw0KGgo=".into() },
        ],
    });

    session.run_turn(String::new()).await.unwrap();

    let request = provider.last();
    let user = request
        .messages
        .iter()
        .find(|message| message.role == Role::User)
        .expect("the user message should be in the request");

    let image = user
        .blocks
        .iter()
        .find_map(|block| match block {
            ContentBlock::Image { mime_type, data } => Some((mime_type.clone(), data.clone())),
            _ => None,
        })
        .expect("the image should survive into the request");

    assert_eq!(image.0, "image/png");
    assert_eq!(image.1, "iVBORw0KGgo=");
    assert!(user.text().contains("What colour"));
}

#[tokio::test]
async fn a_text_only_turn_carries_no_image() {
    let provider = Recorder::new();
    let mut session = session(provider.clone());
    session.run_turn("hello").await.unwrap();

    let request = provider.last();
    let has_image = request.messages.iter().any(|message| {
        message.blocks.iter().any(|block| matches!(block, ContentBlock::Image { .. }))
    });
    assert!(!has_image);
}

#[tokio::test]
async fn the_openai_encoder_emits_a_data_url() {
    // The encoder is what turns the block into what a provider actually reads.
    let message = Message {
        role: Role::User,
        blocks: vec![
            ContentBlock::text("look"),
            ContentBlock::Image { mime_type: "image/png".into(), data: "AAAA".into() },
        ],
    };
    let encoded = luna_core::provider::openai::encode_for_test(&message);
    let parts = encoded["content"].as_array().expect("content should be an array");
    assert_eq!(parts[0]["type"], json!("text"));
    assert_eq!(parts[1]["type"], json!("image_url"));
    assert_eq!(parts[1]["image_url"]["url"], json!("data:image/png;base64,AAAA"));
}

#[tokio::test]
async fn the_anthropic_encoder_emits_a_base64_source() {
    let message = Message {
        role: Role::User,
        blocks: vec![ContentBlock::Image { mime_type: "image/jpeg".into(), data: "BBBB".into() }],
    };
    let encoded = luna_core::provider::anthropic::encode_for_test(&message);
    assert_eq!(encoded["content"][0]["type"], json!("image"));
    assert_eq!(encoded["content"][0]["source"]["media_type"], json!("image/jpeg"));
    assert_eq!(encoded["content"][0]["source"]["data"], json!("BBBB"));
}

#[tokio::test]
async fn the_gemini_encoder_emits_inline_data() {
    let message = Message {
        role: Role::User,
        blocks: vec![ContentBlock::Image { mime_type: "image/webp".into(), data: "CCCC".into() }],
    };
    let encoded = luna_core::provider::gemini::encode_for_test(&message);
    assert_eq!(encoded["parts"][0]["inlineData"]["mimeType"], json!("image/webp"));
    assert_eq!(encoded["parts"][0]["inlineData"]["data"], json!("CCCC"));
}
