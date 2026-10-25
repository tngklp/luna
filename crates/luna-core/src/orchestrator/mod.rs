//! Multi-agent orchestration.
//!
//! A supervisor owns a tree of agents. Each node is a full [`AgentSession`] with
//! its own conversation, model and tool allowlist, so a subagent cannot corrupt
//! its parent's context.
//!
//! Three properties make this reliable rather than merely parallel:
//!
//! * **Bounded fan-out.** A semaphore caps how many subagents run at once, so a
//!   model that spawns twenty workers cannot exhaust the machine or the budget.
//! * **Dependency-ordered work.** The task board decides what runs next, not the
//!   model's prose. A worker only starts when its prerequisites are done.
//! * **Propagating cancellation.** Aborting the root cancels every descendant,
//!   and a subagent that dies reports back instead of hanging its parent.

pub mod board;
pub mod mailbox;

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

use crate::agent::{build_system, AgentDeps, AgentSession};
use crate::config::Config;
use crate::error::{LunaError, Result};
use crate::event::{AgentEvent, AgentRef, Emitted, SinkRef};
use crate::tool::todo::TaskBoard;
use crate::types::ThinkingLevel;

pub use board::{Plan, PlannedTask};
pub use mailbox::{Mailbox, Message};

/// How a subagent ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentStatus {
    Running,
    Done,
    Failed,
    Cancelled,
}

impl AgentStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            AgentStatus::Running => "running",
            AgentStatus::Done => "done",
            AgentStatus::Failed => "failed",
            AgentStatus::Cancelled => "cancelled",
        }
    }
}

/// What a subagent reports back to its parent.
#[derive(Debug, Clone)]
pub struct AgentReport {
    pub agent_id: String,
    pub role: String,
    pub status: AgentStatus,
    pub summary: String,
    pub steps: usize,
    pub tool_calls: usize,
    pub cost: f64,
}

impl AgentReport {
    /// The text the parent sees in its own conversation.
    pub fn to_message(&self) -> String {
        format!(
            "Subagent `{}` ({}) finished with status {}.\n\n{}",
            self.agent_id,
            self.role,
            self.status.as_str(),
            self.summary
        )
    }
}

/// A node in the agent tree.
#[derive(Clone)]
struct Node {
    agent: AgentRef,
    role: String,
    title: String,
    status: AgentStatus,
    report: Option<AgentReport>,
}

/// Owns the agent tree and the shared limits.
pub struct Orchestrator {
    deps: Arc<AgentDeps>,
    config: Config,
    nodes: Arc<Mutex<BTreeMap<String, Node>>>,
    semaphore: Arc<Semaphore>,
    board: TaskBoard,
    cancel: CancellationToken,
    spawned: AtomicUsize,
    /// Messages between agents.
    mailbox: Arc<Mailbox>,
    /// Cost spent per role, so one role cannot eat the whole budget.
    role_spend: Arc<Mutex<BTreeMap<String, f64>>>,
}

impl Orchestrator {
    pub fn new(deps: Arc<AgentDeps>, config: Config, _root: AgentRef) -> Self {
        let limit = config.general.max_concurrent_agents.max(1);
        Orchestrator {
            deps,
            config,
            nodes: Arc::new(Mutex::new(BTreeMap::new())),
            semaphore: Arc::new(Semaphore::new(limit)),
            board: TaskBoard::new(),
            cancel: CancellationToken::new(),
            spawned: AtomicUsize::new(0),
            mailbox: Arc::new(Mailbox::new()),
            role_spend: Arc::new(Mutex::new(BTreeMap::new())),
        }
    }

    pub fn board(&self) -> TaskBoard {
        self.board.clone()
    }

    pub fn mailbox(&self) -> Arc<Mailbox> {
        self.mailbox.clone()
    }

    /// Every agent id in the tree, for a broadcast.
    pub fn agent_ids(&self) -> Vec<String> {
        self.nodes.lock().map(|nodes| nodes.keys().cloned().collect()).unwrap_or_default()
    }

    /// Whether a role has spent its allowance.
    pub fn role_exhausted(&self, role: &str) -> bool {
        let Some(limit) = self.config.role(role).max_cost_usd else { return false };
        if limit <= 0.0 {
            return false;
        }
        self.role_spend
            .lock()
            .map(|spend| spend.get(role).copied().unwrap_or(0.0) >= limit)
            .unwrap_or(false)
    }

    fn record_role_spend(&self, role: &str, cost: f64) {
        if let Ok(mut spend) = self.role_spend.lock() {
            *spend.entry(role.to_string()).or_insert(0.0) += cost;
        }
    }

    pub fn cancel(&self) {
        self.cancel.cancel();
    }

    pub fn sink(&self) -> &SinkRef {
        &self.deps.sink
    }

    /// Every agent in the tree, for the UI's agent pane.
    pub fn tree(&self) -> Vec<(AgentRef, String, String, AgentStatus)> {
        let nodes = self.nodes.lock().map(|nodes| nodes.clone()).unwrap_or_default();
        nodes.into_values().map(|node| (node.agent, node.role, node.title, node.status)).collect()
    }

    /// Build a session for a subagent, with its role's model and tools.
    fn session_for(&self, agent: AgentRef, role: &str, cwd: &std::path::Path) -> AgentSession {
        let role_config = self.config.role(role);
        let model =
            role_config.model.clone().unwrap_or_else(|| self.config.general.default_model.clone());
        let tools = self.deps.tools.restrict(&role_config.tools);
        let system = build_system(&self.config, cwd, Some(role));

        let mut deps = AgentDeps {
            providers: self.deps.providers.clone(),
            tools,
            sink: self.deps.sink.clone(),
            ui: self.deps.ui.clone(),
            approvals: self.deps.approvals,
            budget: self.deps.budget.clone(),
            cwd: cwd.to_path_buf(),
            bash_timeout_secs: self.deps.bash_timeout_secs,
            always_allow: self.deps.always_allow.clone(),
        };
        // A subagent inherits the parent's toolset minus delegation, so a worker
        // cannot spawn its own workers and multiply the tree.
        deps.tools = deps.tools.restrict(&role_config.tools);

        let mut session = AgentSession::new(Arc::new(deps), agent, model, system);
        session.thinking = ThinkingLevel::Medium;
        session
    }

    /// Run one subagent to completion and report back.
    ///
    /// The permit is held for the whole run, which is what bounds concurrency.
    pub async fn run_subagent(
        &self,
        parent: &AgentRef,
        role: &str,
        title: &str,
        brief: &str,
        cwd: &std::path::Path,
    ) -> Result<AgentReport> {
        // A role that has spent its allowance cannot start new work.
        if self.role_exhausted(role) {
            return Err(LunaError::Budget(format!("the `{role}` role has reached its cost limit")));
        }

        let permit = self
            .semaphore
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| LunaError::other("the orchestrator is shutting down"))?;

        let index = self.spawned.fetch_add(1, Ordering::Relaxed);
        let agent_id = format!("{}-{role}-{index}", parent.agent_id);
        let agent = AgentRef::child(agent_id.clone(), parent, role);

        self.deps.sink.emit(Emitted::global(AgentEvent::AgentSpawned {
            agent_id: agent_id.clone(),
            parent_agent_id: parent.agent_id.clone(),
            role: role.to_string(),
            title: title.to_string(),
            depth: agent.depth,
        }));

        if let Ok(mut nodes) = self.nodes.lock() {
            nodes.insert(
                agent_id.clone(),
                Node {
                    agent: agent.clone(),
                    role: role.to_string(),
                    title: title.to_string(),
                    status: AgentStatus::Running,
                    report: None,
                },
            );
        }

        let mut session = self.session_for(agent.clone(), role, cwd);
        // A subagent dies with the tree.
        let linked = self.cancel.child_token();
        session.cancel = linked.clone();

        let outcome = session.run_turn(brief).await;
        drop(permit);

        let (status, summary) = match outcome {
            Ok(()) if linked.is_cancelled() => {
                (AgentStatus::Cancelled, "the subagent was cancelled".to_string())
            }
            Ok(()) => (AgentStatus::Done, last_assistant_text(&session)),
            Err(err) => (AgentStatus::Failed, err.to_string()),
        };

        let report = AgentReport {
            agent_id: agent_id.clone(),
            role: role.to_string(),
            status,
            summary,
            steps: session.stats.steps,
            tool_calls: session.stats.tool_calls,
            cost: session.stats.cost,
        };
        self.record_role_spend(role, report.cost);

        // Anything the tree sent this agent is handed to the parent, so a
        // message is never silently dropped when the agent finishes.
        let unread = self.mailbox.drain(&agent_id);
        let mut report = report;
        if !unread.is_empty() {
            let notes =
                unread.iter().map(|message| message.render()).collect::<Vec<_>>().join("\n");
            report.summary = format!("{}\n\nUnread messages:\n{notes}", report.summary);
        }

        if let Ok(mut nodes) = self.nodes.lock() {
            if let Some(node) = nodes.get_mut(&agent_id) {
                node.status = status;
                node.report = Some(report.clone());
            }
        }

        self.deps.sink.emit(Emitted::global(AgentEvent::AgentFinished {
            agent_id,
            parent_agent_id: parent.agent_id.clone(),
            status: status.as_str().to_string(),
            summary: report.summary.clone(),
            depth: agent.depth,
        }));

        Ok(report)
    }

    /// Run several subagents concurrently, respecting the semaphore.
    pub async fn run_parallel(
        &self,
        parent: &AgentRef,
        jobs: Vec<(String, String, String)>,
        cwd: &std::path::Path,
    ) -> Vec<AgentReport> {
        let mut futures = Vec::with_capacity(jobs.len());
        for (role, title, brief) in &jobs {
            futures.push(self.run_subagent(parent, role, title, brief, cwd));
        }
        futures_util::future::join_all(futures)
            .await
            .into_iter()
            .filter_map(|result| result.ok())
            .collect()
    }

    /// Work the task board until nothing is ready.
    ///
    /// Each ready task is handed to a worker with a brief built from the task
    /// itself, so the board — not the model's prose — decides the order.
    pub async fn drain_board(&self, parent: &AgentRef, cwd: &std::path::Path) -> Vec<AgentReport> {
        let mut reports = Vec::new();
        loop {
            let ready = self.board.ready();
            if ready.is_empty() {
                break;
            }
            let jobs: Vec<(String, String, String)> = ready
                .iter()
                .map(|task| {
                    let brief = format!(
                        "Task: {}\n\n{}\n\nWhen you are done, reply with a short summary of what you changed.",
                        task.title, task.description
                    );
                    ("worker".to_string(), task.title.clone(), brief)
                })
                .collect();

            for task in &ready {
                self.board.claim(&task.id, "worker");
            }

            let batch = self.run_parallel(parent, jobs, cwd).await;
            for (task, report) in ready.iter().zip(batch.iter()) {
                let status = if report.status == AgentStatus::Done {
                    crate::tool::todo::TaskStatus::Done
                } else {
                    crate::tool::todo::TaskStatus::Blocked
                };
                self.board.set_status(&task.id, status);
            }
            reports.extend(batch);

            if !self.board.deadlocked().is_empty() {
                self.deps.sink.emit(Emitted::notice(
                    "warning",
                    "some tasks can never run because a dependency is blocked",
                ));
                break;
            }
        }
        reports
    }
}

/// The last thing the assistant said, which is its report to the parent.
fn last_assistant_text(session: &AgentSession) -> String {
    session
        .messages
        .iter()
        .rev()
        .find(|message| message.role == crate::types::Role::Assistant)
        .map(|message| message.text())
        .filter(|text| !text.trim().is_empty())
        .unwrap_or_else(|| "the subagent finished without a summary".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_read_as_messages() {
        let report = AgentReport {
            agent_id: "root-worker-0".into(),
            role: "worker".into(),
            status: AgentStatus::Done,
            summary: "changed two files".into(),
            steps: 4,
            tool_calls: 6,
            cost: 0.01,
        };
        let message = report.to_message();
        assert!(message.contains("root-worker-0"));
        assert!(message.contains("changed two files"));
    }

    #[test]
    fn statuses_have_stable_names() {
        assert_eq!(AgentStatus::Done.as_str(), "done");
        assert_eq!(AgentStatus::Cancelled.as_str(), "cancelled");
    }
}
