//! OpenAI Chat Completions.
//!
//! This also serves every OpenAI-compatible endpoint — Ollama, llama.cpp, vLLM,
//! LM Studio, OpenRouter, corporate gateways — which is why the request builder
//! tolerates missing optional fields instead of insisting on OpenAI's exact set.

use std::collections::BTreeMap;

use async_trait::async_trait;
use futures_util::StreamExt;
use serde_json::{json, Map, Value};

use super::stream;
use super::{send_retrying, ChatRequest, Provider, ProviderEvent, ProviderStream};
use crate::config::{ProviderConfig, ProviderKind};
use crate::error::{LunaError, Result};
use crate::types::{ContentBlock, Message, Role, StopReason, ToolCall, Usage};

pub struct OpenAiProvider {
    name: String,
    base_url: String,
    api_key: Option<String>,
    headers: BTreeMap<String, String>,
    /// OpenAI itself understands `stream_options`; compatible servers often do
    /// not and reject the field, so it is opt-in.
    stream_usage: bool,
    client: reqwest::Client,
}

impl OpenAiProvider {
    pub fn new(name: &str, config: &ProviderConfig) -> Self {
        OpenAiProvider {
            name: name.to_string(),
            base_url: config.base_url(name),
            api_key: config.api_key(),
            headers: config.headers.clone(),
            stream_usage: name == "openai",
            client: super::http_client(),
        }
    }

    fn endpoint(&self, path: &str) -> String {
        format!("{}/{}", self.base_url.trim_end_matches('/'), path.trim_start_matches('/'))
    }

    fn build_body(&self, request: &ChatRequest) -> Value {
        let mut messages: Vec<Value> = Vec::with_capacity(request.messages.len() + 1);
        if let Some(system) = &request.system {
            if !system.is_empty() {
                messages.push(json!({ "role": "system", "content": system }));
            }
        }
        for message in &request.messages {
            if let Some(value) = encode_message(message) {
                messages.push(value);
            }
        }

        let mut body = Map::new();
        body.insert("model".into(), json!(request.model));
        body.insert("messages".into(), json!(messages));
        body.insert("stream".into(), json!(true));

        if !request.tools.is_empty() {
            let tools: Vec<Value> = request
                .tools
                .iter()
                .map(|tool| {
                    json!({
                        "type": "function",
                        "function": {
                            "name": tool.name,
                            "description": tool.description,
                            "parameters": tool.parameters,
                        }
                    })
                })
                .collect();
            body.insert("tools".into(), json!(tools));
            body.insert("tool_choice".into(), json!("auto"));
        }

        if let Some(temperature) = request.temperature {
            body.insert("temperature".into(), json!(temperature));
        }
        if let Some(max_tokens) = request.max_tokens {
            // Reasoning models on the official API reject `max_tokens`.
            let key = if needs_completion_tokens(&request.model) {
                "max_completion_tokens"
            } else {
                "max_tokens"
            };
            body.insert(key.into(), json!(max_tokens));
        }
        if let Some(effort) = request.thinking.reasoning_effort() {
            if is_reasoning_model(&request.model) {
                body.insert("reasoning_effort".into(), json!(effort));
            }
        }
        if self.stream_usage {
            body.insert("stream_options".into(), json!({ "include_usage": true }));
        }
        Value::Object(body)
    }
}

fn is_reasoning_model(model: &str) -> bool {
    let model = model.to_ascii_lowercase();
    model.starts_with("gpt-5")
        || model.starts_with("o1")
        || model.starts_with("o3")
        || model.starts_with("o4")
}

fn needs_completion_tokens(model: &str) -> bool {
    is_reasoning_model(model)
}

/// Encode one message into OpenAI's chat format.
fn encode_message(message: &Message) -> Option<Value> {
    match message.role {
        Role::Tool => {
            let block = message.blocks.iter().find_map(|block| match block {
                ContentBlock::ToolResult { tool_call_id, content, .. } => {
                    Some((tool_call_id.clone(), content.clone()))
                }
                _ => None,
            })?;
            Some(json!({ "role": "tool", "tool_call_id": block.0, "content": block.1 }))
        }
        Role::Assistant => {
            let text = message.text();
            let calls: Vec<Value> = message
                .tool_calls()
                .into_iter()
                .map(|call| {
                    json!({
                        "id": call.id,
                        "type": "function",
                        "function": {
                            "name": call.name,
                            "arguments": call.arguments.to_string(),
                        }
                    })
                })
                .collect();
            let mut value = Map::new();
            value.insert("role".into(), json!("assistant"));
            // Some gateways reject null content next to tool calls; an empty
            // string is universally accepted.
            value.insert(
                "content".into(),
                json!(if text.is_empty() { String::new() } else { text }),
            );
            if !calls.is_empty() {
                value.insert("tool_calls".into(), json!(calls));
            }
            Some(Value::Object(value))
        }
        _ => {
            let has_image =
                message.blocks.iter().any(|block| matches!(block, ContentBlock::Image { .. }));
            if has_image {
                let parts: Vec<Value> = message
                    .blocks
                    .iter()
                    .filter_map(|block| match block {
                        ContentBlock::Text { text } => {
                            Some(json!({ "type": "text", "text": text }))
                        }
                        ContentBlock::Image { mime_type, data } => Some(json!({
                            "type": "image_url",
                            "image_url": { "url": format!("data:{mime_type};base64,{data}") }
                        })),
                        _ => None,
                    })
                    .collect();
                Some(json!({ "role": message.role.as_str(), "content": parts }))
            } else {
                Some(json!({ "role": message.role.as_str(), "content": message.text() }))
            }
        }
    }
}

struct PendingCall {
    id: String,
    name: String,
    arguments: String,
}

impl PendingCall {
    fn finish(self, index: usize) -> ProviderEvent {
        let arguments: Value = serde_json::from_str(&self.arguments)
            .unwrap_or_else(|_| json!({ "__raw": self.arguments }));
        ProviderEvent::ToolCallEnd {
            index,
            call: ToolCall { id: self.id, name: self.name, arguments },
        }
    }
}

fn stop_reason(reason: &str) -> StopReason {
    match reason {
        "tool_calls" | "function_call" => StopReason::ToolUse,
        "length" => StopReason::MaxTokens,
        "content_filter" => StopReason::Refusal,
        _ => StopReason::EndTurn,
    }
}

#[async_trait]
impl Provider for OpenAiProvider {
    fn name(&self) -> &str {
        &self.name
    }

    fn kind(&self) -> ProviderKind {
        ProviderKind::Openai
    }

    fn ready(&self) -> bool {
        // A local endpoint needs no key; anything else does.
        self.api_key.is_some()
            || self.base_url.contains("localhost")
            || self.base_url.contains("127.0.0.1")
    }

    async fn stream(&self, request: ChatRequest) -> Result<ProviderStream> {
        let body = self.build_body(&request);
        let mut builder = self.client.post(self.endpoint("chat/completions")).json(&body);
        if let Some(key) = &self.api_key {
            builder = builder.bearer_auth(key);
        }
        for (name, value) in &self.headers {
            builder = builder.header(name, value);
        }

        let response = send_retrying(builder, &self.name).await?;
        let mut events = Box::pin(stream::decode(response));
        let provider_name = self.name.clone();

        Ok(Box::pin(async_stream::try_stream! {
            let mut calls: BTreeMap<usize, PendingCall> = BTreeMap::new();
            let mut stop: Option<StopReason> = None;

            while let Some(event) = events.next().await {
                let event = event?;
                if event.is_done() {
                    break;
                }
                let value: Value = match serde_json::from_str(&event.data) {
                    Ok(value) => value,
                    // Gateways occasionally emit non-JSON keep-alives.
                    Err(_) => continue,
                };

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

                if let Some(usage) = value.get("usage").filter(|usage| !usage.is_null()) {
                    yield ProviderEvent::Usage(parse_usage(usage));
                }

                let choice = match value.get("choices").and_then(|choices| choices.get(0)) {
                    Some(choice) => choice,
                    None => continue,
                };

                if let Some(delta) = choice.get("delta") {
                    if let Some(text) = delta.get("content").and_then(|content| content.as_str()) {
                        if !text.is_empty() {
                            yield ProviderEvent::TextDelta(text.to_string());
                        }
                    }
                    // Reasoning models expose their thinking under a few names.
                    for key in ["reasoning_content", "reasoning"] {
                        if let Some(text) = delta.get(key).and_then(|content| content.as_str()) {
                            if !text.is_empty() {
                                yield ProviderEvent::ThinkingDelta(text.to_string());
                            }
                        }
                    }
                    if let Some(tool_calls) = delta.get("tool_calls").and_then(|calls| calls.as_array()) {
                        for (position, call) in tool_calls.iter().enumerate() {
                            let index = call
                                .get("index")
                                .and_then(|index| index.as_u64())
                                .map(|index| index as usize)
                                .unwrap_or(position);
                            let function = call.get("function");
                            let name = function
                                .and_then(|function| function.get("name"))
                                .and_then(|name| name.as_str())
                                .unwrap_or_default();
                            let id = call.get("id").and_then(|id| id.as_str()).unwrap_or_default();

                            let pending = calls.entry(index).or_insert_with(|| PendingCall {
                                id: String::new(),
                                name: String::new(),
                                arguments: String::new(),
                            });
                            if !id.is_empty() {
                                pending.id = id.to_string();
                            }
                            if !name.is_empty() && pending.name.is_empty() {
                                pending.name = name.to_string();
                                yield ProviderEvent::ToolCallStart {
                                    index,
                                    id: pending.id.clone(),
                                    name: pending.name.clone(),
                                };
                            }
                            if let Some(arguments) =
                                function.and_then(|function| function.get("arguments")).and_then(|args| args.as_str())
                            {
                                if !arguments.is_empty() {
                                    pending.arguments.push_str(arguments);
                                    yield ProviderEvent::ToolCallArgsDelta {
                                        index,
                                        delta: arguments.to_string(),
                                    };
                                }
                            }
                        }
                    }
                }

                if let Some(reason) = choice.get("finish_reason").and_then(|reason| reason.as_str()) {
                    stop = Some(stop_reason(reason));
                }
            }

            for (index, call) in std::mem::take(&mut calls) {
                yield call.finish(index);
            }
            yield ProviderEvent::Done(stop.unwrap_or(StopReason::EndTurn));
        }))
    }

    async fn list_models(&self) -> Result<Vec<String>> {
        let mut builder = self.client.get(self.endpoint("models"));
        if let Some(key) = &self.api_key {
            builder = builder.bearer_auth(key);
        }
        for (name, value) in &self.headers {
            builder = builder.header(name, value);
        }
        let response = send_retrying(builder, &self.name).await?;
        let value: Value = response.json().await?;
        let models = value
            .get("data")
            .and_then(|data| data.as_array())
            .map(|entries| {
                entries
                    .iter()
                    .filter_map(|entry| entry.get("id").and_then(|id| id.as_str()))
                    .map(|id| id.to_string())
                    .collect()
            })
            .unwrap_or_default();
        Ok(models)
    }
}

fn parse_usage(value: &Value) -> Usage {
    Usage {
        input: value.get("prompt_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
        output: value.get("completion_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
        cache_read: value
            .get("prompt_tokens_details")
            .and_then(|details| details.get("cached_tokens"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32,
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

    fn provider() -> OpenAiProvider {
        OpenAiProvider::new(
            "openai",
            &ProviderConfig {
                preset: None,
                kind: ProviderKind::Openai,
                base_url: Some("https://api.openai.com/v1".into()),
                api_key: Some("test".into()),
                api_key_env: None,
                headers: BTreeMap::new(),
                models: Vec::new(),
                discover_models: true,
            },
        )
    }

    #[test]
    fn builds_a_streaming_request_with_tools() {
        let mut request = ChatRequest::new("gpt-5", vec![Message::user("hi")]);
        request.tools = vec![crate::types::ToolSchema {
            name: "read".into(),
            description: "read a file".into(),
            parameters: json!({"type": "object"}),
        }];
        let body = provider().build_body(&request);
        assert_eq!(body["stream"], json!(true));
        assert_eq!(body["tools"][0]["function"]["name"], json!("read"));
        assert_eq!(body["stream_options"]["include_usage"], json!(true));
    }

    #[test]
    fn reasoning_models_use_completion_tokens() {
        let request = ChatRequest::new("gpt-5", vec![Message::user("hi")]);
        let mut request = request;
        request.max_tokens = Some(100);
        let body = provider().build_body(&request);
        assert_eq!(body["max_completion_tokens"], json!(100));
        assert!(body.get("max_tokens").is_none());
    }

    #[test]
    fn assistant_tool_calls_round_trip() {
        let message = Message::assistant(vec![
            ContentBlock::text("thinking"),
            ContentBlock::ToolCall {
                id: "call_1".into(),
                name: "bash".into(),
                arguments: json!({"command": "ls"}),
            },
        ]);
        let encoded = encode_message(&message).unwrap();
        assert_eq!(encoded["tool_calls"][0]["id"], json!("call_1"));
        assert_eq!(encoded["tool_calls"][0]["function"]["name"], json!("bash"));
    }

    #[test]
    fn tool_results_become_tool_messages() {
        let message = Message::tool("call_1", "output", false);
        let encoded = encode_message(&message).unwrap();
        assert_eq!(encoded["role"], json!("tool"));
        assert_eq!(encoded["tool_call_id"], json!("call_1"));
    }

    #[test]
    fn stop_reasons_map_to_the_shared_enum() {
        assert_eq!(stop_reason("tool_calls"), StopReason::ToolUse);
        assert_eq!(stop_reason("length"), StopReason::MaxTokens);
        assert_eq!(stop_reason("stop"), StopReason::EndTurn);
    }
}
