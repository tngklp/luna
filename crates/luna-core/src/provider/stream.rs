//! Server-sent-event decoding.
//!
//! Providers all speak SSE but differ in payload shape; this module only turns a
//! byte stream into `(event, data)` pairs so each provider stays a thin mapper.

use bytes::Bytes;
use futures_util::Stream;
use futures_util::StreamExt;

use crate::error::{LunaError, Result};

#[derive(Debug, Clone, Default)]
pub struct SseEvent {
    pub event: Option<String>,
    pub data: String,
}

impl SseEvent {
    /// SSE termination marker some providers send as a plain data payload.
    pub fn is_done(&self) -> bool {
        self.data.trim() == "[DONE]"
    }
}

/// Find the end of the next complete block, returning its byte length.
fn block_end(buffer: &[u8]) -> Option<usize> {
    let mut index = 0;
    while index + 1 < buffer.len() {
        if buffer[index] == b'\n' && buffer[index + 1] == b'\n' {
            return Some(index + 2);
        }
        if index + 3 < buffer.len()
            && buffer[index] == b'\r'
            && buffer[index + 1] == b'\n'
            && buffer[index + 2] == b'\r'
            && buffer[index + 3] == b'\n'
        {
            return Some(index + 4);
        }
        index += 1;
    }
    None
}

/// Parse one raw block into an event. Returns `None` for keep-alive comments.
fn parse_block(block: &str) -> Option<SseEvent> {
    let mut event = None;
    let mut data = String::new();
    for line in block.lines() {
        let line = line.trim_end_matches('\r');
        if line.is_empty() || line.starts_with(':') {
            continue;
        }
        let (field, value) = match line.split_once(':') {
            Some((field, value)) => (field, value.strip_prefix(' ').unwrap_or(value)),
            None => (line, ""),
        };
        match field {
            "event" => event = Some(value.to_string()),
            "data" => {
                if !data.is_empty() {
                    data.push('\n');
                }
                data.push_str(value);
            }
            _ => {}
        }
    }
    if data.is_empty() && event.is_none() {
        return None;
    }
    Some(SseEvent { event, data })
}

/// Turn a streaming HTTP body into decoded SSE events.
pub fn decode(response: reqwest::Response) -> impl Stream<Item = Result<SseEvent>> {
    async_stream::try_stream! {
        let mut body = response.bytes_stream();
        let mut buffer: Vec<u8> = Vec::with_capacity(8 * 1024);
        while let Some(chunk) = body.next().await {
            let chunk: Bytes = chunk.map_err(|err| LunaError::Network(err.to_string()))?;
            buffer.extend_from_slice(&chunk);
            while let Some(end) = block_end(&buffer) {
                let raw: Vec<u8> = buffer.drain(..end).collect();
                let text = String::from_utf8_lossy(&raw);
                if let Some(event) = parse_block(&text) {
                    yield event;
                }
            }
        }
        // A provider that closes without a trailing blank line still has a
        // usable final event.
        if !buffer.is_empty() {
            let text = String::from_utf8_lossy(&buffer);
            if let Some(event) = parse_block(&text) {
                yield event;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_data_line() {
        let event = parse_block("data: {\"a\":1}\n\n").unwrap();
        assert_eq!(event.data, "{\"a\":1}");
    }

    #[test]
    fn joins_multiple_data_lines() {
        let event = parse_block("data: one\ndata: two\n\n").unwrap();
        assert_eq!(event.data, "one\ntwo");
    }

    #[test]
    fn keeps_the_event_name() {
        let event = parse_block("event: content_block_delta\ndata: {}\n\n").unwrap();
        assert_eq!(event.event.as_deref(), Some("content_block_delta"));
    }

    #[test]
    fn ignores_comments_and_blank_blocks() {
        assert!(parse_block(": keep-alive\n\n").is_none());
        assert!(parse_block("\n\n").is_none());
    }

    #[test]
    fn detects_the_done_marker() {
        let event = parse_block("data: [DONE]\n\n").unwrap();
        assert!(event.is_done());
    }

    #[test]
    fn finds_block_boundaries_both_ways() {
        assert_eq!(block_end(b"data: x\n\nrest"), Some(9));
        assert_eq!(block_end(b"data: x\r\n\r\nrest"), Some(11));
        assert_eq!(block_end(b"data: x\n"), None);
    }
}
