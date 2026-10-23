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
    pub async fn execute(
        &self,
        plan: TaskPlan,
        ctx_base: &AgentContext,
        approvals: Arc<dyn ApprovalSink>,
        cancel: CancellationToken,
    ) -> Result<RunResult> {
        match plan {
            TaskPlan::Single { agent, task } => {
                self.transition(format!("single: {agent}"));
                let spec = self.registry.resolve(&agent)?;
                let out = self
                    .run_one(&spec, ctx_base, approvals, task, cancel)
                    .await?;
                Ok(out)
            }
            TaskPlan::FanOut { agents, join_prompt: _ } => {
                self.transition(format!("fan-out: {} agents", agents.len()));
                let mut handles = Vec::new();
                for (name, task) in agents {
                    let spec = self.registry.resolve(&name)?;
                    let ctx = self.child_ctx(ctx_base, &spec);
                    let c = cancel.child_token();
                    self.bus.publish(LunaEvent::new(
                        self.session_id,
                        Some(spec.name.clone()),
                        EventKind::AgentSpawned { agent_id: spec.name.clone(), parent: None, spec_name: spec.name.clone() },
                    ));
                    handles.push(tokio::spawn(async move {
                        let runner = AgentRunner::new(spec);
                        let mut history = vec![Message::user(task)];
                        let res = runner.run(&ctx, &mut history, c).await;
                        (res, history)
                    }));
                }
                let mut result = RunResult { outputs: HashMap::new(), errors: HashMap::new() };
                for h in handles {
                    match h.await {
                        Ok((Ok(text), _)) => {
                            result.outputs.insert(text.clone(), text.clone());
                        }
                        Ok((Err(e), _)) => {
                            result.errors.insert(e.to_string(), e.to_string());
                        }
                        Err(e) => {
                            result.errors.insert(format!("join error: {e}"), format!("join error: {e}"));
                        }
                    }
                }
                self.transition("fan-out joined");
                Ok(result)
            }
            TaskPlan::Supervised { supervisor, workers } => {
                self.transition(format!("supervised: {} workers under {supervisor}", workers.len()));
                let mut worker_results = RunResult { outputs: HashMap::new(), errors: HashMap::new() };
                let mut handles = Vec::new();
                for (name, task) in workers {
                    let spec = self.registry.resolve(&name)?;
                    let ctx = self.child_ctx(ctx_base, &spec);
                    let c = cancel.child_token();
                    self.bus.publish(LunaEvent::new(
                        self.session_id,
                        Some(spec.name.clone()),
                        EventKind::AgentSpawned { agent_id: spec.name.clone(), parent: Some(supervisor.clone()), spec_name: spec.name.clone() },
                    ));
                    handles.push(tokio::spawn(async move {
                        let runner = AgentRunner::new(spec);
                        let mut history = vec![Message::user(task)];
                        let res = runner.run(&ctx, &mut history, c).await;
                        (res, history)
                    }));
                }
                for h in handles {
                    match h.await {
                        Ok((Ok(text), _)) => {
                            worker_results.outputs.insert(text.clone(), text.clone());
                        }
                        Ok((Err(e), _)) => {
                            worker_results.errors.insert(e.to_string(), e.to_string());
                        }
                        Err(e) => {
                            worker_results.errors.insert(format!("join error: {e}"), format!("join error: {e}"));
                        }
                    }
                }

                // Supervisor synthesizes.
                let sup_spec = self.registry.resolve(&supervisor)?;
                let ctx = self.child_ctx(ctx_base, &sup_spec);
                let mut synth_history = Vec::new();
                let worker_summary = worker_results
                    .outputs
                    .iter()
                    .map(|(k, v)| format!("- {k}: {v}"))
                    .collect::<Vec<_>>()
                    .join("\n");
                let error_summary = worker_results
                    .errors
                    .iter()
                    .map(|(k, _)| format!("- {k}"))
                    .collect::<Vec<_>>()
                    .join("\n");
                synth_history.push(Message::user(format!(
                    "Worker results:\n{worker_summary}\n\nWorker errors:\n{error_summary}\n\nSynthesize a final answer."
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
        _approvals: Arc<dyn ApprovalSink>,
        task: String,
        cancel: CancellationToken,
    ) -> Result<RunResult> {
        let ctx = self.child_ctx(ctx_base, spec);
        let runner = AgentRunner::new(spec.clone());
        let mut history = vec![Message::user(task)];
        let text = runner.run(&ctx, &mut history, cancel).await?;
        let mut out = RunResult { outputs: HashMap::new(), errors: HashMap::new() };
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
        assert!(res.outputs.values().any(|v| v.contains("worker-a output")));
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
