//! The agentic loop.
//!
//! One turn is: assemble context, stream the model, run the tools it asked for,
//! append the results, repeat until it stops asking. Everything is cancellable
//! and every step is observable through the event sink.

pub mod context;
pub mod queue;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tokio_util::sync::CancellationToken;

use crate::config::{ApprovalAction, ApprovalConfig, Config};
use crate::error::{LunaError, Result};
use crate::event::{AgentEvent, AgentRef, AssistantUpdate, Emitted, SinkRef, StreamMessage};
use crate::provider::{ChatRequest, Provider, ProviderEvent, ProviderRegistry};
use crate::tool::{ToolClass, ToolContext, ToolRegistry};
use crate::types::{
    ContentBlock, Message, ModelInfo, Role, StopReason, ThinkingLevel, ToolCall, Usage,
};
use queue::Queues;

/// A request from the agent for the user to decide something.
#[derive(Debug, Clone)]
pub enum UiRequest {
    Approval { tool: String, title: String, detail: String },
}

/// The user's answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiAnswer {
    Allow,
    AllowAlways,
    Deny,
}

/// Asks the user. Front-ends implement this; headless runs use [`AutoApprove`].
#[async_trait::async_trait]
pub trait UiBridge: Send + Sync {
    async fn ask(&self, request: UiRequest) -> UiAnswer;
}

/// Approves everything. Used by tests and by `--yes`.
pub struct AutoApprove;

#[async_trait::async_trait]
impl UiBridge for AutoApprove {
    async fn ask(&self, _request: UiRequest) -> UiAnswer {
        UiAnswer::Allow
    }
}

/// Denies everything that needs asking. Used by non-interactive runs.
pub struct AutoDeny;

#[async_trait::async_trait]
impl UiBridge for AutoDeny {
    async fn ask(&self, _request: UiRequest) -> UiAnswer {
        UiAnswer::Deny
    }
}

/// Counters and totals for one agent.
#[derive(Debug, Clone, Default)]
pub struct AgentStats {
    pub usage: Usage,
    pub cost: f64,
    pub steps: usize,
    pub tool_calls: usize,
}

/// A shared cost ledger for a whole agent tree.
#[derive(Debug, Default)]
pub struct Budget {
    spent: std::sync::Mutex<f64>,
    limit: std::sync::Mutex<f64>,
}

impl Budget {
    pub fn new(limit_usd: f64) -> Self {
        Budget { spent: std::sync::Mutex::new(0.0), limit: std::sync::Mutex::new(limit_usd) }
    }

    pub fn spend(&self, amount: f64) -> Result<()> {
        let mut spent = self.spent.lock().unwrap_or_else(|err| err.into_inner());
        *spent += amount;
        let limit = *self.limit.lock().unwrap_or_else(|err| err.into_inner());
        if limit > 0.0 && *spent > limit {
            return Err(LunaError::Budget(format!("spent ${:.4} of a ${limit:.2} limit", *spent)));
        }
        Ok(())
    }

    pub fn spent(&self) -> f64 {
        *self.spent.lock().unwrap_or_else(|err| err.into_inner())
    }

    pub fn set_limit(&self, limit: f64) {
        *self.limit.lock().unwrap_or_else(|err| err.into_inner()) = limit;
    }
}

/// Everything an agent needs that is not its own conversation.
pub struct AgentDeps {
    pub providers: Arc<ProviderRegistry>,
    pub tools: ToolRegistry,
    pub sink: SinkRef,
    pub ui: Arc<dyn UiBridge>,
    pub approvals: ApprovalConfig,
    pub budget: Arc<Budget>,
    pub cwd: PathBuf,
    pub bash_timeout_secs: u64,
    /// Set when the user chose "always allow" for a tool class.
    pub always_allow: Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
}

impl AgentDeps {
    pub fn new(providers: Arc<ProviderRegistry>, sink: SinkRef, cwd: impl Into<PathBuf>) -> Self {
        let config = providers.config().clone();
        AgentDeps {
            providers,
            tools: ToolRegistry::standard(),
            sink,
            ui: Arc::new(AutoDeny),
            approvals: config.approvals,
            budget: Arc::new(Budget::new(config.general.max_cost_usd)),
            cwd: cwd.into(),
            bash_timeout_secs: config.general.bash_timeout_secs,
            always_allow: Arc::new(std::sync::Mutex::new(std::collections::HashSet::new())),
        }
    }

    pub fn with_ui(mut self, ui: Arc<dyn UiBridge>) -> Self {
        self.ui = ui;
        self
    }

    pub fn with_tools(mut self, tools: ToolRegistry) -> Self {
        self.tools = tools;
        self
    }
}

/// One agent: a conversation, a model, and the loop that drives them.
pub struct AgentSession {
    pub agent: AgentRef,
    pub model: String,
    pub thinking: ThinkingLevel,
    pub messages: Vec<Message>,
    pub system: String,
    pub stats: AgentStats,
    pub queues: Queues,
    pub cancel: CancellationToken,
    deps: Arc<AgentDeps>,
    /// Set while a turn is running, so the UI can show "Running".
    streaming: Arc<AtomicBool>,
}

impl AgentSession {
    pub fn new(
        deps: Arc<AgentDeps>,
        agent: AgentRef,
        model: impl Into<String>,
        system: String,
    ) -> Self {
        AgentSession {
            agent,
            model: model.into(),
            thinking: ThinkingLevel::Medium,
            messages: Vec::new(),
            system,
            stats: AgentStats::default(),
            queues: Queues::new(),
            cancel: CancellationToken::new(),
            deps,
            streaming: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn deps(&self) -> &Arc<AgentDeps> {
        &self.deps
    }

    pub fn is_streaming(&self) -> bool {
        self.streaming.load(Ordering::Relaxed)
    }

    pub fn model_info(&self) -> ModelInfo {
        self.deps.providers.model_info(&self.model)
    }

    fn emit(&self, event: AgentEvent) {
        self.deps.sink.emit(Emitted::new(self.agent.clone(), event));
    }

    fn emit_queues(&self) {
        self.emit(AgentEvent::QueueUpdate {
            steering: self.queues.steering(),
            follow_up: self.queues.follow_up(),
        });
    }

    /// Queue a message for the running turn.
    pub fn steer(&mut self, text: impl Into<String>) {
        self.queues.push_steering(text);
        self.emit_queues();
    }

    /// Queue a message for after the turn settles.
    pub fn follow_up(&mut self, text: impl Into<String>) {
        self.queues.push_follow_up(text);
        self.emit_queues();
    }

    pub fn abort(&self) {
        self.cancel.cancel();
    }

    pub fn set_thinking(&mut self, level: ThinkingLevel) {
        self.thinking = level;
        self.emit(AgentEvent::ThinkingLevelChanged { level: level.as_str().to_string() });
    }

    /// Run one user turn to completion.
    ///
    /// Returns the assistant messages produced. A cancelled turn returns
    /// `Ok(())` with whatever was already appended, because a partial answer is
    /// still useful to the user.
    pub async fn run_turn(&mut self, prompt: impl Into<String>) -> Result<()> {
        let prompt = prompt.into();
        if !prompt.trim().is_empty() {
            self.messages.push(Message::user(prompt));
        }
        self.streaming.store(true, Ordering::Relaxed);
        let outcome = self.drive().await;
        self.streaming.store(false, Ordering::Relaxed);
        self.emit(AgentEvent::AgentSettled {});
        outcome
    }

    /// The loop itself, without the surrounding bookkeeping.
    async fn drive(&mut self) -> Result<()> {
        let max_steps = self.deps.providers.config().general.max_steps;
        let mut step = 0usize;

        loop {
            if self.cancel.is_cancelled() {
                return Ok(());
            }
            if step >= max_steps {
                self.deps.sink.emit(Emitted::notice(
                    "warning",
                    format!("stopped after {max_steps} steps; send another message to continue"),
                ));
                return Ok(());
            }
            step += 1;
            self.stats.steps += 1;

            // Fold in anything the user steered with while we were working.
            for text in self.queues.drain_steering() {
                self.messages.push(Message::user(text));
            }
            self.emit_queues();

            self.maybe_compact().await?;

            let (provider, model) = self.deps.providers.resolve(&self.model)?;
            let request = ChatRequest {
                model,
                system: Some(self.system.clone()),
                messages: self.messages.clone(),
                tools: self.deps.tools.schemas(),
                temperature: None,
                max_tokens: None,
                thinking: self.thinking,
            };

            let assistant = self.stream_assistant(provider, request).await?;
            let calls = assistant.tool_calls();
            self.messages.push(assistant);

            if calls.is_empty() {
                // The model is done. A queued follow-up starts the next turn.
                if let Some(next) = self.queues.pop_follow_up() {
                    self.messages.push(Message::user(next));
                    self.emit_queues();
                    continue;
                }
                return Ok(());
            }

            self.run_tools(calls).await?;
        }
    }

    /// Stream one assistant message, emitting deltas as they arrive.
    async fn stream_assistant(
        &mut self,
        provider: Arc<dyn Provider>,
        request: ChatRequest,
    ) -> Result<Message> {
        self.emit(AgentEvent::MessageStart {
            message: StreamMessage { role: Role::Assistant, id: None },
        });

        let mut stream = provider.stream(request).await?;
        let mut blocks: Vec<ContentBlock> = Vec::new();
        let mut text = String::new();
        let mut thinking = String::new();
        let mut calls: Vec<ToolCall> = Vec::new();
        let mut pending: std::collections::BTreeMap<usize, (String, String, String)> =
            std::collections::BTreeMap::new();
        let mut stop = StopReason::EndTurn;

        loop {
            let next = tokio::select! {
                biased;
                _ = self.cancel.cancelled() => {
                    self.emit(AgentEvent::MessageEnd { stop_reason: Some("cancelled".into()) });
                    break;
                }
                item = futures_util::StreamExt::next(&mut stream) => item,
            };
            let Some(item) = next else { break };
            let event = item?;

            match event {
                ProviderEvent::TextDelta(delta) => {
                    text.push_str(&delta);
                    self.emit(AgentEvent::MessageUpdate {
                        assistant_message_event: AssistantUpdate::text_delta(delta),
                    });
                }
                ProviderEvent::ThinkingDelta(delta) => {
                    thinking.push_str(&delta);
                    self.emit(AgentEvent::MessageUpdate {
                        assistant_message_event: AssistantUpdate::thinking_delta(delta),
                    });
                }
                ProviderEvent::ToolCallStart { index, id, name } => {
                    pending.insert(index, (id.clone(), name.clone(), String::new()));
                    // The UI locates the block by index in the partial message.
                    let mut partial = blocks.clone();
                    partial.push(ContentBlock::ToolCall {
                        id: id.clone(),
                        name: name.clone(),
                        arguments: serde_json::Value::Null,
                    });
                    self.emit(AgentEvent::MessageUpdate {
                        assistant_message_event: AssistantUpdate::tool_call_start(
                            partial.len() - 1,
                            partial,
                        ),
                    });
                }
                ProviderEvent::ToolCallArgsDelta { index, delta } => {
                    if let Some(entry) = pending.get_mut(&index) {
                        entry.2.push_str(&delta);
                    }
                    self.emit(AgentEvent::MessageUpdate {
                        assistant_message_event: AssistantUpdate::tool_call_delta(delta),
                    });
                }
                ProviderEvent::ToolCallEnd { index, call } => {
                    pending.remove(&index);
                    self.emit(AgentEvent::MessageUpdate {
                        assistant_message_event: AssistantUpdate::tool_call_end(call.clone()),
                    });
                    calls.push(call);
                }
                ProviderEvent::Usage(usage) => {
                    self.stats.usage.add(usage);
                    let cost = self.model_info().cost.of(&usage);
                    self.stats.cost += cost;
                    self.deps.budget.spend(cost)?;
                }
                ProviderEvent::Done(reason) => {
                    stop = reason;
                    break;
                }
            }
        }

        // A provider that ended without a toolcall_end still has usable calls.
        for (index, (id, name, arguments)) in pending {
            let parsed = serde_json::from_str(&arguments)
                .unwrap_or_else(|_| serde_json::json!({ "__raw": arguments }));
            let call = ToolCall { id, name, arguments: parsed };
            self.emit(AgentEvent::MessageUpdate {
                assistant_message_event: AssistantUpdate::tool_call_end(call.clone()),
            });
            calls.push(call);
            let _ = index;
        }

        if !thinking.is_empty() {
            blocks.push(ContentBlock::Thinking { text: thinking });
        }
        if !text.is_empty() {
            blocks.push(ContentBlock::text(text));
        }
        blocks.extend(calls.into_iter().map(|call| ContentBlock::ToolCall {
            id: call.id,
            name: call.name,
            arguments: call.arguments,
        }));

        self.emit(AgentEvent::MessageEnd { stop_reason: Some(format!("{stop:?}")) });
        Ok(Message::assistant(blocks))
    }

    /// Run every tool call from one assistant message.
    ///
    /// Calls run concurrently when they are all read-only, and sequentially
    /// otherwise, because two writes to the same file must not race.
    async fn run_tools(&mut self, calls: Vec<ToolCall>) -> Result<()> {
        self.stats.tool_calls += calls.len();
        let all_reads = calls.iter().all(|call| {
            self.deps
                .tools
                .get(&call.name)
                .map(|tool| tool.class() == ToolClass::Read)
                .unwrap_or(false)
        });

        if all_reads && calls.len() > 1 {
            let mut futures = Vec::with_capacity(calls.len());
            for call in &calls {
                futures.push(self.execute_one(call));
            }
            let results = futures_util::future::join_all(futures).await;
            for result in results {
                self.messages.push(result);
            }
        } else {
            for call in &calls {
                let message = self.execute_one(call).await;
                self.messages.push(message);
            }
        }
        Ok(())
    }

    /// Approve (if needed) and run one tool call.
    ///
    /// Takes `&self` so read-only calls can be joined concurrently; the only
    /// mutation is the "always allow" set, which is behind its own lock.
    async fn execute_one(&self, call: &ToolCall) -> Message {
        let Some(tool) = self.deps.tools.get(&call.name) else {
            let message = format!("unknown tool `{}`", call.name);
            self.emit(AgentEvent::ToolExecutionEnd {
                tool_call_id: call.id.clone(),
                is_error: true,
            });
            return Message::tool(call.id.clone(), message, true);
        };

        let class = tool.class();
        let action = class.approval(&self.deps.approvals);
        let already_allowed =
            self.deps.always_allow.lock().map(|set| set.contains(tool.name())).unwrap_or(false);

        if action == ApprovalAction::Deny && !already_allowed {
            let message = format!("`{}` is denied by the approval policy", tool.name());
            self.emit(AgentEvent::ToolExecutionEnd {
                tool_call_id: call.id.clone(),
                is_error: true,
            });
            return Message::tool(call.id.clone(), message, true);
        }

        if action == ApprovalAction::Ask && !already_allowed {
            let answer = self
                .deps
                .ui
                .ask(UiRequest::Approval {
                    tool: tool.name().to_string(),
                    title: approval_title(class),
                    detail: approval_detail(&call.name, &call.arguments),
                })
                .await;
            match answer {
                UiAnswer::Allow => {}
                UiAnswer::AllowAlways => {
                    if let Ok(mut set) = self.deps.always_allow.lock() {
                        set.insert(tool.name().to_string());
                    }
                }
                UiAnswer::Deny => {
                    let message = format!("the user declined to run `{}`", tool.name());
                    self.emit(AgentEvent::ToolExecutionEnd {
                        tool_call_id: call.id.clone(),
                        is_error: true,
                    });
                    return Message::tool(call.id.clone(), message, true);
                }
            }
        }

        let ctx = ToolContext {
            cwd: self.deps.cwd.clone(),
            cancel: self.cancel.clone(),
            progress: None,
            bash_timeout_secs: self.deps.bash_timeout_secs,
        };

        let output = self
            .deps
            .tools
            .execute(
                &call.name,
                &call.id,
                call.arguments.clone(),
                &ctx,
                &self.agent,
                &self.deps.sink,
            )
            .await;

        Message::tool(call.id.clone(), output.content, output.is_error)
    }

    /// Summarise old turns when the context is getting full.
    async fn maybe_compact(&mut self) -> Result<()> {
        let config = self.deps.providers.config();
        let info = self.model_info();
        if !context::needs_compaction(
            &self.messages,
            &self.system,
            info.context_window,
            config.general.compact_at,
        ) {
            return Ok(());
        }

        let (head, tail) = context::split_for_compaction(&self.messages, 8);
        if head.is_empty() {
            return Ok(());
        }

        let (provider, model) = self.deps.providers.resolve(&self.model)?;
        let mut request = ChatRequest::new(model, head);
        request.system = Some(context::compaction_prompt().to_string());
        request.thinking = ThinkingLevel::Off;

        let mut summary = String::new();
        if let Ok(mut stream) = provider.stream(request).await {
            while let Some(Ok(event)) = futures_util::StreamExt::next(&mut stream).await {
                match event {
                    ProviderEvent::TextDelta(delta) => summary.push_str(&delta),
                    ProviderEvent::Done(_) => break,
                    _ => {}
                }
            }
        }

        if summary.trim().is_empty() {
            // A failed summary must not lose the conversation.
            return Ok(());
        }

        let mut rebuilt = Vec::with_capacity(tail.len() + 1);
        rebuilt.push(context::compaction_message(summary.trim()));
        rebuilt.extend(tail);
        self.messages = rebuilt;
        self.deps.sink.emit(Emitted::notice(
            "info",
            "compacted the conversation to stay inside the context window",
        ));
        Ok(())
    }
}

fn approval_title(class: ToolClass) -> String {
    match class {
        ToolClass::Read => "Wants to read a file".to_string(),
        ToolClass::Write => "Wants to change a file".to_string(),
        ToolClass::Bash => "Wants to run a command".to_string(),
        ToolClass::Network => "Wants to reach the network".to_string(),
    }
}

fn approval_detail(tool: &str, args: &serde_json::Value) -> String {
    match tool {
        "bash" => {
            args.get("command").and_then(|value| value.as_str()).unwrap_or_default().to_string()
        }
        "write" | "edit" | "multi_edit" | "read" => {
            args.get("path").and_then(|value| value.as_str()).unwrap_or_default().to_string()
        }
        _ => args.to_string(),
    }
}

/// Build the system prompt for a session.
pub fn build_system(config: &Config, cwd: &std::path::Path, role: Option<&str>) -> String {
    let role_config = role.map(|role| config.role(role));
    context::system_prompt(
        context::BASE_PROMPT,
        role,
        role_config.as_ref().and_then(|role| role.system.as_deref()),
        cwd,
        context::platform(),
        &context::today(),
        config.instructions.as_deref(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn budget_stops_runaway_spending() {
        let budget = Budget::new(1.0);
        assert!(budget.spend(0.5).is_ok());
        assert!(budget.spend(0.6).is_err());
        assert!((budget.spent() - 1.1).abs() < f64::EPSILON);
    }

    #[test]
    fn a_zero_limit_disables_the_guard() {
        let budget = Budget::new(0.0);
        assert!(budget.spend(1_000.0).is_ok());
    }

    #[test]
    fn approval_details_show_the_command() {
        let detail = approval_detail("bash", &serde_json::json!({"command": "rm -rf build"}));
        assert_eq!(detail, "rm -rf build");
    }

    #[test]
    fn approval_details_show_the_path() {
        let detail = approval_detail("write", &serde_json::json!({"path": "src/main.rs"}));
        assert_eq!(detail, "src/main.rs");
    }

    #[test]
    fn system_prompt_mentions_the_role() {
        let config = Config::default();
        let prompt = build_system(&config, std::path::Path::new("/work"), Some("reviewer"));
        assert!(prompt.contains("Your role: reviewer."));
    }
}
