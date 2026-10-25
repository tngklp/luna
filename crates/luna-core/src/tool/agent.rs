//! Delegation tools.
//!
//! These are what make the orchestrator reachable from the model: `spawn_agent`
//! runs one subagent, `spawn_agents` runs several in parallel, and
//! `run_plan` turns a plan into board work. Each returns a structured report the
//! parent can reason about, never free-form prose it has to parse.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};

use super::{required_str, Tool, ToolClass, ToolContext, ToolOutput};
use crate::error::{LunaError, Result};
use crate::event::AgentRef;
use crate::orchestrator::board::{planner_brief, Plan};
use crate::orchestrator::Orchestrator;

/// Shared handle so every delegation tool talks to the same tree.
#[derive(Clone)]
pub struct Delegation {
    orchestrator: Arc<Orchestrator>,
    parent: AgentRef,
}

impl Delegation {
    pub fn new(
        orchestrator: Arc<Orchestrator>,
        parent: AgentRef,
        _cwd: impl Into<PathBuf>,
    ) -> Self {
        Delegation { orchestrator, parent }
    }
}

pub struct SpawnAgentTool {
    delegation: Delegation,
}

impl SpawnAgentTool {
    pub fn new(delegation: Delegation) -> Self {
        SpawnAgentTool { delegation }
    }
}

#[async_trait]
impl Tool for SpawnAgentTool {
    fn name(&self) -> &str {
        "spawn_agent"
    }

    fn description(&self) -> &str {
        "Run one subagent on a self-contained brief and wait for its report. The \
         subagent cannot see this conversation, so the brief must include every \
         fact it needs. Use it for work you can hand off whole."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "role": {
                    "type": "string",
                    "description": "Role preset: lead, planner, worker, reviewer or researcher",
                    "enum": ["lead", "planner", "worker", "reviewer", "researcher"]
                },
                "title": { "type": "string", "description": "Short label shown in the agent tree" },
                "brief": { "type": "string", "description": "Complete, self-contained instructions" }
            },
            "required": ["role", "title", "brief"]
        })
    }

    fn class(&self) -> ToolClass {
        ToolClass::Read
    }

    async fn run(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutput> {
        let role = required_str(&args, "role")?;
        let title = required_str(&args, "title")?;
        let brief = required_str(&args, "brief")?;
        let report = self
            .delegation
            .orchestrator
            .run_subagent(&self.delegation.parent, &role, &title, &brief, &ctx.cwd)
            .await?;
        Ok(ToolOutput {
            content: report.to_message(),
            is_error: report.status != crate::orchestrator::AgentStatus::Done,
        })
    }
}

pub struct SpawnAgentsTool {
    delegation: Delegation,
}

impl SpawnAgentsTool {
    pub fn new(delegation: Delegation) -> Self {
        SpawnAgentsTool { delegation }
    }
}

#[async_trait]
impl Tool for SpawnAgentsTool {
    fn name(&self) -> &str {
        "spawn_agents"
    }

    fn description(&self) -> &str {
        "Run several subagents in parallel and wait for all of them. Use it when \
         the jobs are genuinely independent; concurrency is capped by config."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "agents": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "role": { "type": "string" },
                            "title": { "type": "string" },
                            "brief": { "type": "string" }
                        },
                        "required": ["role", "title", "brief"]
                    }
                }
            },
            "required": ["agents"]
        })
    }

    fn class(&self) -> ToolClass {
        ToolClass::Read
    }

    async fn run(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutput> {
        let entries = args
            .get("agents")
            .and_then(|agents| agents.as_array())
            .ok_or_else(|| LunaError::tool("`agents` must be an array"))?;
        if entries.is_empty() {
            return Err(LunaError::tool("`agents` must not be empty"));
        }

        let mut jobs = Vec::with_capacity(entries.len());
        for entry in entries {
            jobs.push((
                required_str(entry, "role")?,
                required_str(entry, "title")?,
                required_str(entry, "brief")?,
            ));
        }

        let reports = self
            .delegation
            .orchestrator
            .run_parallel(&self.delegation.parent, jobs, &ctx.cwd)
            .await;

        let failed =
            reports.iter().any(|report| report.status != crate::orchestrator::AgentStatus::Done);
        let body = reports
            .iter()
            .map(|report| report.to_message())
            .collect::<Vec<_>>()
            .join("\n\n---\n\n");
        Ok(ToolOutput { content: body, is_error: failed })
    }
}

pub struct RunPlanTool {
    delegation: Delegation,
}

impl RunPlanTool {
    pub fn new(delegation: Delegation) -> Self {
        RunPlanTool { delegation }
    }
}

#[async_trait]
impl Tool for RunPlanTool {
    fn name(&self) -> &str {
        "run_plan"
    }

    fn description(&self) -> &str {
        "Execute a dependency-ordered plan. Tasks run as soon as their \
         prerequisites finish, so independent work overlaps automatically."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "tasks": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "id": { "type": "string" },
                            "title": { "type": "string" },
                            "description": { "type": "string" },
                            "dependsOn": { "type": "array", "items": { "type": "string" } },
                            "role": { "type": "string" }
                        },
                        "required": ["id", "title"]
                    }
                }
            },
            "required": ["tasks"]
        })
    }

    fn class(&self) -> ToolClass {
        ToolClass::Read
    }

    async fn run(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutput> {
        let plan: Plan = serde_json::from_value(args)
            .map_err(|err| LunaError::tool(format!("invalid plan: {err}")))?;
        plan.validate()?;
        plan.apply(&self.delegation.orchestrator.board());

        let reports =
            self.delegation.orchestrator.drain_board(&self.delegation.parent, &ctx.cwd).await;

        if reports.is_empty() {
            return Ok(ToolOutput::ok("the plan produced no runnable tasks"));
        }
        let failed =
            reports.iter().any(|report| report.status != crate::orchestrator::AgentStatus::Done);
        let body = reports
            .iter()
            .map(|report| report.to_message())
            .collect::<Vec<_>>()
            .join("\n\n---\n\n");
        Ok(ToolOutput { content: body, is_error: failed })
    }
}

pub struct PlanTool;

#[async_trait]
impl Tool for PlanTool {
    fn name(&self) -> &str {
        "plan"
    }

    fn description(&self) -> &str {
        "Get the instructions for writing a plan. Call this before `run_plan` \
         when the work is large enough to need several agents."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "goal": { "type": "string", "description": "What the plan must achieve" }
            },
            "required": ["goal"]
        })
    }

    fn class(&self) -> ToolClass {
        ToolClass::Read
    }

    async fn run(&self, args: Value, _ctx: &ToolContext) -> Result<ToolOutput> {
        Ok(ToolOutput::ok(planner_brief(&required_str(&args, "goal")?)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delegation_tools_advertise_schemas() {
        let orchestrator = Arc::new(Orchestrator::new(
            Arc::new(crate::agent::AgentDeps::new(
                Arc::new(crate::provider::ProviderRegistry::new(&crate::config::Config::default())),
                Arc::new(crate::event::NullSink),
                ".",
            )),
            crate::config::Config::default(),
            AgentRef::root("root"),
        ));
        let delegation = Delegation::new(orchestrator, AgentRef::root("root"), ".");
        let tool = SpawnAgentTool::new(delegation.clone());
        assert_eq!(tool.name(), "spawn_agent");
        assert_eq!(tool.parameters()["required"][0], json!("role"));

        let parallel = SpawnAgentsTool::new(delegation.clone());
        assert_eq!(parallel.name(), "spawn_agents");

        let plan = RunPlanTool::new(delegation);
        assert_eq!(plan.name(), "run_plan");
    }

    #[tokio::test]
    async fn the_plan_tool_returns_instructions() {
        let ctx = ToolContext::new(".");
        let output = PlanTool.run(json!({"goal": "ship it"}), &ctx).await.unwrap();
        assert!(output.content.contains("ship it"));
        assert!(output.content.contains("JSON"));
    }
}
