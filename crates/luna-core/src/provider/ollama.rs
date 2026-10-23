//! Native Ollama provider: /api/chat streaming, model listing, keep-alive.

use super::Provider;
use super::types::{
    CompletionRequest, ContentBlock, Message, Role, StopReason, StreamChunk, ToolSpec,
};
use crate::error::{Error, Result};
use async_trait::async_trait;
use futures::{Stream, StreamExt};
use serde_json::{Value, json};
use std::pin::Pin;

pub struct OllamaProvider {
    http: reqwest::Client,
    base_url: String,
    /// How long to keep the model loaded after a request (Ollama `keep_alive`).
    keep_alive: String,
}

impl OllamaProvider {
    pub fn new(base_url: &str) -> Result<Self> {
        if base_url.is_empty() {
            return Err(Error::Config("ollama: base URL is empty".into()));
        }
        Ok(Self {
            http: reqwest::Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
            keep_alive: "10m".into(),
        })
    }

    pub fn local() -> Result<Self> {
        Self::new("http://localhost:11434")
    }

    fn encode_messages(messages: &[Message], system: Option<&str>) -> Vec<Value> {
        let mut out = Vec::new();
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
            let mut content = String::new();
            let mut tool_calls = Vec::new();
            let mut tool_results = Vec::new();
            for b in &m.content {
                match b {
                    ContentBlock::Text { text } => content.push_str(text),
                    ContentBlock::ToolUse { id, name, input } => tool_calls.push(json!({
                        "id": id,
                        "type": "function",
                        "function": { "name": name, "arguments": input },
                    })),
                    ContentBlock::ToolResult {
                        tool_use_id,
                        content: c,
                        ..
                    } => {
                        tool_results.push((tool_use_id.clone(), c.clone()));
                    }
                }
            }
            if !tool_results.is_empty() {
                // Ollama models consume tool results as user-role JSON.
                for (id, c) in tool_results {
                    out.push(json!({
                        "role": "user",
                        "content": serde_json::json!({ "tool_result": { "id": id, "content": c } }).to_string(),
                    }));
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
impl Provider for OllamaProvider {
    fn name(&self) -> &str {
        "ollama"
    }

    async fn stream_completion(
        &self,
        request: CompletionRequest,
        cancel: tokio_util::sync::CancellationToken,
    ) -> Result<Pin<Box<dyn Stream<Item = StreamChunk> + Send>>> {
        let body = json!({
            "model": request.model,
            "stream": true,
            "keep_alive": self.keep_alive,
            "messages": Self::encode_messages(&request.messages, request.system.as_deref()),
            "tools": Self::encode_tools(&request.tools),
            "options": {
                "num_predict": request.max_tokens,
                "temperature": request.temperature,
            },
        });

        let resp = self
            .http
            .post(format!("{}/api/chat", self.base_url))
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(Error::ProviderHttp {
                provider: "ollama".into(),
                status,
                body,
            });
        }

        // Ollama streams newline-delimited JSON objects (not SSE).
        let byte_stream = resp
            .bytes_stream()
            .map(|item| item.map_err(|e| std::io::Error::other(e.to_string())));

        let stream = byte_stream.filter_map(move |item| {
            let cancel = cancel.clone();
            async move {
                if cancel.is_cancelled() {
                    return Some(StreamChunk::Done {
                        stop_reason: StopReason::Cancelled,
                    });
                }
                match item {
                    Ok(bytes) => parse_ndjson_chunk(&String::from_utf8_lossy(&bytes)),
                    Err(e) => {
                        tracing::warn!(error = %e, "ollama stream read error");
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
        let resp = self
            .http
            .get(format!("{}/api/tags", self.base_url))
            .send()
            .await?;
        if !resp.status().is_success() {
            return Ok(Vec::new());
        }
        let v: Value = resp.json().await?;
        Ok(v["models"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|m| m["name"].as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default())
    }
}

/// Parse one NDJSON line (or a buffer containing one JSON object per line).
fn parse_ndjson_chunk(text: &str) -> Option<StreamChunk> {
    // A network chunk may contain multiple lines; process the last complete
    // object and emit text from all of them (Ollama emits small chunks, so
    // this is usually one line).
    let mut text_out = String::new();
    let mut tool_calls = Vec::new();
    let mut done: Option<StopReason> = None;

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if v["done"].as_bool() == Some(true) {
            done = Some(StopReason::EndTurn);
            continue;
        }
        if let Some(t) = v["message"]["content"].as_str() {
            text_out.push_str(t);
        }
        if let Some(calls) = v["message"]["tool_calls"].as_array() {
            for call in calls {
                let name = call["function"]["name"].as_str().unwrap_or_default();
                // Ollama passes arguments as an object (not a string).
                let input = if call["function"]["arguments"].is_object() {
                    call["function"]["arguments"].clone()
                } else {
                    serde_json::from_str(call["function"]["arguments"].as_str().unwrap_or("{}"))
                        .unwrap_or_else(|_| json!({}))
                };
                tool_calls.push((
                    format!("ollama-{}", uuid::Uuid::new_v4().simple()),
                    name.to_string(),
                    input,
                ));
            }
        }
    }

    // Emit in priority: tool calls end the turn; otherwise stream text.
    if !tool_calls.is_empty() {
        // Text before tool calls is rare in Ollama; return the first tool call
        // and let the loop re-invoke for the rest (they arrive in one chunk).
        let (id, name, input) = tool_calls.remove(0);
        return Some(StreamChunk::ToolUse { id, name, input });
    }
    if !text_out.is_empty() {
        return Some(StreamChunk::TextDelta(text_out));
    }
    done.map(|stop_reason| StreamChunk::Done { stop_reason })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_text_chunk() {
        let line =
            r#"{"model":"llama3.2","message":{"role":"assistant","content":"Hi"},"done":false}"#;
        match parse_ndjson_chunk(line) {
            Some(StreamChunk::TextDelta(t)) => assert_eq!(t, "Hi"),
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parses_done_chunk() {
        let line = r#"{"model":"llama3.2","message":{},"done":true}"#;
        match parse_ndjson_chunk(line) {
            Some(StreamChunk::Done {
                stop_reason: StopReason::EndTurn,
            }) => {}
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parses_tool_call_with_object_arguments() {
        let line = r#"{"message":{"role":"assistant","tool_calls":[{"function":{"name":"read_file","arguments":{"path":"a.txt"}}}]},"done":false}"#;
        match parse_ndjson_chunk(line) {
            Some(StreamChunk::ToolUse { name, input, .. }) => {
                assert_eq!(name, "read_file");
                assert_eq!(input["path"], "a.txt");
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn multi_line_chunk_yields_combined_text() {
        let two = "{\"message\":{\"content\":\"a\"},\"done\":false}\n{\"message\":{\"content\":\"b\"},\"done\":false}\n";
        match parse_ndjson_chunk(two) {
            Some(StreamChunk::TextDelta(t)) => assert_eq!(t, "ab"),
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn tool_results_become_user_json() {
        let msgs = vec![Message::tool_result("c1", "contents", false)];
        let encoded = OllamaProvider::encode_messages(&msgs, None);
        assert_eq!(encoded[0]["role"], "user");
        let inner: Value = serde_json::from_str(encoded[0]["content"].as_str().unwrap()).unwrap();
        assert_eq!(inner["tool_result"]["id"], "c1");
    }
}
