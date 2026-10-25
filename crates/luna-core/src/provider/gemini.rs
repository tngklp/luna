//! Google Gemini `generateContent`.
//!
//! Gemini differs from the other two in three ways that matter here: roles are
//! `user`/`model`, tool calls carry no id (Luna synthesises one), and the stream
//! is a JSON array of chunks rather than SSE.

use std::collections::BTreeMap;

use async_trait::async_trait;
use futures_util::StreamExt;
use serde_json::{json, Map, Value};

use super::{send_retrying, ChatRequest, Provider, ProviderEvent, ProviderStream};
use crate::config::{ProviderConfig, ProviderKind};
use crate::error::{LunaError, Result};
use crate::types::{ContentBlock, Message, Role, StopReason, ToolCall, Usage};

pub struct GeminiProvider {
    name: String,
    base_url: String,
    api_key: Option<String>,
    headers: BTreeMap<String, String>,
    client: reqwest::Client,
}

impl GeminiProvider {
    pub fn new(name: &str, config: &ProviderConfig) -> Self {
        GeminiProvider {
            name: name.to_string(),
            base_url: config.base_url(name),
            api_key: config.api_key(),
            headers: config.headers.clone(),
            client: super::http_client(),
        }
    }

    fn endpoint(&self, model: &str, action: &str) -> String {
        format!("{}/models/{}:{}", self.base_url.trim_end_matches('/'), model, action)
    }

    fn build_body(&self, request: &ChatRequest) -> Value {
        let mut contents: Vec<Value> = Vec::with_capacity(request.messages.len());
        for message in &request.messages {
            if let Some(value) = encode_message(message) {
                contents.push(value);
            }
        }

        let mut body = Map::new();
        body.insert("contents".into(), json!(contents));

        if let Some(system) = &request.system {
            if !system.is_empty() {
                body.insert("systemInstruction".into(), json!({ "parts": [{ "text": system }] }));
            }
        }

        if !request.tools.is_empty() {
            let declarations: Vec<Value> = request
                .tools
                .iter()
                .map(|tool| {
                    json!({
                        "name": tool.name,
                        "description": tool.description,
                        "parameters": tool.parameters,
                    })
                })
                .collect();
            body.insert("tools".into(), json!([{ "functionDeclarations": declarations }]));
        }

        let mut generation = Map::new();
        if let Some(temperature) = request.temperature {
            generation.insert("temperature".into(), json!(temperature));
        }
        if let Some(max_tokens) = request.max_tokens {
            generation.insert("maxOutputTokens".into(), json!(max_tokens));
        }
        if let Some(budget) = request.thinking.thinking_budget() {
            generation.insert("thinkingConfig".into(), json!({ "thinkingBudget": budget }));
        }
        if !generation.is_empty() {
            body.insert("generationConfig".into(), Value::Object(generation));
        }

        Value::Object(body)
    }
}

fn encode_message(message: &Message) -> Option<Value> {
    match message.role {
        Role::Tool => {
            let parts: Vec<Value> = message
                .blocks
                .iter()
                .filter_map(|block| match block {
                    ContentBlock::ToolResult { tool_call_id, content, .. } => Some(json!({
                        "functionResponse": {
                            "name": tool_call_id,
                            "response": { "result": content }
                        }
                    })),
                    _ => None,
                })
                .collect();
            if parts.is_empty() {
                return None;
            }
            Some(json!({ "role": "user", "parts": parts }))
        }
        Role::Assistant => {
            let parts: Vec<Value> = message
                .blocks
                .iter()
                .filter_map(|block| match block {
                    ContentBlock::Text { text } => {
                        if text.is_empty() {
                            None
                        } else {
                            Some(json!({ "text": text }))
                        }
                    }
                    ContentBlock::ToolCall { name, arguments, .. } => {
                        Some(json!({ "functionCall": { "name": name, "args": arguments } }))
                    }
                    _ => None,
                })
                .collect();
            if parts.is_empty() {
                return None;
            }
            Some(json!({ "role": "model", "parts": parts }))
        }
        _ => {
            let parts: Vec<Value> = message
                .blocks
                .iter()
                .filter_map(|block| match block {
                    ContentBlock::Text { text } => Some(json!({ "text": text })),
                    ContentBlock::Image { mime_type, data } => Some(json!({
                        "inlineData": { "mimeType": mime_type, "data": data }
                    })),
                    _ => None,
                })
                .collect();
            if parts.is_empty() {
                return None;
            }
            Some(json!({ "role": "user", "parts": parts }))
        }
    }
}

fn stop_reason(reason: &str) -> StopReason {
    match reason {
        "MAX_TOKENS" => StopReason::MaxTokens,
        "SAFETY" | "RECITATION" | "PROHIBITED_CONTENT" => StopReason::Refusal,
        _ => StopReason::EndTurn,
    }
}

/// Gemini streams a JSON array; this pulls complete objects out of a buffer.
fn take_objects(buffer: &mut String) -> Vec<Value> {
    let mut out = Vec::new();
    loop {
        let start = match buffer.find('{') {
            Some(index) => index,
            None => {
                buffer.clear();
                break;
            }
        };
        if start > 0 {
            buffer.drain(..start);
        }
        let mut depth = 0i32;
        let mut in_string = false;
        let mut escaped = false;
        let mut end = None;
        for (index, ch) in buffer.char_indices() {
            if in_string {
                if escaped {
                    escaped = false;
                } else if ch == '\\' {
                    escaped = true;
                } else if ch == '"' {
                    in_string = false;
                }
                continue;
            }
            match ch {
                '"' => in_string = true,
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(index + 1);
                        break;
                    }
                }
                _ => {}
            }
        }
        match end {
            Some(end) => {
                let raw: String = buffer.drain(..end).collect();
                if let Ok(value) = serde_json::from_str::<Value>(&raw) {
                    out.push(value);
                }
            }
            None => break,
        }
    }
    out
}

#[async_trait]
impl Provider for GeminiProvider {
    fn name(&self) -> &str {
        &self.name
    }

    fn kind(&self) -> ProviderKind {
        ProviderKind::Gemini
    }

    fn ready(&self) -> bool {
        self.api_key.is_some()
    }

    async fn stream(&self, request: ChatRequest) -> Result<ProviderStream> {
        let body = self.build_body(&request);
        let url = format!(
            "{}?alt=sse&key={}",
            self.endpoint(&request.model, "streamGenerateContent"),
            self.api_key.clone().unwrap_or_default()
        );
        let mut builder = self.client.post(url).json(&body);
        for (name, value) in &self.headers {
            builder = builder.header(name, value);
        }

        let response = send_retrying(builder, &self.name).await?;
        let mut bytes = response.bytes_stream();
        let provider_name = self.name.clone();
        let mut buffer = String::new();
        let mut call_index = 0usize;
        let mut stop: Option<StopReason> = None;

        Ok(Box::pin(async_stream::try_stream! {
            while let Some(chunk) = bytes.next().await {
                let chunk = chunk.map_err(|err| LunaError::Network(err.to_string()))?;
                buffer.push_str(&String::from_utf8_lossy(&chunk));

                // The endpoint is asked for SSE, but a proxy may hand back the
                // raw array form; both are handled by the same object scanner.
                let chunks = if buffer.contains("data:") {
                    let mut events = Vec::new();
                    while let Some(index) = buffer.find("\n\n") {
                        let block: String = buffer.drain(..index + 2).collect();
                        for line in block.lines() {
                            if let Some(data) = line.strip_prefix("data:") {
                                if let Ok(value) = serde_json::from_str::<Value>(data.trim()) {
                                    events.push(value);
                                }
                            }
                        }
                    }
                    events
                } else {
                    take_objects(&mut buffer)
                };

                for value in chunks {
                    if let Some(error) = value.get("error") {
                        let message = error
                            .get("message")
                            .and_then(|message| message.as_str())
                            .unwrap_or("the provider reported an error mid-stream");
                        Err(LunaError::Provider {
                            provider: provider_name.clone(),
                            status: 0,
                            message: message.to_string(),
                            retryable: false,
                        })?;
                    }

                    if let Some(usage) = value.get("usageMetadata") {
                        yield ProviderEvent::Usage(parse_usage(usage));
                    }

                    let candidate = match value.get("candidates").and_then(|c| c.get(0)) {
                        Some(candidate) => candidate,
                        None => continue,
                    };

                    if let Some(parts) = candidate
                        .get("content")
                        .and_then(|content| content.get("parts"))
                        .and_then(|parts| parts.as_array())
                    {
                        for part in parts {
                            if let Some(text) = part.get("text").and_then(|text| text.as_str()) {
                                if !text.is_empty() {
                                    if part.get("thought").and_then(|t| t.as_bool()).unwrap_or(false) {
                                        yield ProviderEvent::ThinkingDelta(text.to_string());
                                    } else {
                                        yield ProviderEvent::TextDelta(text.to_string());
                                    }
                                }
                            }
                            if let Some(call) = part.get("functionCall") {
                                let name = call
                                    .get("name")
                                    .and_then(|name| name.as_str())
                                    .unwrap_or_default()
                                    .to_string();
                                let arguments = call.get("args").cloned().unwrap_or_else(|| json!({}));
                                // Gemini has no call ids, so one is derived from
                                // the position to keep results matchable.
                                let id = format!("gemini_call_{call_index}");
                                yield ProviderEvent::ToolCallStart {
                                    index: call_index,
                                    id: id.clone(),
                                    name: name.clone(),
                                };
                                yield ProviderEvent::ToolCallEnd {
                                    index: call_index,
                                    call: ToolCall { id, name, arguments },
                                };
                                call_index += 1;
                            }
                        }
                    }

                    if let Some(reason) = candidate.get("finishReason").and_then(|r| r.as_str()) {
                        stop = Some(stop_reason(reason));
                    }
                }
            }

            yield ProviderEvent::Done(stop.unwrap_or(StopReason::EndTurn));
        }))
    }

    async fn list_models(&self) -> Result<Vec<String>> {
        let url = format!(
            "{}/models?key={}",
            self.base_url.trim_end_matches('/'),
            self.api_key.clone().unwrap_or_default()
        );
        let mut builder = self.client.get(url);
        for (name, value) in &self.headers {
            builder = builder.header(name, value);
        }
        let response = send_retrying(builder, &self.name).await?;
        let value: Value = response.json().await?;
        Ok(value
            .get("models")
            .and_then(|models| models.as_array())
            .map(|entries| {
                entries
                    .iter()
                    .filter_map(|entry| entry.get("name").and_then(|name| name.as_str()))
                    .map(|name| name.trim_start_matches("models/").to_string())
                    .collect()
            })
            .unwrap_or_default())
    }
}

fn parse_usage(value: &Value) -> Usage {
    Usage {
        input: value.get("promptTokenCount").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
        output: value.get("candidatesTokenCount").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
        cache_read: value.get("cachedContentTokenCount").and_then(|v| v.as_u64()).unwrap_or(0)
            as u32,
        cache_write: 0,
    }
}

/// Expose the message encoder to integration tests.
///
/// The encoder is the part that decides what a provider actually receives, so
/// it is worth asserting on directly rather than only through a mock transport.
#[doc(hidden)]
pub fn encode_for_test(message: &crate::types::Message) -> serde_json::Value {
    encode_message(message).unwrap_or(serde_json::Value::Null)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider() -> GeminiProvider {
        GeminiProvider::new(
            "gemini",
            &ProviderConfig {
                preset: None,
                kind: ProviderKind::Gemini,
                base_url: None,
                api_key: Some("test".into()),
                api_key_env: None,
                headers: BTreeMap::new(),
                models: Vec::new(),
                discover_models: true,
            },
        )
    }

    #[test]
    fn assistant_messages_use_the_model_role() {
        let encoded = encode_message(&Message::assistant(vec![ContentBlock::text("hi")])).unwrap();
        assert_eq!(encoded["role"], json!("model"));
    }

    #[test]
    fn tool_calls_become_function_calls() {
        let message = Message::assistant(vec![ContentBlock::ToolCall {
            id: "x".into(),
            name: "read".into(),
            arguments: json!({"path": "a"}),
        }]);
        let encoded = encode_message(&message).unwrap();
        assert_eq!(encoded["parts"][0]["functionCall"]["name"], json!("read"));
    }

    #[test]
    fn system_prompt_uses_system_instruction() {
        let mut request = ChatRequest::new("gemini-2.5-pro", vec![Message::user("hi")]);
        request.system = Some("be brief".into());
        let body = provider().build_body(&request);
        assert_eq!(body["systemInstruction"]["parts"][0]["text"], json!("be brief"));
    }

    #[test]
    fn extracts_objects_from_a_partial_buffer() {
        let mut buffer = String::from(r#"{"a":1}{"b":"}"#);
        let objects = take_objects(&mut buffer);
        assert_eq!(objects.len(), 1);
        assert_eq!(objects[0]["a"], json!(1));
        // The incomplete object stays buffered for the next chunk.
        assert!(buffer.starts_with(r#"{"b":"}"#));
    }

    #[test]
    fn braces_inside_strings_do_not_end_an_object() {
        let mut buffer = String::from(r#"{"text":"a } b"}"#);
        let objects = take_objects(&mut buffer);
        assert_eq!(objects.len(), 1);
        assert_eq!(objects[0]["text"], json!("a } b"));
    }
}
