//! Provider abstraction: one trait, many backends.

pub mod anthropic;
pub mod mock;
pub mod ollama;
pub mod openai_compat;
pub mod resolver;
pub mod types;

pub use types::{
    CompletionRequest, ContentBlock, Message, Role, StopReason, StreamChunk, ToolSpec,
};

use crate::error::Result;
use async_trait::async_trait;
use futures::Stream;

/// A chat-completion backend with streaming support.
///
/// Implementations must be cancel-safe: dropping the returned stream stops
/// work, and the run's `CancellationToken` is checked by the agent loop.
#[async_trait]
pub trait Provider: Send + Sync {
    /// Human-readable provider name (for errors and logs).
    fn name(&self) -> &str;

    /// Stream a completion for `request`.
    ///
    /// The returned stream yields [`StreamChunk`]s and must terminate with a
    /// `StreamChunk::Done`.
    async fn stream_completion(
        &self,
        request: CompletionRequest,
        cancel: tokio_util::sync::CancellationToken,
    ) -> Result<std::pin::Pin<Box<dyn Stream<Item = types::StreamChunk> + Send>>>;

    /// List models available from this backend (best-effort).
    async fn list_models(&self) -> Result<Vec<String>> {
        Ok(Vec::new())
    }
}
