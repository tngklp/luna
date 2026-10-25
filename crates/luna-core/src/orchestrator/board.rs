//! Planning helpers for the orchestrator.
//!
//! The board is the source of truth for what runs next. This module turns a
//! model's plan into board tasks and validates it, so a malformed plan fails
//! loudly instead of silently stalling the tree.

use serde::{Deserialize, Serialize};

use crate::error::{LunaError, Result};
use crate::tool::todo::{Task, TaskBoard, TaskStatus};

/// One planned unit of work.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannedTask {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub depends_on: Vec<String>,
    /// Role to run this task with. Defaults to `worker`.
    #[serde(default)]
    pub role: Option<String>,
}

/// A plan, as the planner agent returns it.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    #[serde(default)]
    pub tasks: Vec<PlannedTask>,
}

impl Plan {
    /// Parse a plan out of a model reply, tolerating prose around the JSON.
    pub fn parse(text: &str) -> Result<Plan> {
        let start = text.find('{').ok_or_else(|| LunaError::other("the plan contains no JSON"))?;
        let end = text.rfind('}').ok_or_else(|| LunaError::other("the plan is not valid JSON"))?;
        if end <= start {
            return Err(LunaError::other("the plan is not valid JSON"));
        }
        let plan: Plan = serde_json::from_str(&text[start..=end])
            .map_err(|err| LunaError::other(format!("could not parse the plan: {err}")))?;
        plan.validate()?;
        Ok(plan)
    }

    /// Reject plans that cannot be executed.
    pub fn validate(&self) -> Result<()> {
        if self.tasks.is_empty() {
            return Err(LunaError::other("the plan has no tasks"));
        }
        let mut seen = std::collections::HashSet::new();
        for task in &self.tasks {
            if task.id.trim().is_empty() {
                return Err(LunaError::other("every task needs an id"));
            }
            if !seen.insert(task.id.as_str()) {
                return Err(LunaError::other(format!("duplicate task id `{}`", task.id)));
            }
        }
        for task in &self.tasks {
            for dependency in &task.depends_on {
                if !seen.contains(dependency.as_str()) {
                    return Err(LunaError::other(format!(
                        "task `{}` depends on unknown task `{dependency}`",
                        task.id
                    )));
                }
                if dependency == &task.id {
                    return Err(LunaError::other(format!("task `{}` depends on itself", task.id)));
                }
            }
        }
        if self.has_cycle() {
            return Err(LunaError::other("the plan has a dependency cycle"));
        }
        Ok(())
    }

    fn has_cycle(&self) -> bool {
        #[derive(Clone, Copy, PartialEq)]
        enum Mark {
            Unvisited,
            Visiting,
            Done,
        }
        let mut marks: std::collections::HashMap<&str, Mark> =
            self.tasks.iter().map(|task| (task.id.as_str(), Mark::Unvisited)).collect();

        fn visit<'a>(
            id: &'a str,
            plan: &'a Plan,
            marks: &mut std::collections::HashMap<&'a str, Mark>,
        ) -> bool {
            match marks.get(id) {
                Some(Mark::Done) => return false,
                Some(Mark::Visiting) => return true,
                _ => {}
            }
            marks.insert(id, Mark::Visiting);
            if let Some(task) = plan.tasks.iter().find(|task| task.id == id) {
                for dependency in &task.depends_on {
                    if visit(dependency, plan, marks) {
                        return true;
                    }
                }
            }
            marks.insert(id, Mark::Done);
            false
        }

        let ids: Vec<&str> = self.tasks.iter().map(|task| task.id.as_str()).collect();
        ids.into_iter().any(|id| visit(id, self, &mut marks))
    }

    /// Load the plan onto a board.
    pub fn apply(&self, board: &TaskBoard) {
        board.replace(
            self.tasks
                .iter()
                .map(|task| Task {
                    id: task.id.clone(),
                    title: task.title.clone(),
                    description: task.description.clone(),
                    status: TaskStatus::Pending,
                    depends_on: task.depends_on.clone(),
                    owner: None,
                })
                .collect(),
        );
    }

    /// The role a task should run as.
    pub fn role_of(&self, id: &str) -> String {
        self.tasks
            .iter()
            .find(|task| task.id == id)
            .and_then(|task| task.role.clone())
            .unwrap_or_else(|| "worker".to_string())
    }
}

/// The instruction given to a planner agent.
pub fn planner_brief(goal: &str) -> String {
    format!(
        "Break this goal into the smallest set of independent tasks that can run in parallel.\n\n\
         Goal: {goal}\n\n\
         Reply with JSON only, in this shape:\n\
         {{\"tasks\":[{{\"id\":\"kebab-case\",\"title\":\"...\",\"description\":\"...\",\
         \"dependsOn\":[\"other-id\"],\"role\":\"worker\"}}]}}\n\n\
         Rules: ids are unique; dependencies must name real ids; no cycles; prefer \
         more parallel tasks over fewer serial ones; use role `researcher` for \
         investigation and `worker` for changes."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan(json: &str) -> Result<Plan> {
        Plan::parse(json)
    }

    #[test]
    fn parses_a_plan_wrapped_in_prose() {
        let text = r#"Here is the plan:
        {"tasks":[{"id":"a","title":"First"},{"id":"b","title":"Second","dependsOn":["a"]}]}
        Good luck."#;
        let parsed = plan(text).unwrap();
        assert_eq!(parsed.tasks.len(), 2);
    }

    #[test]
    fn rejects_unknown_dependencies() {
        let error =
            plan(r#"{"tasks":[{"id":"a","title":"A","dependsOn":["ghost"]}]}"#).unwrap_err();
        assert!(error.to_string().contains("unknown task"));
    }

    #[test]
    fn rejects_duplicate_ids() {
        let error =
            plan(r#"{"tasks":[{"id":"a","title":"A"},{"id":"a","title":"B"}]}"#).unwrap_err();
        assert!(error.to_string().contains("duplicate"));
    }

    #[test]
    fn rejects_cycles() {
        let error = plan(
            r#"{"tasks":[{"id":"a","title":"A","dependsOn":["b"]},{"id":"b","title":"B","dependsOn":["a"]}]}"#,
        )
        .unwrap_err();
        assert!(error.to_string().contains("cycle"));
    }

    #[test]
    fn rejects_self_dependency() {
        let error = plan(r#"{"tasks":[{"id":"a","title":"A","dependsOn":["a"]}]}"#).unwrap_err();
        assert!(error.to_string().contains("itself"));
    }

    #[test]
    fn rejects_empty_plans() {
        assert!(plan(r#"{"tasks":[]}"#).is_err());
    }

    #[test]
    fn applying_a_plan_fills_the_board() {
        let parsed =
            plan(r#"{"tasks":[{"id":"a","title":"A"},{"id":"b","title":"B","dependsOn":["a"]}]}"#)
                .unwrap();
        let board = TaskBoard::new();
        parsed.apply(&board);
        assert_eq!(board.snapshot().len(), 2);
        assert_eq!(board.ready().len(), 1);
    }

    #[test]
    fn roles_default_to_worker() {
        let parsed = plan(r#"{"tasks":[{"id":"a","title":"A"}]}"#).unwrap();
        assert_eq!(parsed.role_of("a"), "worker");
    }
}
