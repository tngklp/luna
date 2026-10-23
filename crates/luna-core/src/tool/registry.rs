//! Tool trait and registry.

use crate::error::{Error, Result};
use crate::provider::ToolSpec;
use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

/// Result of running a tool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolOutcome {
    pub content: String,
    pub is_error: bool,
}

impl ToolOutcome {
    pub fn ok(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            is_error: false,
        }
    }

    pub fn err(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            is_error: true,
        }
    }
}

/// A callable tool available to agents.
#[async_trait]
pub trait Tool: Send + Sync {
    /// Stable name the model references.
    fn name(&self) -> &str;

    /// Description shown to the model.
    fn description(&self) -> &str;

    /// JSON Schema for the input object.
    fn parameters(&self) -> Value;

    /// Whether calls to this tool require interactive approval by default.
    fn requires_approval(&self) -> bool {
        false
    }

    /// Execute the tool. `cancel` should be honored for long operations.
    async fn execute(
        &self,
        input: Value,
        cancel: tokio_util::sync::CancellationToken,
    ) -> Result<ToolOutcome>;
}

/// Registry of available tools, keyed by name.
#[derive(Default, Clone)]
pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, tool: Arc<dyn Tool>) {
        self.tools.insert(tool.name().to_string(), tool);
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.get(name).cloned()
    }

    pub fn contains(&self, name: &str) -> bool {
        self.tools.contains_key(name)
    }

    pub fn names(&self) -> Vec<String> {
        let mut v: Vec<String> = self.tools.keys().cloned().collect();
        v.sort();
        v
    }

    /// Provider-facing specs for all registered tools.
    pub fn specs(&self) -> Vec<ToolSpec> {
        let mut specs: Vec<ToolSpec> = self
            .tools
            .values()
            .map(|t| ToolSpec {
                name: t.name().to_string(),
                description: t.description().to_string(),
                parameters: t.parameters(),
            })
            .collect();
        specs.sort_by(|a, b| a.name.cmp(&b.name));
        specs
    }

    /// Validate that `input` is an object (deeper schema validation later).
    pub fn validate_input(&self, name: &str, input: &Value) -> Result<()> {
        if !input.is_object() {
            return Err(Error::Tool(format!(
                "tool `{name}` input must be a JSON object"
            )));
        }
        if !self.contains(name) {
            return Err(Error::Tool(format!("unknown tool `{name}`")));
        }
        Ok(())
    }
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
            "echoes input"
        }
        fn parameters(&self) -> Value {
            json!({"type": "object", "properties": {"text": {"type": "string"}}})
        }
        async fn execute(
            &self,
            input: Value,
            _cancel: tokio_util::sync::CancellationToken,
        ) -> Result<ToolOutcome> {
            Ok(ToolOutcome::ok(input["text"].as_str().unwrap_or("")))
        }
    }

    #[test]
    fn register_and_lookup() {
        let mut reg = ToolRegistry::new();
        reg.register(Arc::new(Echo));
        assert!(reg.contains("echo"));
        assert_eq!(reg.names(), vec!["echo"]);
        assert_eq!(reg.specs().len(), 1);
    }

    #[tokio::test]
    async fn executes_tool() {
        let mut reg = ToolRegistry::new();
        reg.register(Arc::new(Echo));
        let t = reg.get("echo").unwrap();
        let out = t
            .execute(
                json!({"text": "hi"}),
                tokio_util::sync::CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(out.content, "hi");
        assert!(!out.is_error);
    }
}
