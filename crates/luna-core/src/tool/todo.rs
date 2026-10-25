//! The task board.
//!
//! A shared, dependency-aware list of tasks. The orchestrator uses it to decide
//! what to fan out next, and the model uses it to keep a long job honest.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::{required_str, Tool, ToolClass, ToolContext, ToolOutput};
use crate::error::{LunaError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Pending,
    InProgress,
    Done,
    Blocked,
}

impl TaskStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            TaskStatus::Pending => "pending",
            TaskStatus::InProgress => "in_progress",
            TaskStatus::Done => "done",
            TaskStatus::Blocked => "blocked",
        }
    }

    pub fn parse(value: &str) -> Option<TaskStatus> {
        match value {
            "pending" => Some(TaskStatus::Pending),
            "in_progress" => Some(TaskStatus::InProgress),
            "done" => Some(TaskStatus::Done),
            "blocked" => Some(TaskStatus::Blocked),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    pub status: TaskStatus,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub owner: Option<String>,
}

/// The board itself. Shared by the tools and the orchestrator.
#[derive(Clone, Default)]
pub struct TaskBoard {
    tasks: Arc<Mutex<Vec<Task>>>,
}

impl TaskBoard {
    pub fn new() -> Self {
        TaskBoard::default()
    }

    pub fn snapshot(&self) -> Vec<Task> {
        self.tasks.lock().map(|tasks| tasks.clone()).unwrap_or_default()
    }

    pub fn replace(&self, tasks: Vec<Task>) {
        if let Ok(mut guard) = self.tasks.lock() {
            *guard = tasks;
        }
    }

    pub fn set_status(&self, id: &str, status: TaskStatus) -> bool {
        let Ok(mut tasks) = self.tasks.lock() else { return false };
        match tasks.iter_mut().find(|task| task.id == id) {
            Some(task) => {
                task.status = status;
                true
            }
            None => false,
        }
    }

    pub fn claim(&self, id: &str, owner: &str) -> bool {
        let Ok(mut tasks) = self.tasks.lock() else { return false };
        match tasks.iter_mut().find(|task| task.id == id) {
            Some(task) if task.status == TaskStatus::Pending => {
                task.status = TaskStatus::InProgress;
                task.owner = Some(owner.to_string());
                true
            }
            _ => false,
        }
    }

    /// Tasks whose dependencies are all done and which nobody has claimed.
    pub fn ready(&self) -> Vec<Task> {
        let tasks = self.snapshot();
        let done: std::collections::HashSet<&str> = tasks
            .iter()
            .filter(|task| task.status == TaskStatus::Done)
            .map(|task| task.id.as_str())
            .collect();
        tasks
            .iter()
            .filter(|task| task.status == TaskStatus::Pending)
            .filter(|task| task.depends_on.iter().all(|dep| done.contains(dep.as_str())))
            .cloned()
            .collect()
    }

    /// Tasks that can never run because a dependency is blocked or missing.
    pub fn deadlocked(&self) -> Vec<Task> {
        let tasks = self.snapshot();
        let known: std::collections::HashSet<&str> =
            tasks.iter().map(|task| task.id.as_str()).collect();
        let blocked: std::collections::HashSet<&str> = tasks
            .iter()
            .filter(|task| task.status == TaskStatus::Blocked)
            .map(|task| task.id.as_str())
            .collect();
        tasks
            .iter()
            .filter(|task| task.status == TaskStatus::Pending)
            .filter(|task| {
                task.depends_on
                    .iter()
                    .any(|dep| blocked.contains(dep.as_str()) || !known.contains(dep.as_str()))
            })
            .cloned()
            .collect()
    }

    pub fn all_done(&self) -> bool {
        let tasks = self.snapshot();
        !tasks.is_empty() && tasks.iter().all(|task| task.status == TaskStatus::Done)
    }

    pub fn render(&self) -> String {
        let tasks = self.snapshot();
        if tasks.is_empty() {
            return "The task board is empty.".to_string();
        }
        let mut out = String::new();
        for task in tasks {
            let marker = match task.status {
                TaskStatus::Pending => "[ ]",
                TaskStatus::InProgress => "[~]",
                TaskStatus::Done => "[x]",
                TaskStatus::Blocked => "[!]",
            };
            out.push_str(&format!("{marker} {} — {}", task.id, task.title));
            if !task.depends_on.is_empty() {
                out.push_str(&format!(" (after {})", task.depends_on.join(", ")));
            }
            if let Some(owner) = &task.owner {
                out.push_str(&format!(" @{owner}"));
            }
            out.push('\n');
        }
        out
    }
}

pub struct TodoWriteTool {
    board: TaskBoard,
}

impl TodoWriteTool {
    pub fn new(board: TaskBoard) -> Self {
        TodoWriteTool { board }
    }
}

#[async_trait]
impl Tool for TodoWriteTool {
    fn name(&self) -> &str {
        "todo_write"
    }

    fn description(&self) -> &str {
        "Replace the task board. Send the complete list every time; it is not \
         merged. Use it to plan multi-step work and to keep the user informed."
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
                            "id": { "type": "string", "description": "Short kebab-case identifier" },
                            "title": { "type": "string" },
                            "description": { "type": "string" },
                            "status": { "type": "string", "enum": ["pending", "in_progress", "done", "blocked"] },
                            "depends_on": { "type": "array", "items": { "type": "string" } }
                        },
                        "required": ["id", "title", "status"]
                    }
                }
            },
            "required": ["tasks"]
        })
    }

    fn class(&self) -> ToolClass {
        ToolClass::Write
    }

    async fn run(&self, args: Value, _ctx: &ToolContext) -> Result<ToolOutput> {
        let entries = args
            .get("tasks")
            .and_then(|tasks| tasks.as_array())
            .ok_or_else(|| LunaError::tool("`tasks` must be an array"))?;
        let mut tasks = Vec::with_capacity(entries.len());
        for entry in entries {
            let id = required_str(entry, "id")?;
            let title = required_str(entry, "title")?;
            let status_text = required_str(entry, "status")?;
            let status = TaskStatus::parse(&status_text)
                .ok_or_else(|| LunaError::tool(format!("unknown status `{status_text}`")))?;
            let depends_on = entry
                .get("depends_on")
                .and_then(|deps| deps.as_array())
                .map(|deps| {
                    deps.iter().filter_map(|dep| dep.as_str()).map(|dep| dep.to_string()).collect()
                })
                .unwrap_or_default();
            tasks.push(Task {
                id,
                title,
                description: entry
                    .get("description")
                    .and_then(|value| value.as_str())
                    .unwrap_or_default()
                    .to_string(),
                status,
                depends_on,
                owner: None,
            });
        }
        self.board.replace(tasks);
        Ok(ToolOutput::ok(self.board.render()))
    }
}

pub struct TodoReadTool {
    board: TaskBoard,
}

impl TodoReadTool {
    pub fn new(board: TaskBoard) -> Self {
        TodoReadTool { board }
    }
}

#[async_trait]
impl Tool for TodoReadTool {
    fn name(&self) -> &str {
        "todo_read"
    }

    fn description(&self) -> &str {
        "Read the current task board."
    }

    fn parameters(&self) -> Value {
        json!({ "type": "object", "properties": {} })
    }

    fn class(&self) -> ToolClass {
        ToolClass::Read
    }

    async fn run(&self, _args: Value, _ctx: &ToolContext) -> Result<ToolOutput> {
        Ok(ToolOutput::ok(self.board.render()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(id: &str, status: TaskStatus, deps: &[&str]) -> Task {
        Task {
            id: id.to_string(),
            title: id.to_string(),
            description: String::new(),
            status,
            depends_on: deps.iter().map(|dep| dep.to_string()).collect(),
            owner: None,
        }
    }

    #[test]
    fn ready_tasks_respect_dependencies() {
        let board = TaskBoard::new();
        board.replace(vec![
            task("a", TaskStatus::Done, &[]),
            task("b", TaskStatus::Pending, &["a"]),
            task("c", TaskStatus::Pending, &["b"]),
        ]);
        let ready: Vec<String> = board.ready().into_iter().map(|task| task.id).collect();
        assert_eq!(ready, vec!["b"]);
    }

    #[test]
    fn blocked_dependencies_deadlock() {
        let board = TaskBoard::new();
        board.replace(vec![
            task("a", TaskStatus::Blocked, &[]),
            task("b", TaskStatus::Pending, &["a"]),
        ]);
        assert_eq!(board.deadlocked().len(), 1);
    }

    #[test]
    fn claiming_is_exclusive() {
        let board = TaskBoard::new();
        board.replace(vec![task("a", TaskStatus::Pending, &[])]);
        assert!(board.claim("a", "worker-1"));
        assert!(!board.claim("a", "worker-2"));
    }

    #[test]
    fn rendering_marks_each_status() {
        let board = TaskBoard::new();
        board.replace(vec![
            task("a", TaskStatus::Done, &[]),
            task("b", TaskStatus::InProgress, &[]),
            task("c", TaskStatus::Blocked, &[]),
        ]);
        let rendered = board.render();
        assert!(rendered.contains("[x] a"));
        assert!(rendered.contains("[~] b"));
        assert!(rendered.contains("[!] c"));
    }

    #[tokio::test]
    async fn the_write_tool_replaces_the_board() {
        let board = TaskBoard::new();
        let tool = TodoWriteTool::new(board.clone());
        let ctx = ToolContext::new(".");
        let output = tool
            .run(
                json!({"tasks": [
                    {"id": "one", "title": "First", "status": "pending"},
                    {"id": "two", "title": "Second", "status": "pending", "depends_on": ["one"]}
                ]}),
                &ctx,
            )
            .await
            .unwrap();
        assert!(!output.is_error);
        assert_eq!(board.snapshot().len(), 2);
        assert_eq!(board.ready().len(), 1);
    }
}
