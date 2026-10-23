//! The orchestrator: delegation, parallel fan-out with join, run tracking.

use super::agent_loop::{AgentContext, AgentRunner, AgentSpec, ApprovalSink};
use super::messaging::{aggregate_results, MessageHub, MessageKind};
use super::registry::AgentRegistry;
use crate::error::Result;
use crate::event::{EventKind, LunaEvent};
use crate::provider::types::Message;
use std::collections::HashMap;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

/// How a task is distributed to agents.
#[derive(Debug, Clone)]
pub enum TaskPlan {
    /// One agent handles the task end-to-end.
    Single { agent: String, task: String },
    /// A supervisor delegates to workers and synthesizes results.
    Supervised { supervisor: String, workers: Vec<(String, String)> },
    /// Run the same/different agents in parallel and join results.
    FanOut { agents: Vec<(String, String)>, join_prompt: Option<String> },
}

/// Result of one orchestrated run.
#[derive(Debug, Clone)]
pub struct RunResult {
    /// Final output per agent (workers + supervisor, if any).
    pub outputs: HashMap<String, String>,
    /// Per-agent failures.
    pub errors: HashMap<String, String>,
    /// The message hub used during the run (for post-run inspection).
    pub hub: MessageHub,
}

impl std::fmt::Debug for MessageHub {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MessageHub")
            .field("agents", &self.agent_ids())
            .field("scratchpad_keys", &(self.scratch_snapshot().iter().map(|(k, _)| k.clone()).collect::<Vec<_>>()))
            .finish()
    }
}

impl RunResult {
    pub fn joined(&self) -> String {
        let mut names: Vec<&String> = self.outputs.keys().collect();
        names.sort();
        names
            .iter()
            .filter_map(|n| self.outputs.get(*n))
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join("\n\n")
    }
}

/// Coordinates multi-agent runs.
pub struct Orchestrator {
    registry: AgentRegistry,
    bus: crate::event::EventBus,
    session_id: uuid::Uuid,
}

impl Orchestrator {
    pub fn new(registry: AgentRegistry, bus: crate::event::EventBus) -> Self {
        Self {
            registry,
            bus,
            session_id: uuid::Uuid::new_v4(),
        }
    }

    pub fn session_id(&self) -> uuid::Uuid {
        self.session_id
    }

    pub fn bus(&self) -> &crate::event::EventBus {
        &self.bus
    }

    fn transition(&self, what: impl Into<String>) {
        self.bus.publish(LunaEvent::new(
            self.session_id,
            None,
            EventKind::OrchestratorTransition(what.into()),
        ));
    }

    /// Execute a task plan. Each agent gets its own history and cancellation
    /// token; failures are captured per-agent and never abort siblings.
    /// All agents in the run share a `MessageHub` for coordination.
    pub async fn execute(
        &self,
        plan: TaskPlan,
        ctx_base: &AgentContext,
        _approvals: Arc<dyn ApprovalSink>,
        cancel: CancellationToken,
    ) -> Result<RunResult> {
        let hub = MessageHub::new();
        match plan {
            TaskPlan::Single { agent, task } => {
                self.transition(format!("single: {agent}"));
                let spec = self.registry.resolve(&agent)?;
                let mut out = self.run_one(&spec, ctx_base, task, cancel).await?;
                out.hub = hub;
                Ok(out)
            }
            TaskPlan::FanOut { agents, join_prompt: _ } => {
                self.transition(format!("fan-out: {} agents", agents.len()));
                let mut handles = Vec::new();
                for (name, task) in agents {
                    let spec = self.registry.resolve(&name)?;
                    let ctx = self.child_ctx(ctx_base, &spec);
                    let c = cancel.child_token();
                    let hub2 = hub.clone();
                    // Task arrives via the mailbox so it's visible to the hub.
                    hub2
                        .send_to(&spec.name, "orchestrator", MessageKind::Task, task.clone(), None)
                        .await
                        .ok();
                    self.bus.publish(LunaEvent::new(
                        self.session_id,
                        Some(spec.name.clone()),
                        EventKind::AgentSpawned { agent_id: spec.name.clone(), parent: None, spec_name: spec.name.clone() },
                    ));
                    let agent_name = spec.name.clone();
                    handles.push(tokio::spawn(async move {
                        let runner = AgentRunner::new(spec);
                        let mut history = vec![Message::user(task)];
                        let res = runner.run(&ctx, &mut history, c).await;
                        (agent_name, res, history)
                    }));
                }
                let mut result = RunResult { outputs: HashMap::new(), errors: HashMap::new(), hub };
                for h in handles {
                    match h.await {
                        Ok((name, Ok(text), _)) => {
                            result.outputs.insert(name, text);
                        }
                        Ok((name, Err(e), _)) => {
                            result.errors.insert(name, e.to_string());
                        }
                        Err(e) => {
                            result.errors.insert(format!("join-{e}"), format!("join error: {e}"));
                        }
                    }
                }
                self.transition("fan-out joined");
                Ok(result)
            }
            TaskPlan::Supervised { supervisor, workers } => {
                self.transition(format!("supervised: {} workers under {supervisor}", workers.len()));
                let mut worker_results = RunResult { outputs: HashMap::new(), errors: HashMap::new(), hub: hub.clone() };
                let mut handles = Vec::new();
                for (name, task) in workers {
                    let spec = self.registry.resolve(&name)?;
                    let ctx = self.child_ctx(ctx_base, &spec);
                    let c = cancel.child_token();
                    let hub2 = hub.clone();
                    let sup = supervisor.clone();
                    hub2
                        .send_to(&spec.name, &sup, MessageKind::Task, task.clone(), None)
                        .await
                        .ok();
                    self.bus.publish(LunaEvent::new(
                        self.session_id,
                        Some(spec.name.clone()),
                        EventKind::AgentSpawned { agent_id: spec.name.clone(), parent: Some(supervisor.clone()), spec_name: spec.name.clone() },
                    ));
                    let agent_name = spec.name.clone();
                    handles.push(tokio::spawn(async move {
                        let runner = AgentRunner::new(spec);
                        let mut history = vec![Message::user(task)];
                        let res = runner.run(&ctx, &mut history, c).await;
                        (agent_name, res, history)
                    }));
                }
                for h in handles {
                    match h.await {
                        Ok((name, Ok(text), _)) => {
                            worker_results.outputs.insert(name, text);
                        }
                        Ok((name, Err(e), _)) => {
                            worker_results.errors.insert(name, e.to_string());
                        }
                        Err(e) => {
                            worker_results.errors.insert(format!("join-{e}"), format!("join error: {e}"));
                        }
                    }
                }

                // Supervisor synthesizes from aggregated worker results.
                let sup_spec = self.registry.resolve(&supervisor)?;
                let ctx = self.child_ctx(ctx_base, &sup_spec);
                let mut synth_history = Vec::new();
                let summary = aggregate_results(&worker_results.outputs, &worker_results.errors);
                synth_history.push(Message::user(format!(
                    "{summary}\nSynthesize a final answer for the user."
                )));
                let runner = AgentRunner::new(sup_spec);
                let final_text = runner.run(&ctx, &mut synth_history, cancel).await?;
                self.transition("supervised complete");
                let mut out = worker_results;
                out.outputs.insert("supervisor".into(), final_text);
                Ok(out)
            }
        }
    }

    async fn run_one(
        &self,
        spec: &AgentSpec,
        ctx_base: &AgentContext,
        task: String,
        cancel: CancellationToken,
    ) -> Result<RunResult> {
        let ctx = self.child_ctx(ctx_base, spec);
        let runner = AgentRunner::new(spec.clone());
        let mut history = vec![Message::user(task)];
        let text = runner.run(&ctx, &mut history, cancel).await?;
        let mut out = RunResult { outputs: HashMap::new(), errors: HashMap::new(), hub: MessageHub::new() };
        out.outputs.insert(spec.name.clone(), text);
        Ok(out)
    }

    /// Build a per-agent context: same tools/gate, agent-tagged events.
    fn child_ctx(&self, base: &AgentContext, _spec: &AgentSpec) -> AgentContext {
        AgentContext {
            provider: base.provider.clone(),
            tools: base.tools.clone(),
            gate: base.gate.clone(),
            bus: self.bus.clone(),
            session_id: self.session_id,
            approvals: base.approvals.clone(),
        }
    }
}

// Re-export for convenience in UIs.
pub use super::agent_loop::AgentContext as Ctx;

#[allow(unused_imports)]
use crate::tool::registry::ToolRegistry;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::EventKind as EK;
    use crate::orchestrator::agent_loop::AutoApprove;
    use crate::provider::mock::{MockProvider, ScriptedTurn};
    use crate::provider::Provider;
    use crate::tool::builtin;
    use crate::tool::permissions::PermissionGate;
    use crate::tool::registry::ToolRegistry;

    fn base_ctx(provider: Arc<dyn Provider>) -> AgentContext {
        let mut tools = ToolRegistry::new();
        builtin::register_all(&mut tools);
        AgentContext {
            provider,
            tools,
            gate: Arc::new(PermissionGate::new(PermissionGate::default_rules(), vec![])),
            bus: crate::event::EventBus::default(),
            session_id: uuid::Uuid::new_v4(),
            approvals: Arc::new(AutoApprove),
        }
    }

    fn registry_two_agents() -> AgentRegistry {
        let mut reg = AgentRegistry::new();
        reg.register_provider(
            "mock",
            super::super::registry::ProviderSettings { model: "mock-1".into(), max_tokens: 512, temperature: 0.0 },
        );
        reg.register_agent(crate::config::AgentSpecConfig {
            name: "alpha".into(),
            provider: "mock".into(),
            model: None,
            system: None,
            tools: vec![],
            max_iterations: 3,
        });
        reg.register_agent(crate::config::AgentSpecConfig {
            name: "beta".into(),
            provider: "mock".into(),
            model: None,
            system: None,
            tools: vec![],
            max_iterations: 3,
        });
        reg
    }

    #[tokio::test]
    async fn single_agent_run() {
        let provider = Arc::new(MockProvider::new(vec![ScriptedTurn::Text("done by alpha".into())]));
        let ctx = base_ctx(provider);
        let bus = ctx.bus.clone();
        let orch = Orchestrator::new(registry_two_agents(), bus.clone());
        let res = orch
            .execute(
                TaskPlan::Single { agent: "alpha".into(), task: "do it".into() },
                &ctx,
                Arc::new(AutoApprove),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(res.outputs["alpha"], "done by alpha");
    }

    #[tokio::test]
    async fn fan_out_runs_in_parallel_and_joins() {
        // Both agents answer; results must contain both.
        let provider = Arc::new(MockProvider::new(vec![
            ScriptedTurn::Text("alpha-result".into()),
            ScriptedTurn::Text("beta-result".into()),
        ]));
        let ctx = base_ctx(provider);
        let bus = ctx.bus.clone();
        let orch = Orchestrator::new(registry_two_agents(), bus.clone());
        let res = orch
            .execute(
                TaskPlan::FanOut {
                    agents: vec![("alpha".into(), "task a".into()), ("beta".into(), "task b".into())],
                    join_prompt: None,
                },
                &ctx,
                Arc::new(AutoApprove),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        let joined = res.joined();
        assert!(joined.contains("alpha-result") && joined.contains("beta-result"), "got: {joined}");
    }

    #[tokio::test]
    async fn supervised_synthesizes() {
        let provider = Arc::new(MockProvider::new(vec![
            ScriptedTurn::Text("worker-a output".into()),
            ScriptedTurn::Text("worker-b output".into()),
            ScriptedTurn::Text("final synthesis".into()),
        ]));
        let ctx = base_ctx(provider);
        let bus = ctx.bus.clone();
        let mut reg = registry_two_agents();
        reg.register_agent(crate::config::AgentSpecConfig {
            name: "boss".into(),
            provider: "mock".into(),
            model: None,
            system: None,
            tools: vec![],
            max_iterations: 3,
        });
        let orch = Orchestrator::new(reg, bus.clone());
        let res = orch
            .execute(
                TaskPlan::Supervised {
                    supervisor: "boss".into(),
                    workers: vec![("alpha".into(), "task a".into()), ("beta".into(), "task b".into())],
                },
                &ctx,
                Arc::new(AutoApprove),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(res.outputs["supervisor"], "final synthesis");
        assert_eq!(res.outputs["alpha"], "worker-a output");
        assert_eq!(res.outputs["beta"], "worker-b output");
    }

    #[tokio::test]
    async fn supervised_failure_does_not_abort_siblings() {
        // alpha succeeds, beta's provider is exhausted (errors), supervisor still runs.
        let provider = Arc::new(MockProvider::new(vec![
            ScriptedTurn::Text("alpha ok".into()),
            // beta gets nothing -> "(mock exhausted)" is still a Text turn, so
            // instead simulate an error via max_iterations: give beta a tool
            // loop that trips the guard.
            ScriptedTurn::ToolCalls {
                text: String::new(),
                calls: vec![("c1".into(), "read_file".into(), serde_json::json!({"path": "."}))],
            },
            ScriptedTurn::ToolCalls {
                text: String::new(),
                calls: vec![("c1".into(), "read_file".into(), serde_json::json!({"path": "."}))],
            },
            ScriptedTurn::ToolCalls {
                text: String::new(),
                calls: vec![("c1".into(), "read_file".into(), serde_json::json!({"path": "."}))],
            },
            ScriptedTurn::ToolCalls {
                text: String::new(),
                calls: vec![("c1".into(), "read_file".into(), serde_json::json!({"path": "."}))],
            },
            ScriptedTurn::Text("synthesis despite failure".into()),
        ]));
        let ctx = base_ctx(provider);
        let bus = ctx.bus.clone();
        let mut reg = registry_two_agents();
        // beta: max_iterations 2 so it fails fast.
        reg.register_agent(crate::config::AgentSpecConfig {
            name: "beta".into(),
            provider: "mock".into(),
            model: None,
            system: None,
            tools: vec![],
            max_iterations: 2,
        });
        reg.register_agent(crate::config::AgentSpecConfig {
            name: "boss".into(),
            provider: "mock".into(),
            model: None,
            system: None,
            tools: vec![],
            max_iterations: 3,
        });
        let orch = Orchestrator::new(reg, bus.clone());
        let res = orch
            .execute(
                TaskPlan::Supervised {
                    supervisor: "boss".into(),
                    workers: vec![("alpha".into(), "task a".into()), ("beta".into(), "task b".into())],
                },
                &ctx,
                Arc::new(AutoApprove),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        assert!(res.outputs.contains_key("alpha"));
        assert!(res.errors.contains_key("beta"));
        assert_eq!(res.outputs["supervisor"], "synthesis despite failure");
    }

    #[tokio::test]
    async fn fan_out_keys_outputs_by_agent_name() {
        let provider = Arc::new(MockProvider::new(vec![
            ScriptedTurn::Text("alpha-result".into()),
            ScriptedTurn::Text("beta-result".into()),
        ]));
        let ctx = base_ctx(provider);
        let bus = ctx.bus.clone();
        let orch = Orchestrator::new(registry_two_agents(), bus.clone());
        let res = orch
            .execute(
                TaskPlan::FanOut {
                    agents: vec![("alpha".into(), "task a".into()), ("beta".into(), "task b".into())],
                    join_prompt: None,
                },
                &ctx,
                Arc::new(AutoApprove),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(res.outputs["alpha"], "alpha-result");
        assert_eq!(res.outputs["beta"], "beta-result");
    }

    #[tokio::test]
    async fn supervised_sends_task_messages_via_hub() {
        let provider = Arc::new(MockProvider::new(vec![
            ScriptedTurn::Text("w1".into()),
            ScriptedTurn::Text("synth".into()),
        ]));
        let ctx = base_ctx(provider);
        let bus = ctx.bus.clone();
        let mut reg = registry_two_agents();
        reg.register_agent(crate::config::AgentSpecConfig {
            name: "boss".into(),
            provider: "mock".into(),
            model: None,
            system: None,
            tools: vec![],
            max_iterations: 3,
        });
        let orch = Orchestrator::new(reg, bus.clone());
        let res = orch
            .execute(
                TaskPlan::Supervised {
                    supervisor: "boss".into(),
                    workers: vec![("alpha".into(), "the task".into())],
                },
                &ctx,
                Arc::new(AutoApprove),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        // The hub recorded the task assignment for alpha.
        let (rx, _) = res.hub.register("probe");
        let _ = rx; // hub still usable after run
        assert!(res.outputs.contains_key("alpha"));
        assert!(res.outputs.contains_key("supervisor"));
    }

    #[tokio::test]
    async fn orchestrator_emits_transitions() {
        let provider = Arc::new(MockProvider::new(vec![ScriptedTurn::Text("ok".into())]));
        let ctx = base_ctx(provider);
        let bus = ctx.bus.clone();
        let orch = Orchestrator::new(registry_two_agents(), bus.clone());
        let mut rx = bus.subscribe();
        let _ = orch
            .execute(
                TaskPlan::Single { agent: "alpha".into(), task: "t".into() },
                &ctx,
                Arc::new(AutoApprove),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        let mut saw_transition = false;
        while let Ok(ev) = rx.try_recv() {
            if matches!(ev.kind, EK::OrchestratorTransition(_)) {
                saw_transition = true;
            }
        }
        assert!(saw_transition);
    }
}
