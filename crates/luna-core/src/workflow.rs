//! Saved, resumable workflows.
//!
//! A workflow is a named list of steps with dependencies — the same shape as a
//! plan, but persisted so it can be run again, resumed after a failure, or
//! shared with a team.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{LunaError, Result};
use crate::orchestrator::board::{Plan, PlannedTask};

/// One step of a workflow.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Step {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub role: Option<String>,
}

/// A saved workflow.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Workflow {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub steps: Vec<Step>,
    /// Where it was loaded from, when it came from disk.
    #[serde(skip)]
    pub path: PathBuf,
}

impl Workflow {
    /// Convert to the plan the orchestrator runs.
    pub fn to_plan(&self) -> Plan {
        Plan {
            tasks: self
                .steps
                .iter()
                .map(|step| PlannedTask {
                    id: step.id.clone(),
                    title: step.title.clone(),
                    description: step.description.clone(),
                    depends_on: step.depends_on.clone(),
                    role: step.role.clone(),
                })
                .collect(),
        }
    }

    /// Reject a workflow that cannot run.
    pub fn validate(&self) -> Result<()> {
        if self.steps.is_empty() {
            return Err(LunaError::other("the workflow has no steps"));
        }
        self.to_plan().validate()
    }
}

/// How a run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Running,
    Done,
    Failed,
    Cancelled,
}

/// The state of one execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Run {
    pub id: String,
    pub workflow: String,
    pub status: RunStatus,
    pub started: String,
    #[serde(default)]
    pub finished: Option<String>,
    /// Step id to how it ended.
    #[serde(default)]
    pub steps: std::collections::BTreeMap<String, String>,
}

impl Run {
    pub fn new(id: impl Into<String>, workflow: impl Into<String>) -> Run {
        Run {
            id: id.into(),
            workflow: workflow.into(),
            status: RunStatus::Running,
            started: chrono::Utc::now().to_rfc3339(),
            finished: None,
            steps: Default::default(),
        }
    }

    /// Steps that still need to run, in dependency order.
    pub fn remaining<'a>(&self, workflow: &'a Workflow) -> Vec<&'a Step> {
        workflow.steps.iter().filter(|step| !self.steps.contains_key(&step.id)).collect()
    }

    pub fn finish(&mut self, status: RunStatus) {
        self.status = status;
        self.finished = Some(chrono::Utc::now().to_rfc3339());
    }
}

/// Reads and writes workflows and runs under `~/.luna`.
pub struct WorkflowStore {
    dir: PathBuf,
}

impl WorkflowStore {
    pub fn new(root: &Path) -> WorkflowStore {
        WorkflowStore { dir: root.join("workflows") }
    }

    fn workflow_path(&self, name: &str) -> PathBuf {
        self.dir.join(format!("{name}.json"))
    }

    fn run_path(&self, id: &str) -> PathBuf {
        self.dir.join("runs").join(format!("{id}.json"))
    }

    pub fn save(&self, workflow: &Workflow) -> Result<()> {
        workflow.validate()?;
        let text = serde_json::to_string_pretty(workflow)?;
        crate::config::write_atomic(&self.workflow_path(&workflow.name), text.as_bytes())
    }

    pub fn load(&self, name: &str) -> Result<Workflow> {
        let path = self.workflow_path(name);
        let text = std::fs::read_to_string(&path)
            .map_err(|source| LunaError::Io { path: path.clone(), source })?;
        let mut workflow: Workflow = serde_json::from_str(&text)?;
        workflow.path = path;
        Ok(workflow)
    }

    pub fn list(&self) -> Vec<Workflow> {
        let Ok(entries) = std::fs::read_dir(&self.dir) else { return Vec::new() };
        let mut found: Vec<Workflow> = entries
            .flatten()
            .filter(|entry| entry.path().extension().and_then(|v| v.to_str()) == Some("json"))
            .filter_map(|entry| {
                let text = std::fs::read_to_string(entry.path()).ok()?;
                let mut workflow: Workflow = serde_json::from_str(&text).ok()?;
                workflow.path = entry.path();
                Some(workflow)
            })
            .collect();
        found.sort_by(|a, b| a.name.cmp(&b.name));
        found
    }

    pub fn delete(&self, name: &str) -> Result<()> {
        std::fs::remove_file(self.workflow_path(name)).ok();
        Ok(())
    }

    pub fn save_run(&self, run: &Run) -> Result<()> {
        let text = serde_json::to_string_pretty(run)?;
        crate::config::write_atomic(&self.run_path(&run.id), text.as_bytes())
    }

    pub fn load_run(&self, id: &str) -> Result<Run> {
        let path = self.run_path(id);
        let text = std::fs::read_to_string(&path)
            .map_err(|source| LunaError::Io { path: path.clone(), source })?;
        Ok(serde_json::from_str(&text)?)
    }

    pub fn list_runs(&self) -> Vec<Run> {
        let dir = self.dir.join("runs");
        let Ok(entries) = std::fs::read_dir(&dir) else { return Vec::new() };
        let mut found: Vec<Run> = entries
            .flatten()
            .filter_map(|entry| {
                let text = std::fs::read_to_string(entry.path()).ok()?;
                serde_json::from_str(&text).ok()
            })
            .collect();
        found.sort_by(|a, b| b.started.cmp(&a.started));
        found
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("luna-wf-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn sample() -> Workflow {
        Workflow {
            name: "ship".into(),
            description: "Ship a feature".into(),
            steps: vec![
                Step {
                    id: "a".into(),
                    title: "Plan".into(),
                    description: String::new(),
                    depends_on: vec![],
                    role: Some("planner".into()),
                },
                Step {
                    id: "b".into(),
                    title: "Build".into(),
                    description: String::new(),
                    depends_on: vec!["a".into()],
                    role: Some("worker".into()),
                },
            ],
            path: PathBuf::new(),
        }
    }

    #[test]
    fn a_workflow_round_trips() {
        let root = temp("roundtrip");
        let store = WorkflowStore::new(&root);
        store.save(&sample()).unwrap();

        let loaded = store.load("ship").unwrap();
        assert_eq!(loaded.steps.len(), 2);
        assert_eq!(loaded.steps[1].depends_on, vec!["a"]);
        assert_eq!(store.list().len(), 1);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn an_invalid_workflow_is_rejected() {
        let root = temp("invalid");
        let store = WorkflowStore::new(&root);
        let mut workflow = sample();
        workflow.steps[1].depends_on = vec!["ghost".into()];
        assert!(store.save(&workflow).is_err());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_workflow_becomes_a_plan() {
        let plan = sample().to_plan();
        assert_eq!(plan.tasks.len(), 2);
        assert_eq!(plan.tasks[1].depends_on, vec!["a"]);
    }

    #[test]
    fn a_run_tracks_which_steps_finished() {
        let workflow = sample();
        let mut run = Run::new("r1", "ship");
        assert_eq!(run.remaining(&workflow).len(), 2);

        run.steps.insert("a".into(), "done".into());
        let remaining = run.remaining(&workflow);
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].id, "b");
    }

    #[test]
    fn finishing_a_run_records_the_time() {
        let mut run = Run::new("r1", "ship");
        assert!(run.finished.is_none());
        run.finish(RunStatus::Done);
        assert_eq!(run.status, RunStatus::Done);
        assert!(run.finished.is_some());
    }

    #[test]
    fn runs_round_trip() {
        let root = temp("runs");
        let store = WorkflowStore::new(&root);
        let mut run = Run::new("r1", "ship");
        run.steps.insert("a".into(), "done".into());
        store.save_run(&run).unwrap();

        let loaded = store.load_run("r1").unwrap();
        assert_eq!(loaded.steps["a"], "done");
        assert_eq!(store.list_runs().len(), 1);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn deleting_a_workflow_removes_it() {
        let root = temp("delete");
        let store = WorkflowStore::new(&root);
        store.save(&sample()).unwrap();
        store.delete("ship").unwrap();
        assert!(store.list().is_empty());
        std::fs::remove_dir_all(&root).ok();
    }
}
