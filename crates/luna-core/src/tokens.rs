//! Token estimation.
//!
//! Providers do not expose a free tokenizer, and a real one would cost more than
//! it saves here. Luna uses a size-based estimate that is deliberately slightly
//! pessimistic: over-counting triggers compaction a little early, which is far
//! better than a 400 from the provider.

use crate::types::{ContentBlock, Message, Role};

/// Rough characters-per-token ratio. English sits near 4; code and JSON nearer 3.
const CHARS_PER_TOKEN: f32 = 3.6;
/// Fixed per-message overhead (role marker, separators).
const MESSAGE_OVERHEAD: u32 = 4;

pub fn estimate_text(text: &str) -> u32 {
    if text.is_empty() {
        return 0;
    }
    let chars = text.chars().count() as f32;
    (chars / CHARS_PER_TOKEN).ceil() as u32
}

pub fn estimate_blocks(blocks: &[ContentBlock]) -> u32 {
    blocks
        .iter()
        .map(|block| match block {
            ContentBlock::Text { text } | ContentBlock::Thinking { text } => estimate_text(text),
            ContentBlock::ToolCall { name, arguments, .. } => {
                estimate_text(name) + estimate_text(&arguments.to_string())
            }
            ContentBlock::ToolResult { content, .. } => estimate_text(content),
            // Images are billed by area, not characters; a flat allowance keeps
            // the estimate in the right order of magnitude.
            ContentBlock::Image { .. } => 1_600,
        })
        .sum()
}

pub fn estimate_message(message: &Message) -> u32 {
    estimate_blocks(&message.blocks) + MESSAGE_OVERHEAD
}

pub fn estimate_messages(messages: &[Message]) -> u32 {
    messages.iter().map(estimate_message).sum()
}

/// Account for the tool schemas, which are sent on every request.
pub fn estimate_tools(schemas: &[crate::types::ToolSchema]) -> u32 {
    schemas
        .iter()
        .map(|schema| {
            estimate_text(&schema.name)
                + estimate_text(&schema.description)
                + estimate_text(&schema.parameters.to_string())
                + MESSAGE_OVERHEAD
        })
        .sum()
}

/// Whether a conversation plus its system prompt fits the window.
pub fn fits(used: u32, limit: u32, reserve: u32) -> bool {
    used + reserve < limit
}

/// Count the tokens a set of messages would add, by role, for stats display.
pub fn split_by_role(messages: &[Message]) -> (u32, u32) {
    let mut user = 0;
    let mut assistant = 0;
    for message in messages {
        let count = estimate_message(message);
        match message.role {
            Role::User | Role::System | Role::Tool => user += count,
            Role::Assistant => assistant += count,
        }
    }
    (user, assistant)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_text_costs_nothing() {
        assert_eq!(estimate_text(""), 0);
    }

    #[test]
    fn estimation_scales_with_length() {
        let short = estimate_text("hello");
        let long = estimate_text(&"hello ".repeat(100));
        assert!(long > short * 50);
    }

    #[test]
    fn message_estimate_includes_overhead() {
        let message = Message::user("hi");
        assert!(estimate_message(&message) > estimate_text("hi"));
    }
}
