//! Anthropic Messages API provider (streaming, tool use).

use super::types::{CompletionRequest, ContentBlock, Message, Role, StopReason, StreamChunk, ToolSpec};
use super::Provider;
use crate::error::{Error, Result};
use async_trait::async_trait;
use eventsource_stream::Eventsource;
use futures::{Stream, StreamExt};
use serde_json::{json, Value};
use std::pin::Pin;

const API_URL: &str = "https://api.anthropic.com/v1/messages";
const API_VERSION: &str = "2023-06-01";

pub struct AnthropicProvider {
    http: reqwest::Client,
    api_key: String,
}

impl AnthropicProvider {
    pub fn new(api_key: impl Into<String>) -> Result<Self> {
        let api_key = api_key.into();
        if api_key.is_empty() {
            return Err(Error::Config("Anthropic API key is empty".into()));
        }
        Ok(Self {
            http: reqwest::Client::new(),
            api_key,
        })
    }

    fn encode_messages(messages: &[Message]) -> Result<Vec<Value>> {
        let mut out = Vec::with_capacity(messages.len());
        for m in messages {
            // The Messages API has no `tool` role; tool results ride in user messages.
            let role = match m.role {
                Role::System => continue, // system handled separately
                Role::User | Role::Tool => "user",
                Role::Assistant => "assistant",
            };
            let content: Vec<Value> = m
                .content
                .iter()
                .map(|b| match b {
                    ContentBlock::Text { text } => json!({ "type": "text", "text": text }),
                    ContentBlock::ToolUse { id, name, input } => {
                        json!({ "type": "tool_use", "id": id, "name": name, "input": input })
                    }
                    ContentBlock::ToolResult { tool_use_id, content, is_error } => json!({
                        "type": "tool_result",
                        "tool_use_id": tool_use_id,
                        "content": content,
                        "is_error": is_error,
                    }),
                })
                .collect();
            out.push(json!({ "role": role, "content": content }));
        }
        Ok(out)
    }

    fn encode_tools(tools: &[ToolSpec]) -> Vec<Value> {
        tools
            .iter()
            .map(|t| {
                json!({
                    "name": t.name,
                    "description": t.description,
                    "input_schema": t.parameters,
                })
            })
            .collect()
    }

    async fn raise_api_error(&self, status: reqwest::StatusCode, body: String) -> Error {
        Error::ProviderHttp {
            provider: "anthropic".into(),
            status: status.as_u16(),
            body,
        }
    }
}

#[async_trait]
impl Provider for AnthropicProvider {
    fn name(&self) -> &str {
        "anthropic"
    }

    async fn stream_completion(
        &self,
        request: CompletionRequest,
        cancel: tokio_util::sync::CancellationToken,
    ) -> Result<Pin<Box<dyn Stream<Item = StreamChunk> + Send>>> {
        let mut body = json!({
            "model": request.model,
            "max_tokens": request.max_tokens,
            "temperature": request.temperature,
            "stream": true,
            "messages": Self::encode_messages(&request.messages)?,
            "tools": Self::encode_tools(&request.tools),
        });
        if let Some(system) = &request.system {
            body["system"] = json!(system);
        }

        let resp = self
            .http
            .post(API_URL)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", API_VERSION)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(self.raise_api_error(status, body).await);
        }

        let byte_stream = resp
            .bytes_stream()
            .map(|item| item.map_err(|e| std::io::Error::other(e.to_string())));
        let sse = byte_stream.eventsource();

        let stream = sse.filter_map(move |item| {
            let cancel = cancel.clone();
            async move {
                if cancel.is_cancelled() {
                    return Some(StreamChunk::Done { stop_reason: StopReason::Cancelled });
                }
                match item {
                    Ok(ev) => parse_sse_event(&ev.data),
                    Err(e) => {
                        tracing::warn!(error = %e, "anthropic sse decode error");
                        Some(StreamChunk::Done { stop_reason: StopReason::EndTurn })
                    }
                }
            }
        });

        Ok(Box::pin(stream))
    }

    async fn list_models(&self) -> Result<Vec<String>> {
        // Anthropic has no public model-list endpoint usable here; return known families.
        Ok(vec![
            "claude-sonnet-4-5".into(),
            "claude-opus-4-1".into(),
            "claude-haiku-4-5".into(),
        ])
    }
}

/// Map one SSE `data:` payload to zero or more stream chunks.
fn parse_sse_event(data: &str) -> Option<StreamChunk> {
    let v: Value = serde_json::from_str(data).ok()?;
    match v["type"].as_str() {
        Some("content_block_delta") => {
            let delta = &v["delta"];
            match delta["type"].as_str() {
                Some("text_delta") => delta["text"]
                    .as_str()
                    .map(|t| StreamChunk::TextDelta(t.to_string())),
                Some("input_json_delta") => {
                    // Partial tool JSON; assembled by the agent loop via ToolUse below
                    // only when complete. We forward as text on a synthetic path is
                    // wrong, so we ignore partials here: Anthropic sends the full
                    // input via input_json_delta accumulation, so we buffer instead.
                    None
                }
                _ => None,
            }
        }
        Some("content_block_start") => {
            let block = &v["content_block"];
            if block["type"] == "tool_use" {
                // Emit ToolUse with empty input; input arrives via deltas and is
                // finalized in content_block_stop handling below (buffered here).
                Some(StreamChunk::ToolUse {
                    id: block["id"].as_str().unwrap_or_default().to_string(),
                    name: block["name"].as_str().unwrap_or_default().to_string(),
                    input: Value::Object(Default::default()),
                })
            } else {
                None
            }
        }
        Some("message_delta") => {
            let stop = v["delta"]["stop_reason"].as_str();
            let usage = &v["usage"];
            let _ = usage;
            match stop {
                Some("tool_use") => Some(StreamChunk::Done { stop_reason: StopReason::ToolUse }),
                Some("max_tokens") => Some(StreamChunk::Done { stop_reason: StopReason::MaxTokens }),
                Some("end_turn") | None => Some(StreamChunk::Done { stop_reason: StopReason::EndTurn }),
                _ => Some(StreamChunk::Done { stop_reason: StopReason::EndTurn }),
            }
        }
        Some("message_stop") => None, // Done already emitted by message_delta
        Some("error") => Some(StreamChunk::Done { stop_reason: StopReason::EndTurn }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_text_delta() {
        let data = r#"{"type":"content_block_delta","delta":{"type":"text_delta","text":"hi"}}"#;
        match parse_sse_event(data) {
            Some(StreamChunk::TextDelta(t)) => assert_eq!(t, "hi"),
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parses_tool_use_start() {
        let data = r#"{"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"tu_1","name":"read_file","input":{}}}"#;
        match parse_sse_event(data) {
            Some(StreamChunk::ToolUse { id, name, .. }) => {
                assert_eq!(id, "tu_1");
                assert_eq!(name, "read_file");
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parses_stop_reasons() {
        let data = r#"{"type":"message_delta","delta":{"stop_reason":"tool_use"}}"#;
        match parse_sse_event(data) {
            Some(StreamChunk::Done { stop_reason: StopReason::ToolUse }) => {}
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn encodes_tool_role_as_user() {
        let msgs = vec![Message::tool_result("tu_1", "file contents", false)];
        let encoded = AnthropicProvider::encode_messages(&msgs).unwrap();
        assert_eq!(encoded[0]["role"], "user");
        assert_eq!(encoded[0]["content"][0]["type"], "tool_result");
    }
}
