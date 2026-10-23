//! Scripted mock provider for deterministic, network-free tests.

use super::types::{CompletionRequest, StopReason, StreamChunk};
use super::Provider;
use crate::error::Result;
use async_trait::async_trait;
use futures::Stream;
use std::pin::Pin;
use std::sync::Mutex;

/// One scripted assistant turn.
#[derive(Debug, Clone)]
pub enum ScriptedTurn {
    /// Stream this text, then stop.
    Text(String),
    /// Stream text, then request these tool calls, then stop with ToolUse.
    ToolCalls { text: String, calls: Vec<(String, String, serde_json::Value)> },
}

pub struct MockProvider {
    turns: Mutex<Vec<ScriptedTurn>>,
    name: String,
}

impl MockProvider {
    pub fn new(turns: Vec<ScriptedTurn>) -> Self {
        Self {
            turns: Mutex::new(turns),
            name: "mock".into(),
        }
    }

    pub fn with_name(name: &str, turns: Vec<ScriptedTurn>) -> Self {
        Self {
            turns: Mutex::new(turns),
            name: name.to_string(),
        }
    }
}

#[async_trait]
impl Provider for MockProvider {
    fn name(&self) -> &str {
        &self.name
    }

    async fn stream_completion(
        &self,
        _request: CompletionRequest,
        _cancel: tokio_util::sync::CancellationToken,
    ) -> Result<Pin<Box<dyn Stream<Item = StreamChunk> + Send>>> {
        let turn = self.turns.lock().expect("mock turns").pop_front_turn();
        let chunks: Vec<StreamChunk> = match turn {
            Some(ScriptedTurn::Text(text)) => vec![
                StreamChunk::TextDelta(text),
                StreamChunk::Usage { input_tokens: 10, output_tokens: 5 },
                StreamChunk::Done { stop_reason: StopReason::EndTurn },
            ],
            Some(ScriptedTurn::ToolCalls { text, calls }) => {
                let mut chunks = Vec::new();
                if !text.is_empty() {
                    chunks.push(StreamChunk::TextDelta(text));
                }
                for (id, name, input) in calls {
                    chunks.push(StreamChunk::ToolUse { id, name, input });
                }
                chunks.push(StreamChunk::Usage { input_tokens: 10, output_tokens: 5 });
                chunks.push(StreamChunk::Done { stop_reason: StopReason::ToolUse });
                chunks
            }
            None => vec![
                StreamChunk::TextDelta("(mock exhausted)".into()),
                StreamChunk::Done { stop_reason: StopReason::EndTurn },
            ],
        };
        Ok(Box::pin(futures::stream::iter(chunks)))
    }
}

trait PopFront {
    fn pop_front_turn(&mut self) -> Option<ScriptedTurn>;
}

impl PopFront for Vec<ScriptedTurn> {
    fn pop_front_turn(&mut self) -> Option<ScriptedTurn> {
        if self.is_empty() {
            None
        } else {
            Some(self.remove(0))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;

    #[tokio::test]
    async fn streams_scripted_text() {
        let p = MockProvider::new(vec![ScriptedTurn::Text("hello".into())]);
        let cancel = tokio_util::sync::CancellationToken::new();
        let mut s = p
            .stream_completion(
                CompletionRequest {
                    model: "m".into(),
                    system: None,
                    messages: vec![],
                    tools: vec![],
                    max_tokens: 100,
                    temperature: 0.0,
                },
                cancel,
            )
            .await
            .unwrap();
        let mut text = String::new();
        while let Some(chunk) = s.next().await {
            match chunk {
                StreamChunk::TextDelta(t) => text.push_str(&t),
                StreamChunk::Done { .. } => break,
                _ => {}
            }
        }
        assert_eq!(text, "hello");
    }
}
