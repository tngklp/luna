//! Anthropic Messages API.
//!
//! Two details matter for cost and latency: the system prompt is sent as a
//! cacheable block, and the last message gets a cache breakpoint so a long
//! conversation is only re-read once per turn.

use std::collections::BTreeMap;

use async_trait::async_trait;
use futures_util::StreamExt;
use serde_json::{json, Map, Value};

use super::stream;
use super::{send_retrying, ChatRequest, Provider, ProviderEvent, ProviderStream};
use crate::config::{ProviderConfig, ProviderKind};
use crate::error::{LunaError, Result};
use crate::types::{ContentBlock, Message, Role, StopReason, ToolCall, Usage};

const API_VERSION: &str = "2023-06-01";

pub struct AnthropicProvider {
    name: String,
    base_url: String,
    api_key: Option<String>,
    headers: BTreeMap<String, String>,
    client: reqwest::Client,
}

impl AnthropicProvider {
    pub fn new(name: &str, config: &ProviderConfig) -> Self {
        AnthropicProvider {
            name: name.to_string(),
            base_url: config.base_url(name),
            api_key: config.api_key(),
            headers: config.headers.clone(),
            client: super::http_client(),
        }
    }

    fn endpoint(&self, path: &str) -> String {
        format!("{}/{}", self.base_url.trim_end_matches('/'), path.trim_start_matches('/'))
    }

    fn build_body(&self, request: &ChatRequest) -> Value {
        let mut messages: Vec<Value> = Vec::with_capacity(request.messages.len());
        for message in &request.messages {
            if let Some(value) = encode_message(message) {
                messages.push(value);
            }
        }
        // Anthropic requires at least one message.
        if messages.is_empty() {
            messages.push(json!({ "role": "user", "content": "Continue." }));
        }
        // Cache the conversation prefix: mark the final message so the whole
        // history up to it is reusable on the next turn.
        if let Some(last) = messages.last_mut() {
            mark_cache_breakpoint(last);
        }

        let mut body = Map::new();
        body.insert("model".into(), json!(request.model));
        body.insert("messages".into(), json!(messages));
        body.insert("stream".into(), json!(true));
        body.insert("max_tokens".into(), json!(request.max_tokens.unwrap_or(8_192)));

        if let Some(system) = &request.system {
            if !system.is_empty() {
                body.insert(
                    "system".into(),
                    json!([{
                        "type": "text",
                        "text": system,
                        "cache_control": { "type": "ephemeral" }
                    }]),
                );
            }
        }

        if !request.tools.is_empty() {
            let tools: Vec<Value> = request
                .tools
                .iter()
                .map(|tool| {
                    json!({
                        "name": tool.name,
                        "description": tool.description,
                        "input_schema": tool.parameters,
                    })
                })
                .collect();
            body.insert("tools".into(), json!(tools));
        }

        if let Some(temperature) = request.temperature {
            body.insert("temperature".into(), json!(temperature));
        }

        if let Some(budget) = request.thinking.thinking_budget() {
            // Extended thinking requires a budget below max_tokens and forbids a
            // custom temperature.
            let max_tokens = request.max_tokens.unwrap_or(8_192).max(budget + 1_024);
            body.insert("max_tokens".into(), json!(max_tokens));
            body.insert("thinking".into(), json!({ "type": "enabled", "budget_tokens": budget }));
            body.remove("temperature");
        }

        Value::Object(body)
    }
}

/// Attach a cache breakpoint to the last content block of a message.
fn mark_cache_breakpoint(message: &mut Value) {
    let Some(content) = message.get_mut("content") else { return };
    match content {
        Value::Array(blocks) => {
            if let Some(Value::Object(last)) = blocks.last_mut() {
                last.insert("cache_control".into(), json!({ "type": "ephemeral" }));
            }
        }
        Value::String(text) => {
            let text = std::mem::take(text);
            *content = json!([{
                "type": "text",
                "text": text,
                "cache_control": { "type": "ephemeral" }
            }]);
        }
        _ => {}
    }
}

fn encode_message(message: &Message) -> Option<Value> {
    match message.role {
        Role::Tool => {
            // Tool results travel as user messages in the Messages API.
            let blocks: Vec<Value> = message
                .blocks
                .iter()
                .filter_map(|block| match block {
                    ContentBlock::ToolResult { tool_call_id, content, is_error } => Some(json!({
                        "type": "tool_result",
                        "tool_use_id": tool_call_id,
                        "content": content,
                        "is_error": is_error,
                    })),
                    _ => None,
                })
                .collect();
            if blocks.is_empty() {
                return None;
            }
            Some(json!({ "role": "user", "content": blocks }))
        }
        Role::Assistant => {
            let blocks: Vec<Value> = message
                .blocks
                .iter()
                .filter_map(|block| match block {
                    ContentBlock::Text { text } => {
                        if text.is_empty() {
                            None
                        } else {
                            Some(json!({ "type": "text", "text": text }))
                        }
                    }
                    ContentBlock::Thinking { text } => {
                        Some(json!({ "type": "thinking", "thinking": text }))
                    }
                    ContentBlock::ToolCall { id, name, arguments } => Some(json!({
                        "type": "tool_use",
                        "id": id,
                        "name": name,
                        "input": arguments,
                    })),
                    _ => None,
                })
                .collect();
            if blocks.is_empty() {
                return None;
            }
            Some(json!({ "role": "assistant", "content": blocks }))
        }
        _ => {
            let blocks: Vec<Value> = message
                .blocks
                .iter()
                .filter_map(|block| match block {
                    ContentBlock::Text { text } => Some(json!({ "type": "text", "text": text })),
                    ContentBlock::Image { mime_type, data } => Some(json!({
                        "type": "image",
                        "source": { "type": "base64", "media_type": mime_type, "data": data }
                    })),
                    _ => None,
                })
                .collect();
            if blocks.is_empty() {
                return None;
            }
            Some(json!({ "role": "user", "content": blocks }))
        }
    }
}

fn stop_reason(reason: &str) -> StopReason {
    match reason {
        "tool_use" => StopReason::ToolUse,
        "max_tokens" => StopReason::MaxTokens,
        "stop_sequence" => StopReason::StopSequence,
        "refusal" => StopReason::Refusal,
        _ => StopReason::EndTurn,
    }
}

#[async_trait]
impl Provider for AnthropicProvider {
    fn name(&self) -> &str {
        &self.name
    }

    fn kind(&self) -> ProviderKind {
        ProviderKind::Anthropic
    }

    fn ready(&self) -> bool {
        self.api_key.is_some()
    }

    async fn stream(&self, request: ChatRequest) -> Result<ProviderStream> {
        let body = self.build_body(&request);
        let mut builder = self
            .client
            .post(self.endpoint("v1/messages"))
            .header("anthropic-version", API_VERSION)
            .json(&body);
        if let Some(key) = &self.api_key {
            builder = builder.header("x-api-key", key);
        }
        for (name, value) in &self.headers {
            builder = builder.header(name, value);
        }

        let response = send_retrying(builder, &self.name).await?;
        let mut events = Box::pin(stream::decode(response));
        let provider_name = self.name.clone();

        Ok(Box::pin(async_stream::try_stream! {
            // Anthropic streams tool input as partial JSON keyed by block index.
            let mut tool_blocks: BTreeMap<usize, (String, String, String)> = BTreeMap::new();
            let mut stop: Option<StopReason> = None;

            while let Some(event) = events.next().await {
                let event = event?;
                let value: Value = match serde_json::from_str(&event.data) {
                    Ok(value) => value,
                    Err(_) => continue,
                };
                let kind = value.get("type").and_then(|kind| kind.as_str()).unwrap_or_default();

                match kind {
                    "error" => {
                        let message = value
                            .get("error")
                            .and_then(|error| error.get("message"))
                            .and_then(|message| message.as_str())
                            .unwrap_or("the provider reported an error mid-stream");
                        Err(LunaError::Provider {
                            provider: provider_name.clone(),
                            status: 0,
                            message: message.to_string(),
                            retryable: false,
                        })?;
                    }
                    "message_start" => {
                        if let Some(usage) = value
                            .get("message")
                            .and_then(|message| message.get("usage"))
                        {
                            yield ProviderEvent::Usage(parse_usage(usage));
                        }
                    }
                    "content_block_start" => {
                        let index = value.get("index").and_then(|index| index.as_u64()).unwrap_or(0) as usize;
                        let block = value.get("content_block").cloned().unwrap_or(Value::Null);
                        match block.get("type").and_then(|kind| kind.as_str()) {
                            Some("tool_use") => {
                                let id = block
                                    .get("id")
                                    .and_then(|id| id.as_str())
                                    .unwrap_or_default()
                                    .to_string();
                                let name = block
                                    .get("name")
                                    .and_then(|name| name.as_str())
                                    .unwrap_or_default()
                                    .to_string();
                                tool_blocks.insert(index, (id.clone(), name.clone(), String::new()));
                                yield ProviderEvent::ToolCallStart { index, id, name };
                            }
                            Some("text") => {
                                if let Some(text) = block.get("text").and_then(|text| text.as_str()) {
                                    if !text.is_empty() {
                                        yield ProviderEvent::TextDelta(text.to_string());
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    "content_block_delta" => {
                        let index = value.get("index").and_then(|index| index.as_u64()).unwrap_or(0) as usize;
                        let delta = value.get("delta").cloned().unwrap_or(Value::Null);
                        match delta.get("type").and_then(|kind| kind.as_str()) {
                            Some("text_delta") => {
                                if let Some(text) = delta.get("text").and_then(|text| text.as_str()) {
                                    yield ProviderEvent::TextDelta(text.to_string());
                                }
                            }
                            Some("thinking_delta") => {
                                if let Some(text) = delta.get("thinking").and_then(|text| text.as_str()) {
                                    yield ProviderEvent::ThinkingDelta(text.to_string());
                                }
                            }
                            Some("input_json_delta") => {
                                if let Some(partial) =
                                    delta.get("partial_json").and_then(|json| json.as_str())
                                {
                                    if let Some(entry) = tool_blocks.get_mut(&index) {
                                        entry.2.push_str(partial);
                                    }
                                    yield ProviderEvent::ToolCallArgsDelta {
                                        index,
                                        delta: partial.to_string(),
                                    };
                                }
                            }
                            _ => {}
                        }
                    }
                    "content_block_stop" => {
                        if let Some((id, name, arguments)) = tool_blocks.remove(&index_of(&value)) {
                            let parsed: Value = serde_json::from_str(&arguments)
                                .unwrap_or_else(|_| json!({ "__raw": arguments }));
                            yield ProviderEvent::ToolCallEnd {
                                index: index_of(&value),
                                call: ToolCall { id, name, arguments: parsed },
                            };
                        }
                    }
                    "message_delta" => {
                        if let Some(reason) = value
                            .get("delta")
                            .and_then(|delta| delta.get("stop_reason"))
                            .and_then(|reason| reason.as_str())
                        {
                            stop = Some(stop_reason(reason));
                        }
                        if let Some(usage) = value.get("usage") {
                            yield ProviderEvent::Usage(parse_usage(usage));
                        }
                    }
                    "message_stop" => break,
                    _ => {}
                }
            }

            yield ProviderEvent::Done(stop.unwrap_or(StopReason::EndTurn));
        }))
    }

    async fn list_models(&self) -> Result<Vec<String>> {
        let mut builder =
            self.client.get(self.endpoint("v1/models")).header("anthropic-version", API_VERSION);
        if let Some(key) = &self.api_key {
            builder = builder.header("x-api-key", key);
        }
        for (name, value) in &self.headers {
            builder = builder.header(name, value);
        }
        let response = send_retrying(builder, &self.name).await?;
        let value: Value = response.json().await?;
        Ok(value
            .get("data")
            .and_then(|data| data.as_array())
            .map(|entries| {
                entries
                    .iter()
                    .filter_map(|entry| entry.get("id").and_then(|id| id.as_str()))
                    .map(|id| id.to_string())
                    .collect()
            })
            .unwrap_or_default())
    }
}

fn index_of(value: &Value) -> usize {
    value.get("index").and_then(|index| index.as_u64()).unwrap_or(0) as usize
}

fn parse_usage(value: &Value) -> Usage {
    Usage {
        input: value.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
        output: value.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
        cache_read: value.get("cache_read_input_tokens").and_then(|v| v.as_u64()).unwrap_or(0)
            as u32,
        cache_write: value.get("cache_creation_input_tokens").and_then(|v| v.as_u64()).unwrap_or(0)
            as u32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider() -> AnthropicProvider {
        AnthropicProvider::new(
            "anthropic",
            &ProviderConfig {
                preset: None,
                kind: ProviderKind::Anthropic,
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
    fn system_prompt_is_cacheable() {
        let mut request = ChatRequest::new("claude-sonnet-4-5", vec![Message::user("hi")]);
        request.system = Some("be brief".into());
        let body = provider().build_body(&request);
        assert_eq!(body["system"][0]["cache_control"]["type"], json!("ephemeral"));
    }

    #[test]
    fn tool_results_are_user_messages() {
        let encoded = encode_message(&Message::tool("toolu_1", "done", false)).unwrap();
        assert_eq!(encoded["role"], json!("user"));
        assert_eq!(encoded["content"][0]["type"], json!("tool_result"));
    }

    #[test]
    fn thinking_sets_a_budget_and_drops_temperature() {
        let mut request = ChatRequest::new("claude-sonnet-4-5", vec![Message::user("hi")]);
        request.temperature = Some(0.7);
        request.thinking = crate::types::ThinkingLevel::High;
        let body = provider().build_body(&request);
        assert_eq!(body["thinking"]["budget_tokens"], json!(12_000));
        assert!(body.get("temperature").is_none());
        assert!(body["max_tokens"].as_u64().unwrap() > 12_000);
    }

    #[test]
    fn empty_conversations_still_send_a_message() {
        let request = ChatRequest::new("claude-sonnet-4-5", Vec::new());
        let body = provider().build_body(&request);
        assert_eq!(body["messages"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn stop_reasons_map_to_the_shared_enum() {
        assert_eq!(stop_reason("tool_use"), StopReason::ToolUse);
        assert_eq!(stop_reason("max_tokens"), StopReason::MaxTokens);
    }
}
