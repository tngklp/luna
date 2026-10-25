//! Tools that change how the agent behaves rather than what it can touch:
//! asking the user, applying a patch, driving git, running a REPL, and ending a
//! subagent with a structured report.

use std::collections::BTreeMap;
use std::process::Stdio;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde_json::{json, Value};

use super::{optional_str, required_str, Tool, ToolClass, ToolContext, ToolOutput};
use crate::error::{LunaError, Result};

// ---------------------------------------------------------------- ask_user

/// A question the agent wants answered.
#[derive(Debug, Clone)]
pub struct Question {
    pub prompt: String,
    pub options: Vec<String>,
    /// Whether more than one option may be chosen.
    pub multiple: bool,
}

/// Asks the user. Front-ends implement this; the tool calls it.
#[async_trait]
pub trait QuestionBridge: Send + Sync {
    /// Returns the chosen options, or an empty list when the user dismissed it.
    async fn ask(&self, question: Question) -> Vec<String>;
}

/// Answers every question with its first option. Used by tests and headless runs.
pub struct FirstOption;

#[async_trait]
impl QuestionBridge for FirstOption {
    async fn ask(&self, question: Question) -> Vec<String> {
        question.options.into_iter().take(1).collect()
    }
}

pub struct AskUserTool {
    bridge: Arc<dyn QuestionBridge>,
}

impl AskUserTool {
    pub fn new(bridge: Arc<dyn QuestionBridge>) -> Self {
        AskUserTool { bridge }
    }
}

#[async_trait]
impl Tool for AskUserTool {
    fn name(&self) -> &str {
        "ask_user"
    }

    fn description(&self) -> &str {
        "Ask the user a question and wait for the answer. Use it when a decision \
         is genuinely theirs — a preference, a missing credential, a choice \
         between approaches — rather than guessing."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "question": { "type": "string", "description": "What to ask" },
                "options": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Choices to offer. Omit for a free-text answer."
                },
                "multiple": { "type": "boolean", "description": "Allow more than one choice" }
            },
            "required": ["question"]
        })
    }

    fn class(&self) -> ToolClass {
        ToolClass::Read
    }

    async fn run(&self, args: Value, _ctx: &ToolContext) -> Result<ToolOutput> {
        let prompt = required_str(&args, "question")?;
        let options = args
            .get("options")
            .and_then(|value| value.as_array())
            .map(|values| {
                values.iter().filter_map(|value| value.as_str()).map(|v| v.to_string()).collect()
            })
            .unwrap_or_default();
        let multiple = args.get("multiple").and_then(|value| value.as_bool()).unwrap_or(false);

        let answers = self.bridge.ask(Question { prompt, options, multiple }).await;
        if answers.is_empty() {
            return Ok(ToolOutput::ok("the user dismissed the question without answering"));
        }
        Ok(ToolOutput::ok(answers.join("\n")))
    }
}

// ------------------------------------------------------------- apply_patch

/// One hunk of a unified diff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hunk {
    pub old_start: usize,
    pub old_len: usize,
    pub new_start: usize,
    pub new_len: usize,
    /// Lines with their leading marker: ' ', '-', or '+'.
    pub lines: Vec<String>,
}

/// A parsed unified diff for one file.
#[derive(Debug, Clone, Default)]
pub struct Patch {
    pub old_path: Option<String>,
    pub new_path: Option<String>,
    pub hunks: Vec<Hunk>,
}

/// Parse a unified diff.
///
/// Only the subset models actually emit is supported: `---`/`+++` headers and
/// `@@` hunks. Anything else is ignored rather than rejected, so a patch with a
/// stray `diff --git` line still applies.
pub fn parse_patch(text: &str) -> Result<Patch> {
    let mut patch = Patch::default();
    let mut current: Option<Hunk> = None;

    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("--- ") {
            if let Some(hunk) = current.take() {
                patch.hunks.push(hunk);
            }
            patch.old_path = Some(clean_path(rest));
            continue;
        }
        if let Some(rest) = line.strip_prefix("+++ ") {
            patch.new_path = Some(clean_path(rest));
            continue;
        }
        if let Some(rest) = line.strip_prefix("@@ ") {
            if let Some(hunk) = current.take() {
                patch.hunks.push(hunk);
            }
            current = Some(parse_hunk_header(rest)?);
            continue;
        }
        if let Some(hunk) = current.as_mut() {
            if line.starts_with(' ') || line.starts_with('-') || line.starts_with('+') {
                hunk.lines.push(line.to_string());
            } else if line.starts_with('\\') {
                // "\ No newline at end of file" — nothing to do.
            }
        }
    }
    if let Some(hunk) = current.take() {
        patch.hunks.push(hunk);
    }

    if patch.hunks.is_empty() {
        return Err(LunaError::tool("the patch contains no hunks"));
    }
    Ok(patch)
}

/// Strip the timestamp and any `a/` or `b/` prefix from a diff path.
fn clean_path(raw: &str) -> String {
    let path = raw.split('\t').next().unwrap_or(raw).trim();
    let path = path.strip_prefix("a/").or_else(|| path.strip_prefix("b/")).unwrap_or(path);
    path.to_string()
}

fn parse_hunk_header(rest: &str) -> Result<Hunk> {
    // `-1,5 +1,6 @@ context`
    let header = rest.split("@@").next().unwrap_or(rest).trim();
    let mut parts = header.split_whitespace();
    let old = parts.next().ok_or_else(|| LunaError::tool("malformed hunk header"))?;
    let new = parts.next().ok_or_else(|| LunaError::tool("malformed hunk header"))?;

    let (old_start, old_len) = parse_range(old.trim_start_matches('-'))?;
    let (new_start, new_len) = parse_range(new.trim_start_matches('+'))?;
    Ok(Hunk { old_start, old_len, new_start, new_len, lines: Vec::new() })
}

fn parse_range(value: &str) -> Result<(usize, usize)> {
    match value.split_once(',') {
        Some((start, len)) => Ok((
            start.parse().map_err(|_| LunaError::tool("malformed hunk range"))?,
            len.parse().map_err(|_| LunaError::tool("malformed hunk range"))?,
        )),
        None => Ok((value.parse().map_err(|_| LunaError::tool("malformed hunk range"))?, 1)),
    }
}

/// Apply a patch to a file's contents.
///
/// Each hunk is located by its context rather than by its line number, because a
/// model's line numbers drift as soon as it makes more than one edit.
pub fn apply_patch(original: &str, patch: &Patch) -> Result<String> {
    let mut lines: Vec<String> = original.lines().map(|line| line.to_string()).collect();
    let had_trailing_newline = original.ends_with('\n');

    // Apply from the bottom up so earlier hunks keep their indices.
    let mut ordered = patch.hunks.clone();
    ordered.sort_by_key(|hunk| std::cmp::Reverse(hunk.old_start));

    for hunk in ordered {
        let old_lines: Vec<&str> = hunk
            .lines
            .iter()
            .filter(|line| !line.starts_with('+'))
            .map(|line| &line[1..])
            .collect();
        let new_lines: Vec<String> = hunk
            .lines
            .iter()
            .filter(|line| !line.starts_with('-'))
            .map(|line| line[1..].to_string())
            .collect();

        let position = find_hunk(&lines, &old_lines, hunk.old_start);
        let Some(position) = position else {
            return Err(LunaError::tool(format!(
                "hunk at line {} does not match the file; read it again",
                hunk.old_start
            )));
        };

        lines.splice(position..position + old_lines.len(), new_lines);
    }

    let mut out = lines.join("\n");
    if had_trailing_newline {
        out.push('\n');
    }
    Ok(out)
}

/// Find where a hunk's old lines sit, preferring the stated line number.
fn find_hunk(lines: &[String], old: &[&str], hint: usize) -> Option<usize> {
    if old.is_empty() {
        return Some(hint.saturating_sub(1).min(lines.len()));
    }
    let matches_at = |start: usize| {
        start + old.len() <= lines.len()
            && old.iter().enumerate().all(|(offset, line)| lines[start + offset] == *line)
    };

    // The hint is 1-based and may be off by a few lines; search outward from it.
    let hint = hint.saturating_sub(1).min(lines.len().saturating_sub(1));
    for delta in 0..lines.len() {
        if hint >= delta && matches_at(hint - delta) {
            return Some(hint - delta);
        }
        if hint + delta < lines.len() && matches_at(hint + delta) {
            return Some(hint + delta);
        }
    }
    None
}

pub struct ApplyPatchTool;

#[async_trait]
impl Tool for ApplyPatchTool {
    fn name(&self) -> &str {
        "apply_patch"
    }

    fn description(&self) -> &str {
        "Apply a unified diff to a file. More reliable than `edit` for a change \
         that touches several places at once."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "File to patch" },
                "patch": { "type": "string", "description": "Unified diff, including --- and +++ headers" }
            },
            "required": ["path", "patch"]
        })
    }

    fn class(&self) -> ToolClass {
        ToolClass::Write
    }

    async fn run(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutput> {
        let path = ctx.resolve(&required_str(&args, "path")?)?;
        let text = required_str(&args, "patch")?;
        let patch = match parse_patch(&text) {
            Ok(patch) => patch,
            Err(err) => return Ok(ToolOutput::error(err.to_string())),
        };

        let original = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(err) => {
                return Ok(ToolOutput::error(format!("could not read {}: {err}", path.display())))
            }
        };

        match apply_patch(&original, &patch) {
            Ok(updated) => {
                crate::config::write_atomic(&path, updated.as_bytes())?;
                Ok(ToolOutput::ok(format!(
                    "applied {} hunks to {}",
                    patch.hunks.len(),
                    path.display()
                )))
            }
            Err(err) => Ok(ToolOutput::error(err.to_string())),
        }
    }
}

// -------------------------------------------------------------------- git

pub struct GitTool;

/// Run git in the working directory and return its output.
async fn git(ctx: &ToolContext, args: &[&str]) -> Result<ToolOutput> {
    let output = tokio::process::Command::new("git")
        .args(args)
        .current_dir(&ctx.cwd)
        .stdin(Stdio::null())
        .output()
        .await
        .map_err(|err| LunaError::tool(format!("could not run git: {err}")))?;

    let stdout = String::from_utf8_lossy(&output.stdout).trim_end().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim_end().to_string();
    if output.status.success() {
        Ok(ToolOutput::ok(if stdout.is_empty() { "(no output)".to_string() } else { stdout }))
    } else {
        Ok(ToolOutput::error(if stderr.is_empty() { stdout } else { stderr }))
    }
}

#[async_trait]
impl Tool for GitTool {
    fn name(&self) -> &str {
        "git"
    }

    fn description(&self) -> &str {
        "Run a git command: status, diff, log, show, add, commit, branch, checkout, \
         stash. Prefer this over `bash` for git so the output stays structured."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["status", "diff", "log", "show", "add", "commit", "branch", "checkout", "stash"],
                    "description": "Which git command to run"
                },
                "args": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Extra arguments, e.g. [\"--stat\"] or [\"-m\", \"message\"]"
                }
            },
            "required": ["action"]
        })
    }

    fn class(&self) -> ToolClass {
        ToolClass::Bash
    }

    async fn run(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutput> {
        let action = required_str(&args, "action")?;
        let extra: Vec<String> = args
            .get("args")
            .and_then(|value| value.as_array())
            .map(|values| {
                values.iter().filter_map(|value| value.as_str()).map(|v| v.to_string()).collect()
            })
            .unwrap_or_default();

        let mut command: Vec<&str> = vec![action.as_str()];
        command.extend(extra.iter().map(|value| value.as_str()));

        // `status` and `log` are far more useful with a stable, compact format.
        match action.as_str() {
            "status" if extra.is_empty() => git(ctx, &["status", "--short", "--branch"]).await,
            "log" if extra.is_empty() => git(ctx, &["log", "--oneline", "--decorate", "-20"]).await,
            "diff" if extra.is_empty() => git(ctx, &["diff", "--stat"]).await,
            "status" | "diff" | "log" | "show" | "add" | "commit" | "branch" | "checkout"
            | "stash" => git(ctx, &command).await,
            other => Ok(ToolOutput::error(format!("unsupported git action `{other}`"))),
        }
    }
}

// ------------------------------------------------------------------- repl

/// A persistent interpreter, so state survives between calls.
pub struct Repl {
    child: tokio::process::Child,
    stdin: tokio::process::ChildStdin,
    stdout: tokio::io::BufReader<tokio::process::ChildStdout>,
}

impl Repl {
    /// Start a Python interpreter with an unbuffered, marker-delimited loop.
    pub async fn python(cwd: &std::path::Path) -> Result<Repl> {
        let mut child = tokio::process::Command::new("python")
            .args(["-u", "-i"])
            .current_dir(cwd)
            .env("PYTHONUNBUFFERED", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|err| LunaError::tool(format!("could not start python: {err}")))?;

        let stdin = child.stdin.take().ok_or_else(|| LunaError::tool("python has no stdin"))?;
        let stdout = child.stdout.take().ok_or_else(|| LunaError::tool("python has no stdout"))?;
        Ok(Repl { child, stdin, stdout: tokio::io::BufReader::new(stdout) })
    }

    /// Run one snippet and collect everything it prints.
    pub async fn eval(&mut self, code: &str) -> Result<String> {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

        const MARKER: &str = "__LUNA_REPL_DONE__";
        // `repr` so the marker prints with quotes, which cannot collide with
        // anything the snippet itself prints.
        let script = format!("{code}\nprint(repr({MARKER:?}))\n");
        self.stdin
            .write_all(script.as_bytes())
            .await
            .map_err(|err| LunaError::tool(format!("could not write to the REPL: {err}")))?;
        self.stdin
            .flush()
            .await
            .map_err(|err| LunaError::tool(format!("could not flush the REPL: {err}")))?;

        let mut out = String::new();
        let mut line = String::new();
        loop {
            line.clear();
            let read = tokio::time::timeout(
                std::time::Duration::from_secs(30),
                self.stdout.read_line(&mut line),
            )
            .await
            .map_err(|_| LunaError::tool("the REPL did not answer within 30s"))?
            .map_err(|err| LunaError::tool(format!("could not read the REPL: {err}")))?;
            if read == 0 {
                break;
            }
            if line.contains(MARKER) {
                break;
            }
            out.push_str(&line);
        }
        Ok(out.trim_end().to_string())
    }

    pub fn kill(&mut self) {
        let _ = self.child.start_kill();
    }
}

/// Shared REPL handle, so one interpreter serves a whole session.
#[derive(Clone, Default)]
pub struct ReplHandle {
    inner: Arc<tokio::sync::Mutex<Option<Repl>>>,
}

impl ReplHandle {
    pub fn new() -> Self {
        ReplHandle::default()
    }

    pub async fn eval(&self, cwd: &std::path::Path, code: &str) -> Result<String> {
        let mut guard = self.inner.lock().await;
        if guard.is_none() {
            *guard = Some(Repl::python(cwd).await?);
        }
        let repl = guard.as_mut().ok_or_else(|| LunaError::tool("the REPL is unavailable"))?;
        repl.eval(code).await
    }

    pub async fn reset(&self) {
        let mut guard = self.inner.lock().await;
        if let Some(mut repl) = guard.take() {
            repl.kill();
        }
    }
}

pub struct PythonReplTool {
    handle: ReplHandle,
}

impl PythonReplTool {
    pub fn new(handle: ReplHandle) -> Self {
        PythonReplTool { handle }
    }
}

#[async_trait]
impl Tool for PythonReplTool {
    fn name(&self) -> &str {
        "python_repl"
    }

    fn description(&self) -> &str {
        "Run Python in a persistent interpreter. Variables survive between calls, \
         so use it for data work, calculations and quick experiments."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "code": { "type": "string", "description": "Python to run" },
                "reset": { "type": "boolean", "description": "Start a fresh interpreter first" }
            },
            "required": ["code"]
        })
    }

    fn class(&self) -> ToolClass {
        ToolClass::Bash
    }

    async fn run(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutput> {
        if args.get("reset").and_then(|value| value.as_bool()).unwrap_or(false) {
            self.handle.reset().await;
        }
        let code = required_str(&args, "code")?;
        match self.handle.eval(&ctx.cwd, &code).await {
            Ok(output) if output.is_empty() => Ok(ToolOutput::ok("(no output)")),
            Ok(output) => Ok(ToolOutput::ok(output)),
            Err(err) => Ok(ToolOutput::error(err.to_string())),
        }
    }
}

// ---------------------------------------------------------- submit_result

/// Where a subagent's final report lands.
#[derive(Clone, Default)]
pub struct ResultSlot {
    inner: Arc<Mutex<Option<String>>>,
}

impl ResultSlot {
    pub fn new() -> Self {
        ResultSlot::default()
    }

    pub fn set(&self, value: String) {
        if let Ok(mut slot) = self.inner.lock() {
            *slot = Some(value);
        }
    }

    pub fn take(&self) -> Option<String> {
        self.inner.lock().ok().and_then(|mut slot| slot.take())
    }
}

pub struct SubmitResultTool {
    slot: ResultSlot,
}

impl SubmitResultTool {
    pub fn new(slot: ResultSlot) -> Self {
        SubmitResultTool { slot }
    }
}

#[async_trait]
impl Tool for SubmitResultTool {
    fn name(&self) -> &str {
        "submit_result"
    }

    fn description(&self) -> &str {
        "Finish the task and report back. Call this once the work is done; the \
         summary is what the caller sees."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "summary": { "type": "string", "description": "What you did and what the caller needs to know" },
                "status": {
                    "type": "string",
                    "enum": ["done", "blocked", "failed"],
                    "description": "How the task ended"
                }
            },
            "required": ["summary"]
        })
    }

    fn class(&self) -> ToolClass {
        ToolClass::Read
    }

    async fn run(&self, args: Value, _ctx: &ToolContext) -> Result<ToolOutput> {
        let summary = required_str(&args, "summary")?;
        let status = optional_str(&args, "status").unwrap_or_else(|| "done".to_string());
        self.slot.set(summary.clone());
        Ok(ToolOutput::ok(format!("result submitted ({status})")))
    }
}

/// A map of tool name to the class it belongs to, for the settings UI.
pub fn tool_classes() -> BTreeMap<&'static str, &'static str> {
    BTreeMap::from([
        ("read", "read"),
        ("ls", "read"),
        ("glob", "read"),
        ("grep", "read"),
        ("write", "write"),
        ("edit", "write"),
        ("multi_edit", "write"),
        ("apply_patch", "write"),
        ("bash", "bash"),
        ("git", "bash"),
        ("python_repl", "bash"),
        ("web_fetch", "network"),
        ("web_search", "network"),
        ("read_pdf", "read"),
        ("ask_user", "read"),
        ("submit_result", "read"),
        ("todo_write", "write"),
        ("todo_read", "read"),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    const ORIGINAL: &str = "one\ntwo\nthree\nfour\nfive\n";

    #[test]
    fn a_single_hunk_applies() {
        let patch =
            parse_patch("--- a/f.txt\n+++ b/f.txt\n@@ -2,2 +2,2 @@\n two\n-three\n+THREE\n")
                .unwrap();
        let out = apply_patch(ORIGINAL, &patch).unwrap();
        assert_eq!(out, "one\ntwo\nTHREE\nfour\nfive\n");
    }

    #[test]
    fn several_hunks_apply_bottom_up() {
        let patch = parse_patch(
            "--- a/f.txt\n+++ b/f.txt\n@@ -1,1 +1,1 @@\n-one\n+ONE\n@@ -5,1 +5,1 @@\n-five\n+FIVE\n",
        )
        .unwrap();
        let out = apply_patch(ORIGINAL, &patch).unwrap();
        assert_eq!(out, "ONE\ntwo\nthree\nfour\nFIVE\n");
    }

    #[test]
    fn a_hunk_is_found_even_when_the_line_number_drifts() {
        let patch = parse_patch("--- a/f\n+++ b/f\n@@ -99,1 +99,1 @@\n-three\n+THREE\n").unwrap();
        let out = apply_patch(ORIGINAL, &patch).unwrap();
        assert!(out.contains("THREE"));
    }

    #[test]
    fn a_hunk_that_does_not_match_is_an_error() {
        let patch = parse_patch("--- a/f\n+++ b/f\n@@ -1,1 +1,1 @@\n-nope\n+x\n").unwrap();
        assert!(apply_patch(ORIGINAL, &patch).is_err());
    }

    #[test]
    fn a_patch_without_hunks_is_rejected() {
        assert!(parse_patch("--- a/f\n+++ b/f\n").is_err());
    }

    #[test]
    fn diff_prefixes_are_stripped_from_paths() {
        let patch = parse_patch("--- a/src/x.rs\n+++ b/src/x.rs\n@@ -1 +1 @@\n-a\n+b\n").unwrap();
        assert_eq!(patch.old_path.as_deref(), Some("src/x.rs"));
        assert_eq!(patch.new_path.as_deref(), Some("src/x.rs"));
    }

    #[test]
    fn a_trailing_newline_is_preserved() {
        let patch = parse_patch("--- a/f\n+++ b/f\n@@ -1,1 +1,1 @@\n-one\n+ONE\n").unwrap();
        let out = apply_patch(ORIGINAL, &patch).unwrap();
        assert!(out.ends_with('\n'));
    }

    #[tokio::test]
    async fn ask_user_returns_the_bridge_answer() {
        let tool = AskUserTool::new(Arc::new(FirstOption));
        let ctx = ToolContext::new(".");
        let out =
            tool.run(json!({"question": "Which?", "options": ["a", "b"]}), &ctx).await.unwrap();
        assert_eq!(out.content, "a");
    }

    #[tokio::test]
    async fn submit_result_records_the_summary() {
        let slot = ResultSlot::new();
        let tool = SubmitResultTool::new(slot.clone());
        let ctx = ToolContext::new(".");
        tool.run(json!({"summary": "did the thing"}), &ctx).await.unwrap();
        assert_eq!(slot.take().as_deref(), Some("did the thing"));
    }

    #[test]
    fn every_tool_has_a_class() {
        for (name, class) in tool_classes() {
            assert!(
                ["read", "write", "bash", "network"].contains(&class),
                "{name} has an unknown class {class}"
            );
        }
    }
}
