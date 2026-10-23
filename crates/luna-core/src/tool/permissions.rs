//! Permission gate: config-driven allow/deny rules plus interactive approvals.

use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// A single permission rule.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Rule {
    /// Allow a tool by exact name.
    AllowTool { tool: String },
    /// Deny a tool by exact name.
    DenyTool { tool: String },
    /// Allow shell commands matching this prefix (e.g. "cargo", "git status").
    AllowCommandPrefix { prefix: String },
    /// Deny shell commands matching this prefix (e.g. "rm", "del").
    DenyCommandPrefix { prefix: String },
    /// Allow filesystem access under this root.
    AllowPath { path: PathBuf },
    /// Deny filesystem access under this root.
    DenyPath { path: PathBuf },
}

/// Decision for a tool call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Allowed,
    Denied(String),
    /// Needs interactive approval; UI shows arguments and user decides.
    NeedsApproval,
}

/// The gate evaluates every tool call before execution.
#[derive(Debug, Clone, Default)]
pub struct PermissionGate {
    rules: Vec<Rule>,
    /// Tools that always require approval unless explicitly allowed.
    approval_tools: Vec<String>,
}

impl PermissionGate {
    pub fn new(rules: Vec<Rule>, approval_tools: Vec<String>) -> Self {
        Self {
            rules,
            approval_tools,
        }
    }

    /// Gate with sane defaults: destructive shell prefixes denied, everything
    /// else allowed except tools in the default approval list.
    pub fn default_rules() -> Vec<Rule> {
        vec![
            Rule::DenyCommandPrefix {
                prefix: "rm ".into(),
            },
            Rule::DenyCommandPrefix {
                prefix: "del ".into(),
            },
            Rule::DenyCommandPrefix {
                prefix: "rmdir ".into(),
            },
            Rule::DenyCommandPrefix {
                prefix: "format ".into(),
            },
            Rule::DenyCommandPrefix {
                prefix: "Remove-Item".into(),
            },
            Rule::DenyCommandPrefix {
                prefix: "shutdown".into(),
            },
        ]
    }

    /// Evaluate a tool call.
    pub fn evaluate(&self, tool: &str, input: &Value) -> Decision {
        // Exact deny wins first.
        for rule in &self.rules {
            if let Rule::DenyTool { tool: t } = rule
                && t == tool
            {
                return Decision::Denied(format!("tool `{tool}` is denied by config"));
            }
        }

        // Tool-specific contextual rules.
        match tool {
            "shell" => {
                if let Some(d) = self.eval_shell(input) {
                    return d;
                }
            }
            "read_file" | "write_file" | "list_dir" => {
                if let Some(d) = self.eval_path(tool, input) {
                    return d;
                }
            }
            _ => {}
        }

        // Exact allow.
        for rule in &self.rules {
            if let Rule::AllowTool { tool: t } = rule
                && t == tool
            {
                return Decision::Allowed;
            }
        }

        // Default: tools flagged for approval need it; others allowed.
        if self.approval_tools.iter().any(|t| t == tool) {
            Decision::NeedsApproval
        } else {
            Decision::Allowed
        }
    }

    /// Like [`evaluate`], but also honors the tool's own
    /// `requires_approval()` flag from the registry.
    pub fn evaluate_with_registry(
        &self,
        tool: &str,
        input: &Value,
        registry: &crate::tool::registry::ToolRegistry,
    ) -> Decision {
        // Registry-flagged tools always need approval unless an explicit
        // AllowTool rule whitelists them.
        if let Some(t) = registry.get(tool)
            && t.requires_approval()
        {
            let explicitly_allowed = self
                .rules
                .iter()
                .any(|r| matches!(r, Rule::AllowTool { tool: allowed } if allowed == tool));
            if !explicitly_allowed {
                return Decision::NeedsApproval;
            }
        }
        self.evaluate(tool, input)
    }

    fn eval_shell(&self, input: &Value) -> Option<Decision> {
        let cmd = input["command"].as_str()?;
        let first = cmd.split_whitespace().next().unwrap_or("");
        let mut rest = cmd.split_whitespace();
        let _ = rest.next();
        let two_word_prefix = {
            let words: Vec<&str> = cmd.split_whitespace().take(2).collect();
            if words.len() == 2 {
                format!("{} {}", words[0], words[1])
            } else {
                String::new()
            }
        };
        for rule in &self.rules {
            match rule {
                Rule::DenyCommandPrefix { prefix } => {
                    if cmd.starts_with(prefix.as_str())
                        || first == prefix
                        || (!two_word_prefix.is_empty() && two_word_prefix == *prefix)
                    {
                        return Some(Decision::Denied(format!(
                            "command `{cmd}` matches denied prefix `{prefix}`"
                        )));
                    }
                }
                Rule::AllowCommandPrefix { prefix }
                    if (cmd.starts_with(prefix.as_str()) || first == prefix) =>
                {
                    return Some(Decision::Allowed);
                }
                _ => {}
            }
        }
        None
    }

    fn eval_path(&self, _tool: &str, input: &Value) -> Option<Decision> {
        let raw = input["path"].as_str()?;
        let path = Path::new(raw);
        for rule in &self.rules {
            match rule {
                Rule::DenyPath { path: p } => {
                    if path.starts_with(p) {
                        return Some(Decision::Denied(format!(
                            "path `{raw}` is under denied root {}",
                            p.display()
                        )));
                    }
                }
                Rule::AllowPath { path: p } if path.starts_with(p) => {
                    return Some(Decision::Allowed);
                }
                _ => {}
            }
        }
        None
    }

    /// Enforce a decision, returning an error when denied.
    pub fn enforce(&self, tool: &str, input: &Value) -> Result<()> {
        match self.evaluate(tool, input) {
            Decision::Allowed => Ok(()),
            Decision::NeedsApproval => Err(Error::ToolDenied {
                tool: tool.to_string(),
                reason: "requires interactive approval".into(),
            }),
            Decision::Denied(reason) => Err(Error::ToolDenied {
                tool: tool.to_string(),
                reason,
            }),
        }
    }
}

/// Shared handle used by the agent loop to consult the gate.
pub type SharedGate = Arc<PermissionGate>;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn denies_destructive_shell() {
        let gate = PermissionGate::new(PermissionGate::default_rules(), vec![]);
        let d = gate.evaluate("shell", &json!({"command": "rm -rf /"}));
        assert_eq!(
            d,
            Decision::Denied("command `rm -rf /` matches denied prefix `rm `".into())
        );
    }

    #[test]
    fn allows_safe_shell_by_default() {
        let gate = PermissionGate::new(PermissionGate::default_rules(), vec![]);
        assert_eq!(
            gate.evaluate("shell", &json!({"command": "cargo test"})),
            Decision::Allowed
        );
    }

    #[test]
    fn approval_tools_need_approval() {
        let gate = PermissionGate::new(vec![], vec!["shell".into()]);
        assert_eq!(
            gate.evaluate("shell", &json!({"command": "ls"})),
            Decision::NeedsApproval
        );
    }

    #[test]
    fn deny_tool_beats_allow() {
        let gate = PermissionGate::new(
            vec![
                Rule::DenyTool {
                    tool: "shell".into(),
                },
                Rule::AllowTool {
                    tool: "shell".into(),
                },
            ],
            vec![],
        );
        assert!(matches!(
            gate.evaluate("shell", &json!({})),
            Decision::Denied(_)
        ));
    }

    #[test]
    fn registry_flagged_tools_need_approval() {
        // write_file sets requires_approval() = true in its impl.
        let mut reg = crate::tool::registry::ToolRegistry::new();
        crate::tool::builtin::register_all(&mut reg);
        let gate = PermissionGate::new(vec![], vec![]);
        assert_eq!(
            gate.evaluate_with_registry("write_file", &json!({"path": "a", "content": "b"}), &reg),
            Decision::NeedsApproval
        );
        // read_file does not.
        assert_eq!(
            gate.evaluate_with_registry("read_file", &json!({"path": "a"}), &reg),
            Decision::Allowed
        );
        // Explicit allow overrides the registry flag.
        let gate2 = PermissionGate::new(
            vec![Rule::AllowTool {
                tool: "write_file".into(),
            }],
            vec![],
        );
        assert_eq!(
            gate2.evaluate_with_registry("write_file", &json!({"path": "a", "content": "b"}), &reg),
            Decision::Allowed
        );
    }

    #[test]
    fn path_rules() {
        // Platform-agnostic roots: deny everything under temp_dir, allow
        // everything under the current dir's parent.
        let denied_root = std::env::temp_dir();
        let denied_file = denied_root.join("luna-test-denied").join("config");
        let allowed_file = std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("allowed.txt");

        let gate = PermissionGate::new(vec![Rule::DenyPath { path: denied_root }], vec![]);
        let d = gate.evaluate("read_file", &json!({"path": denied_file.to_string_lossy()}));
        assert!(
            matches!(d, Decision::Denied(_)),
            "expected denial, got {d:?}"
        );
        assert_eq!(
            gate.evaluate(
                "read_file",
                &json!({"path": allowed_file.to_string_lossy()})
            ),
            Decision::Allowed
        );
    }
}
