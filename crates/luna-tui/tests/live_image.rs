//! Prove the TUI's attachment loader feeds a real provider.
//!
//! Ignored by default: it needs a live endpoint. Run with
//! `LUNA_LIVE=1 cargo test -p luna-tui --test live_image -- --ignored`.

use std::sync::Arc;

use luna_core::agent::{AgentDeps, AgentSession};
use luna_core::config::{Config, ProviderConfig, ProviderKind};
use luna_core::event::{AgentRef, Emitted, EventSink};
use luna_core::provider::ProviderRegistry;
use luna_core::types::{ContentBlock, Message, Role};
use luna_tui::image;

struct NullEvents;

impl EventSink for NullEvents {
    fn emit(&self, _event: Emitted) {}
}

/// An 8x8 solid red PNG.
///
/// Generated rather than copied, and every chunk's CRC verified: a corrupt image
/// is rejected by the provider with a generic "invalid request", which is a
/// confusing way to learn the fixture was bad.
const RED_PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x08, 0x00, 0x00, 0x00, 0x08, 0x08, 0x02, 0x00, 0x00, 0x00, 0x4b, 0x6d, 0x29,
    0xdc, 0x00, 0x00, 0x00, 0x11, 0x49, 0x44, 0x41, 0x54, 0x78, 0xda, 0x63, 0xf8, 0xcf, 0xc0, 0x80,
    0x15, 0x31, 0x0c, 0x2d, 0x09, 0x00, 0x28, 0xff, 0x3f, 0xc1, 0xce, 0x77, 0xc8, 0x4f, 0x00, 0x00,
    0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];

#[tokio::test]
#[ignore = "needs a live provider; set LUNA_LIVE=1"]
async fn a_real_provider_reads_an_attached_image() {
    if std::env::var("LUNA_LIVE").is_err() {
        return;
    }
    let base_url =
        std::env::var("LUNA_LIVE_URL").unwrap_or_else(|_| "https://api.river.ai/v1".to_string());
    let model = std::env::var("LUNA_LIVE_MODEL")
        .unwrap_or_else(|_| "deepseek-ai/DeepSeek-V4.1-Flash-1017".to_string());
    let key = std::env::var("LUNA_LIVE_KEY").expect("LUNA_LIVE_KEY must be set");

    let dir = std::env::temp_dir().join(format!("luna-live-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("red.png");
    std::fs::write(&path, RED_PNG).unwrap();

    let attachment = image::load(&path).expect("the image should load");
    assert_eq!(attachment.mime_type, "image/png");

    let mut config = Config::default();
    config.general.default_model = format!("live/{model}");
    config.providers.insert(
        "live".into(),
        ProviderConfig {
            preset: None,
            kind: ProviderKind::Openai,
            base_url: Some(base_url),
            api_key: Some(key),
            api_key_env: None,
            headers: Default::default(),
            models: vec![model.clone()],
            discover_models: false,
        },
    );

    let registry = Arc::new(ProviderRegistry::new(&config));
    let deps = Arc::new(AgentDeps::new(registry, Arc::new(NullEvents), &dir));
    let mut session =
        AgentSession::new(deps, AgentRef::root("live"), format!("live/{model}"), String::new());

    session.messages.push(Message {
        role: Role::User,
        blocks: vec![
            ContentBlock::text("What colour is this image? Answer with one word."),
            attachment.to_block(),
        ],
    });

    session.run_turn(String::new()).await.expect("the turn should succeed");

    let answer = session
        .messages
        .iter()
        .rev()
        .find(|message| message.role == Role::Assistant)
        .map(|message| message.text())
        .unwrap_or_default();
    assert!(
        answer.to_lowercase().contains("red"),
        "the model should have seen a red image, got: {answer}"
    );
    std::fs::remove_dir_all(&dir).ok();
}
