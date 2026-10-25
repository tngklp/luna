//! Tools the model can call.
//!
//! A tool is a name, a JSON schema and an async function. Everything else —
//! approval policy, output capping, event emission, cancellation — is handled by
//! [`ToolRegistry::execute`], so individual tools stay small and testable.

pub mod agent;
pub mod bash;
pub mod extras;
pub mod fs;
pub mod todo;
pub mod web;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use crate::config::{ApprovalAction, ApprovalConfig};
use crate::error::{LunaError, Result};
use crate::event::{AgentEvent, AgentRef, Emitted, SinkRef, ToolPartial};
use crate::tool::todo::TaskBoard;
use crate::types::ToolSchema;

/// How dangerous a tool is, which decides the approval it needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolClass {
    Read,
    Write,
    Bash,
    Network,
}

impl ToolClass {
    pub fn approval(self, config: &ApprovalConfig) -> ApprovalAction {
        match self {
            ToolClass::Read => config.read,
            ToolClass::Write => config.write,
            ToolClass::Bash => config.bash,
            ToolClass::Network => config.network,
        }
    }
}

/// A channel for a tool's incremental output.
#[derive(Clone)]
pub struct ProgressSink {
    sender: tokio::sync::mpsc::UnboundedSender<String>,
}

impl ProgressSink {
    pub fn new(sender: tokio::sync::mpsc::UnboundedSender<String>) -> Self {
        ProgressSink { sender }
    }

    pub fn push(&self, text: String) {
        // A closed receiver just means the UI moved on.
        let _ = self.sender.send(text);
    }
}

/// Everything a tool needs to run.
pub struct ToolContext {
    pub cwd: PathBuf,
    pub cancel: CancellationToken,
    /// Streams partial output to the UI while the tool runs.
    pub progress: Option<ProgressSink>,
    pub bash_timeout_secs: u64,
}

impl ToolContext {
    pub fn new(cwd: impl Into<PathBuf>) -> Self {
        ToolContext {
            cwd: cwd.into(),
            cancel: CancellationToken::new(),
            progress: None,
            bash_timeout_secs: 120,
        }
    }

    /// Resolve a path against the working directory.
    ///
    /// `..` is normalised lexically so a missing file still reports a sensible
    /// path; the result is not confined to the working directory, because agents
    /// legitimately read outside it, but every write is reported to the UI.
    pub fn resolve(&self, path: &str) -> Result<PathBuf> {
        let candidate = Path::new(path);
        let joined = if candidate.is_absolute() {
            candidate.to_path_buf()
        } else {
            self.cwd.join(candidate)
        };
        let mut normalised = PathBuf::new();
        for component in joined.components() {
            match component {
                std::path::Component::ParentDir => {
                    normalised.pop();
                }
                std::path::Component::CurDir => {}
                other => normalised.push(other.as_os_str()),
            }
        }
        Ok(normalised)
    }

    pub fn emit_progress(&self, text: impl Into<String>) {
        if let Some(sink) = &self.progress {
            sink.push(text.into());
        }
    }
}

/// The result of running a tool.
#[derive(Debug, Clone)]
pub struct ToolOutput {
    pub content: String,
    pub is_error: bool,
}

impl ToolOutput {
    pub fn ok(content: impl Into<String>) -> Self {
        ToolOutput { content: content.into(), is_error: false }
    }

    pub fn error(content: impl Into<String>) -> Self {
        ToolOutput { content: content.into(), is_error: true }
    }
}

#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;

    fn description(&self) -> &str;

    fn parameters(&self) -> Value;

    fn class(&self) -> ToolClass;

    async fn run(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutput>;

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: self.name().to_string(),
            description: self.description().to_string(),
            parameters: self.parameters(),
        }
    }
}

/// Tools larger than this are truncated before they reach the model, which keeps
/// one runaway command from consuming the whole context window.
pub const MAX_OUTPUT_CHARS: usize = 60_000;

fn truncate(content: String) -> String {
    if content.chars().count() <= MAX_OUTPUT_CHARS {
        return content;
    }
    let kept: String = content.chars().take(MAX_OUTPUT_CHARS).collect();
    let dropped = content.chars().count() - MAX_OUTPUT_CHARS;
    format!("{kept}\n\n… output truncated ({dropped} more characters)")
}

/// The set of tools available to one agent.
#[derive(Clone, Default)]
pub struct ToolRegistry {
    tools: BTreeMap<String, Arc<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        ToolRegistry::default()
    }

    /// The standard toolset.
    pub fn standard() -> Self {
        Self::standard_with_board(TaskBoard::new())
    }

    /// The standard toolset, sharing a task board with the orchestrator.
    pub fn standard_with_board(board: TaskBoard) -> Self {
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(fs::ReadTool));
        registry.register(Arc::new(fs::WriteTool));
        registry.register(Arc::new(fs::EditTool));
        registry.register(Arc::new(fs::MultiEditTool));
        registry.register(Arc::new(fs::LsTool));
        registry.register(Arc::new(fs::GlobTool));
        registry.register(Arc::new(fs::GrepTool));
        registry.register(Arc::new(bash::BashTool));
        registry.register(Arc::new(todo::TodoWriteTool::new(board.clone())));
        registry.register(Arc::new(todo::TodoReadTool::new(board)));
        registry.register(Arc::new(web::WebFetchTool));
        registry.register(Arc::new(web::WebSearchTool::auto()));
        registry.register(Arc::new(web::ReadPdfTool));
        registry.register(Arc::new(extras::ApplyPatchTool));
        registry.register(Arc::new(extras::GitTool));
        registry.register(Arc::new(extras::PythonReplTool::new(extras::ReplHandle::new())));
        registry.register(Arc::new(extras::AskUserTool::new(Arc::new(extras::FirstOption))));
        registry
    }

    pub fn register(&mut self, tool: Arc<dyn Tool>) {
        self.tools.insert(tool.name().to_string(), tool);
    }

    pub fn with(mut self, tool: Arc<dyn Tool>) -> Self {
        self.register(tool);
        self
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.get(name).cloned()
    }

    pub fn names(&self) -> Vec<String> {
        self.tools.keys().cloned().collect()
    }

    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }

    /// Keep only the named tools. An empty allowlist means "everything".
    pub fn restrict(&self, allowed: &[String]) -> ToolRegistry {
        if allowed.is_empty() {
            return self.clone();
        }
        let mut registry = ToolRegistry::new();
        for name in allowed {
            if let Some(tool) = self.tools.get(name) {
                registry.tools.insert(name.clone(), tool.clone());
            }
        }
        registry
    }

    pub fn schemas(&self) -> Vec<ToolSchema> {
        self.tools.values().map(|tool| tool.schema()).collect()
    }

    /// Run a tool, emitting start/update/end events around it.
    ///
    /// Approval is the caller's job: the agent loop asks the user first and only
    /// calls this once the call is permitted.
    pub async fn execute(
        &self,
        name: &str,
        call_id: &str,
        args: Value,
        ctx: &ToolContext,
        agent: &AgentRef,
        sink: &SinkRef,
    ) -> ToolOutput {
        let Some(tool) = self.get(name) else {
            let message = format!("unknown tool `{name}`");
            sink.emit(Emitted::new(
                agent.clone(),
                AgentEvent::ToolExecutionEnd { tool_call_id: call_id.to_string(), is_error: true },
            ));
            return ToolOutput::error(message);
        };

        sink.emit(Emitted::new(
            agent.clone(),
            AgentEvent::ToolExecutionStart {
                tool_call_id: call_id.to_string(),
                tool_name: name.to_string(),
                args: args.clone(),
            },
        ));

        // Bridge the tool's progress channel onto the event stream.
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel::<String>();
        let scoped = ToolContext {
            cwd: ctx.cwd.clone(),
            cancel: ctx.cancel.clone(),
            progress: Some(ProgressSink::new(sender)),
            bash_timeout_secs: ctx.bash_timeout_secs,
        };

        let forward = {
            let sink = sink.clone();
            let agent = agent.clone();
            let call_id = call_id.to_string();
            let name = name.to_string();
            let args = args.clone();
            tokio::spawn(async move {
                while let Some(text) = receiver.recv().await {
                    sink.emit(Emitted::new(
                        agent.clone(),
                        AgentEvent::ToolExecutionUpdate {
                            tool_call_id: call_id.clone(),
                            tool_name: name.clone(),
                            args: args.clone(),
                            partial_result: ToolPartial::text(text),
                        },
                    ));
                }
            })
        };

        let outcome = tokio::select! {
            biased;
            _ = ctx.cancel.cancelled() => Err(LunaError::Cancelled),
            result = tool.run(args, &scoped) => result,
        };

        // Dropping the scoped context closes the channel, which ends the
        // forwarder once it has drained.
        drop(scoped);
        let _ = forward.await;

        let output = match outcome {
            Ok(output) => {
                ToolOutput { content: truncate(output.content), is_error: output.is_error }
            }
            Err(LunaError::Cancelled) => ToolOutput::error("the tool was cancelled"),
            Err(err) => ToolOutput::error(err.to_string()),
        };

        sink.emit(Emitted::new(
            agent.clone(),
            AgentEvent::ToolExecutionEnd {
                tool_call_id: call_id.to_string(),
                is_error: output.is_error,
            },
        ));

        output
    }
}

/// Read a required string argument.
pub(crate) fn required_str(args: &Value, key: &str) -> Result<String> {
    args.get(key)
        .and_then(|value| value.as_str())
        .map(|value| value.to_string())
        .ok_or_else(|| LunaError::tool(format!("missing required argument `{key}`")))
}

/// Read an optional string argument.
pub(crate) fn optional_str(args: &Value, key: &str) -> Option<String> {
    args.get(key).and_then(|value| value.as_str()).map(|value| value.to_string())
}

pub(crate) fn optional_usize(args: &Value, key: &str) -> Option<usize> {
    args.get(key).and_then(|value| value.as_u64()).map(|value| value as usize)
}

pub(crate) fn optional_bool(args: &Value, key: &str) -> Option<bool> {
    args.get(key).and_then(|value| value.as_bool())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct Echo;

    #[async_trait]
    impl Tool for Echo {
        fn name(&self) -> &str {
            "echo"
        }
        fn description(&self) -> &str {
            "echo the input"
        }
        fn parameters(&self) -> Value {
            json!({"type": "object", "properties": {"text": {"type": "string"}}})
        }
        fn class(&self) -> ToolClass {
            ToolClass::Read
        }
        async fn run(&self, args: Value, _ctx: &ToolContext) -> Result<ToolOutput> {
            Ok(ToolOutput::ok(required_str(&args, "text")?))
        }
    }

    #[tokio::test]
    async fn executes_a_registered_tool() {
        let registry = ToolRegistry::new().with(Arc::new(Echo));
        let ctx = ToolContext::new(".");
        let sink: SinkRef = Arc::new(crate::event::NullSink);
        let output = registry
            .execute("echo", "call_1", json!({"text": "hi"}), &ctx, &AgentRef::root("a"), &sink)
            .await;
        assert_eq!(output.content, "hi");
        assert!(!output.is_error);
    }

    #[tokio::test]
    async fn unknown_tools_report_an_error() {
        let registry = ToolRegistry::new();
        let ctx = ToolContext::new(".");
        let sink: SinkRef = Arc::new(crate::event::NullSink);
        let output =
            registry.execute("nope", "call_1", json!({}), &ctx, &AgentRef::root("a"), &sink).await;
        assert!(output.is_error);
        assert!(output.content.contains("unknown tool"));
    }

    #[tokio::test]
    async fn cancellation_stops_a_tool() {
        struct Slow;
        #[async_trait]
        impl Tool for Slow {
            fn name(&self) -> &str {
                "slow"
            }
            fn description(&self) -> &str {
                "sleeps"
            }
            fn parameters(&self) -> Value {
                json!({"type": "object"})
            }
            fn class(&self) -> ToolClass {
                ToolClass::Read
            }
            async fn run(&self, _args: Value, _ctx: &ToolContext) -> Result<ToolOutput> {
                tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                Ok(ToolOutput::ok("done"))
            }
        }

        let registry = ToolRegistry::new().with(Arc::new(Slow));
        let ctx = ToolContext::new(".");
        ctx.cancel.cancel();
        let sink: SinkRef = Arc::new(crate::event::NullSink);
        let output =
            registry.execute("slow", "call_1", json!({}), &ctx, &AgentRef::root("a"), &sink).await;
        assert!(output.is_error);
        assert!(output.content.contains("cancelled"));
    }

    #[test]
    fn restrict_keeps_only_allowed_tools() {
        let registry = ToolRegistry::new().with(Arc::new(Echo));
        assert_eq!(registry.restrict(&["echo".to_string()]).names(), vec!["echo"]);
        assert!(registry.restrict(&["other".to_string()]).is_empty());
        assert_eq!(registry.restrict(&[]).names(), vec!["echo"]);
    }

    #[test]
    fn long_output_is_truncated() {
        let long = "x".repeat(MAX_OUTPUT_CHARS + 100);
        let truncated = truncate(long);
        assert!(truncated.contains("output truncated"));
        assert!(truncated.chars().count() < MAX_OUTPUT_CHARS + 100);
    }

    #[test]
    fn paths_are_normalised_lexically() {
        let ctx = ToolContext::new("/work/project");
        assert_eq!(ctx.resolve("src/main.rs").unwrap(), PathBuf::from("/work/project/src/main.rs"));
        assert_eq!(ctx.resolve("../other/a.rs").unwrap(), PathBuf::from("/work/other/a.rs"));
    }
}
