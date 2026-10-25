//! Prompt assembly and compaction.
//!
//! Two rules keep this cheap and predictable:
//!
//! 1. The system prompt is built once per session and never reordered, so the
//!    provider's prompt cache stays warm across turns.
//! 2. Compaction replaces the oldest turns with a summary and keeps the tail
//!    verbatim, so recent context is never lossy.

use crate::tokens;
use crate::types::{ContentBlock, Message, Role};

/// The environment block every agent gets.
pub fn environment(cwd: &std::path::Path, platform: &str, date: &str) -> String {
    format!("Working directory: {}\nPlatform: {platform}\nToday: {date}", cwd.display())
}

/// The base instructions. Kept stable so it can be cached.
pub const BASE_PROMPT: &str = "\
You are Luna, a coding agent running in the user's terminal or desktop app.

How to work:
- Read before you write. Never guess at a file's contents.
- Prefer the smallest change that fully solves the problem.
- Use `bash` for builds, tests and git; use the file tools for edits.
- When a task has several steps, plan it with `todo_write` and keep it current.
- Verify your work. Run the tests or the build you just affected.
- If something fails, read the error and fix the cause, not the symptom.

How to talk:
- Be concise. The user is reading a terminal, not a report.
- Reference files as `path:line` so they are clickable.
- Do not narrate tool calls; the interface already shows them.
- Say plainly when you are unsure or blocked.

Delegation:
- You can spawn subagents with `spawn_agent` for independent work.
- Give each subagent a self-contained brief: it cannot see this conversation.
- Do not delegate work you can finish in a few tool calls yourself.";

/// Assemble the system prompt for one agent.
pub fn system_prompt(
    base: &str,
    role: Option<&str>,
    role_instructions: Option<&str>,
    cwd: &std::path::Path,
    platform: &str,
    date: &str,
    extra: Option<&str>,
) -> String {
    let mut prompt = String::with_capacity(base.len() + 512);
    prompt.push_str(base);
    prompt.push_str("\n\n");
    prompt.push_str(&environment(cwd, platform, date));
    if let Some(role) = role {
        prompt.push_str("\n\nYour role: ");
        prompt.push_str(role);
        prompt.push('.');
    }
    if let Some(instructions) = role_instructions {
        if !instructions.trim().is_empty() {
            prompt.push_str("\n\n");
            prompt.push_str(instructions);
        }
    }
    if let Some(extra) = extra {
        if !extra.trim().is_empty() {
            prompt.push_str("\n\n");
            prompt.push_str(extra);
        }
    }
    prompt
}

/// Today's date, in the form the prompt shows.
pub fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

pub fn platform() -> &'static str {
    std::env::consts::OS
}

/// Whether the conversation needs compacting.
pub fn needs_compaction(messages: &[Message], system: &str, context_window: u32, at: f64) -> bool {
    let used = tokens::estimate_messages(messages) + tokens::estimate_text(system);
    let threshold = (context_window as f64 * at.clamp(0.3, 0.95)) as u32;
    used > threshold
}

/// Split a conversation into the part to summarise and the part to keep.
///
/// The tail is kept verbatim and always starts at a user message, so the model
/// never sees a tool result whose call was summarised away.
pub fn split_for_compaction(
    messages: &[Message],
    keep_tail: usize,
) -> (Vec<Message>, Vec<Message>) {
    if messages.len() <= keep_tail {
        return (Vec::new(), messages.to_vec());
    }
    let mut split = messages.len() - keep_tail;
    // Walk back to a user message so the tail is a valid conversation start.
    while split > 0 && messages[split].role != Role::User {
        split -= 1;
    }
    if split == 0 {
        return (Vec::new(), messages.to_vec());
    }
    (messages[..split].to_vec(), messages[split..].to_vec())
}

/// The instruction sent to the model when compacting.
pub fn compaction_prompt() -> &'static str {
    "Summarise the conversation above so work can continue without it. Keep: \
     decisions made, files touched and why, commands that mattered, open \
     problems, and anything the user asked for that is not finished. Drop \
     pleasantries and repeated tool output. Write it as terse notes, not prose."
}

/// Build the synthetic message that replaces the summarised turns.
pub fn compaction_message(summary: &str) -> Message {
    Message {
        role: Role::User,
        blocks: vec![ContentBlock::text(format!(
            "Summary of the earlier conversation:\n\n{summary}"
        ))],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_prompt_includes_environment_and_role() {
        let prompt = system_prompt(
            BASE_PROMPT,
            Some("reviewer"),
            Some("Only review, never edit."),
            std::path::Path::new("/work"),
            "linux",
            "2026-01-01",
            None,
        );
        assert!(prompt.contains("Working directory: /work"));
        assert!(prompt.contains("Your role: reviewer."));
        assert!(prompt.contains("Only review, never edit."));
    }

    #[test]
    fn compaction_triggers_past_the_threshold() {
        let messages = vec![Message::user("x".repeat(40_000))];
        assert!(needs_compaction(&messages, "", 10_000, 0.8));
        assert!(!needs_compaction(&messages, "", 1_000_000, 0.8));
    }

    #[test]
    fn compaction_keeps_a_user_aligned_tail() {
        let messages = vec![
            Message::user("one"),
            Message::assistant(vec![ContentBlock::text("a")]),
            Message::tool("call", "result", false),
            Message::user("two"),
            Message::assistant(vec![ContentBlock::text("b")]),
        ];
        let (head, tail) = split_for_compaction(&messages, 2);
        assert_eq!(head.len(), 3);
        assert_eq!(tail[0].role, Role::User);
        assert_eq!(tail[0].text(), "two");
    }

    #[test]
    fn short_conversations_are_not_split() {
        let messages = vec![Message::user("one")];
        let (head, tail) = split_for_compaction(&messages, 10);
        assert!(head.is_empty());
        assert_eq!(tail.len(), 1);
    }

    #[test]
    fn compaction_message_is_marked_as_a_summary() {
        let message = compaction_message("did things");
        assert!(message.text().contains("Summary of the earlier conversation"));
    }
}
