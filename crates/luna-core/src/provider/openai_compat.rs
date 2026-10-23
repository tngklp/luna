//! OpenAI-compatible Chat Completions provider.
//!
//! Covers OpenAI (`https://api.openai.com/v1`) and any compatible endpoint,
//! including Ollama (`http://localhost:11434/v1`).

use super::Provider;
use super::types::{
    CompletionRequest, ContentBlock, Message, Role, StopReason, StreamChunk, ToolSpec,
};
use crate::error::{Error, Result};
use async_trait::async_trait;
use eventsource_stream::Eventsource;
use futures::{Stream, StreamExt};
use serde_json::{Value, json};
use std::pin::Pin;

pub struct OpenAiCompatProvider {
    http: reqwest::Client,
    base_url: String,
    api_key: String,
    label: String,
}

impl OpenAiCompatProvider {
    /// `base_url` without trailing slash, e.g. `https://api.openai.com/v1`.
    pub fn new(label: &str, base_url: &str, api_key: impl Into<String>) -> Result<Self> {
        if base_url.is_empty() {
            return Err(Error::Config(format!("{label}: base URL is empty")));
        }
        Ok(Self {
            http: reqwest::Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key: api_key.into(),
            label: label.to_string(),
        })
    }

    pub fn openai(api_key: impl Into<String>) -> Result<Self> {
        Self::new("openai", "https://api.openai.com/v1", api_key)
    }

    pub fn ollama() -> Result<Self> {
        // Ollama's OpenAI-compatible endpoint needs no real key.
        Self::new("ollama", "http://localhost:11434/v1", "ollama")
    }

    fn encode_messages(messages: &[Message], system: Option<&str>) -> Vec<Value> {
        let mut out = Vec::with_capacity(messages.len() + 1);
        if let Some(sys) = system {
            out.push(json!({ "role": "system", "content": sys }));
        }
        for m in messages {
            let role = match m.role {
                Role::System => "system",
                Role::User => "user",
                Role::Assistant => "assistant",
                Role::Tool => "tool",
            };
            // OpenAI expects plain string content for user/assistant/system;
            // tool results go in role:"tool" messages keyed by tool_call_id.
            let mut any_tool_result = false;
            let mut content = String::new();
            let mut tool_calls = Vec::new();
            for b in &m.content {
                match b {
                    ContentBlock::Text { text } => content.push_str(text),
                    ContentBlock::ToolUse { id, name, input } => tool_calls.push(json!({
                        "id": id,
                        "type": "function",
                        "function": { "name": name, "arguments": input.to_string() },
                    })),
                    ContentBlock::ToolResult { .. } => any_tool_result = true,
                }
            }
            if any_tool_result {
                for b in &m.content {
                    if let ContentBlock::ToolResult {
                        tool_use_id,
                        content: c,
                        ..
                    } = b
                    {
                        out.push(json!({
                            "role": "tool",
                            "tool_call_id": tool_use_id,
                            "content": c,
                        }));
                    }
                }
                continue;
            }
            let mut msg = json!({ "role": role, "content": content });
            if !tool_calls.is_empty() {
                msg["tool_calls"] = Value::Array(tool_calls);
            }
            out.push(msg);
        }
        out
    }

    fn encode_tools(tools: &[ToolSpec]) -> Vec<Value> {
        tools
            .iter()
            .map(|t| {
                json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.parameters,
                    },
                })
            })
            .collect()
    }
}

#[async_trait]
impl Provider for OpenAiCompatProvider {
    fn name(&self) -> &str {
        &self.label
    }

    async fn stream_completion(
        &self,
        request: CompletionRequest,
        cancel: tokio_util::sync::CancellationToken,
    ) -> Result<Pin<Box<dyn Stream<Item = StreamChunk> + Send>>> {
        let body = json!({
            "model": request.model,
            "stream": true,
            "stream_options": { "include_usage": true },
            "messages": Self::encode_messages(&request.messages, request.system.as_deref()),
            "tools": Self::encode_tools(&request.tools),
            "max_tokens": request.max_tokens,
            "temperature": request.temperature,
        });

        let mut req = self
            .http
            .post(format!("{}/chat/completions", self.base_url))
            .header("content-type", "application/json")
            .json(&body);
        if !self.api_key.is_empty() {
            req = req.bearer_auth(&self.api_key);
        }
        let resp = req.send().await?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(Error::ProviderHttp {
                provider: self.label.clone(),
                status,
                body,
            });
        }

        let byte_stream = resp
            .bytes_stream()
            .map(|item| item.map_err(|e| std::io::Error::other(e.to_string())));
        let sse = byte_stream.eventsource();

        let label = self.label.clone();
        let stream = sse.filter_map(move |item| {
            let cancel = cancel.clone();
            let label = label.clone();
            async move {
                if cancel.is_cancelled() {
                    return Some(StreamChunk::Done {
                        stop_reason: StopReason::Cancelled,
                    });
                }
                match item {
                    Ok(ev) => {
                        if ev.data == "[DONE]" {
                            return Some(StreamChunk::Done {
                                stop_reason: StopReason::EndTurn,
                            });
                        }
                        parse_sse_event(&ev.data, &label)
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "openai-compat sse decode error");
                        Some(StreamChunk::Done {
                            stop_reason: StopReason::EndTurn,
                        })
                    }
                }
            }
        });

        Ok(Box::pin(stream))
    }

    async fn list_models(&self) -> Result<Vec<String>> {
        let mut req = self.http.get(format!("{}/models", self.base_url));
        if !self.api_key.is_empty() {
            req = req.bearer_auth(&self.api_key);
        }
        let resp = req.send().await?;
        if !resp.status().is_success() {
            return Ok(Vec::new());
        }
        let v: Value = resp.json().await?;
        Ok(v["data"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|m| m["id"].as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default())
    }
}

fn parse_sse_event(data: &str, label: &str) -> Option<StreamChunk> {
    let v: Value = serde_json::from_str(data).ok()?;
    if let Some(err_msg) = v["error"]["message"].as_str() {
        tracing::warn!(provider = label, %err_msg, "provider stream error");
        return Some(StreamChunk::Done {
            stop_reason: StopReason::EndTurn,
        });
    }
    let choice = &v["choices"][0];
    let delta = &choice["delta"];

    if let Some(text) = delta["content"].as_str()
        && !text.is_empty()
    {
        return Some(StreamChunk::TextDelta(text.to_string()));
    }

    if let Some(calls) = delta["tool_calls"].as_array() {
        for call in calls {
            if let (Some(id), Some(name)) = (call["id"].as_str(), call["function"]["name"].as_str())
            {
                let input: Value =
                    serde_json::from_str(call["function"]["arguments"].as_str().unwrap_or("{}"))
                        .unwrap_or_else(|_| json!({}));
                return Some(StreamChunk::ToolUse {
                    id: id.to_string(),
                    name: name.to_string(),
                    input,
                });
            }
        }
    }

    if let Some(finish) = choice["finish_reason"].as_str() {
        let stop = match finish {
            "tool_calls" => StopReason::ToolUse,
            "length" => StopReason::MaxTokens,
            _ => StopReason::EndTurn,
        };
        return Some(StreamChunk::Done { stop_reason: stop });
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_text_delta() {
        let data = r#"{"choices":[{"delta":{"content":"hi"}}]}"#;
        match parse_sse_event(data, "openai") {
            Some(StreamChunk::TextDelta(t)) => assert_eq!(t, "hi"),
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parses_tool_call() {
        let data = r#"{"choices":[{"delta":{"tool_calls":[{"id":"c1","function":{"name":"read_file","arguments":"{\"path\":\"a.txt\"}"}}]}}]}"#;
        match parse_sse_event(data, "openai") {
            Some(StreamChunk::ToolUse { id, name, input }) => {
                assert_eq!(id, "c1");
                assert_eq!(name, "read_file");
                assert_eq!(input["path"], "a.txt");
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parses_finish_reasons() {
        let data = r#"{"choices":[{"delta":{},"finish_reason":"tool_calls"}]}"#;
        match parse_sse_event(data, "openai") {
            Some(StreamChunk::Done {
                stop_reason: StopReason::ToolUse,
            }) => {}
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn done_sentinel_is_handled_in_stream_layer() {
        // "[DONE]" is intercepted by the SSE stream wrapper, not parse_sse_event;
        // parse_sse_event returns None for non-JSON payloads.
        assert!(parse_sse_event("[DONE]", "openai").is_none());
    }

    #[test]
    fn tool_results_become_tool_messages() {
        let msgs = vec![Message::tool_result("c1", "contents", false)];
        let encoded = OpenAiCompatProvider::encode_messages(&msgs, None);
        assert_eq!(encoded[0]["role"], "tool");
        assert_eq!(encoded[0]["tool_call_id"], "c1");
    }
}
