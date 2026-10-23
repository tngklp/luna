//! MCP (Model Context Protocol) client: stdio servers bridged into the tool registry.

use crate::error::{Error, Result};
use crate::provider::ToolSpec;
use crate::tool::registry::{Tool, ToolOutcome, ToolRegistry};
use rmcp::model::{CallToolRequestParams, ListToolsResult};
use rmcp::service::RunningService;
use rmcp::service::{RoleClient, ServiceExt};
use rmcp::transport::child_process::TokioChildProcess;
use serde_json::Value;
use std::sync::Arc;
use tokio::process::Command;
use tokio_util::sync::CancellationToken;

/// A running connection to one MCP server over stdio.
pub struct McpConnection {
    service: RunningService<RoleClient, ()>,
    server_name: String,
}

impl McpConnection {
    /// Spawn an MCP server as a child process and initialize the session.
    pub async fn spawn(server_name: &str, program: &str, args: &[String]) -> Result<Self> {
        let transport = TokioChildProcess::new({
            let mut cmd = Command::new(program);
            cmd.args(args);
            cmd
        })
        .map_err(|e| Error::Tool(format!("mcp `{server_name}` spawn: {e}")))?;

        let service = ()
            .serve(transport)
            .await
            .map_err(|e| Error::Tool(format!("mcp `{server_name}` init: {e}")))?;

        Ok(Self {
            service,
            server_name: server_name.to_string(),
        })
    }

    /// List tools the server exposes, as Luna ToolSpecs.
    pub async fn list_tools(&self) -> Result<Vec<ToolSpec>> {
        let ListToolsResult { tools, .. } = self
            .service
            .list_tools(Default::default())
            .await
            .map_err(|e| Error::Tool(format!("mcp `{}` list_tools: {e}", self.server_name)))?;

        Ok(tools
            .into_iter()
            .map(|t| ToolSpec {
                name: format!("mcp_{}_{}", self.server_name, t.name),
                description: t
                    .description
                    .as_ref()
                    .map(|d| d.to_string())
                    .unwrap_or_else(|| format!("MCP tool from `{}`", self.server_name)),
                parameters: serde_json::to_value(&*t.input_schema)
                    .unwrap_or_else(|_| Value::Object(Default::default())),
            })
            .collect())
    }

    async fn call(&self, tool_name: &str, input: Value) -> Result<ToolOutcome> {
        // Strip the mcp_<server>_ prefix for the wire call.
        let wire_name = tool_name
            .strip_prefix(&format!("mcp_{}_", self.server_name))
            .unwrap_or(tool_name);

        let arguments = match input {
            Value::Object(map) => Some(map.into_iter().collect()),
            Value::Null => None,
            other => {
                return Ok(ToolOutcome::err(format!(
                    "mcp: arguments must be an object, got {other}"
                )));
            }
        };

        let result = self
            .service
            .call_tool({
                let mut params = CallToolRequestParams::new(wire_name.to_owned());
                params.arguments = arguments;
                params
            })
            .await
            .map_err(|e| {
                Error::Tool(format!(
                    "mcp `{}` call `{wire_name}`: {e}",
                    self.server_name
                ))
            })?;

        // Concatenate text content; flag errors from the structured result.
        let mut text = String::new();
        let is_error = result.is_error.unwrap_or(false);
        for content in result.content {
            if let rmcp::model::ContentBlock::Text(t) = content {
                if !text.is_empty() {
                    text.push('\n');
                }
                text.push_str(&t.text);
            }
        }
        if is_error && text.is_empty() {
            text = "mcp tool reported an error".into();
        }
        Ok(ToolOutcome {
            content: text,
            is_error,
        })
    }
}

/// A Tool adapter over an MCP connection.
pub struct McpTool {
    connection: Arc<McpConnection>,
    spec: ToolSpec,
}

#[async_trait::async_trait]
impl Tool for McpTool {
    fn name(&self) -> &str {
        &self.spec.name
    }

    fn description(&self) -> &str {
        &self.spec.description
    }

    fn parameters(&self) -> Value {
        self.spec.parameters.clone()
    }

    fn requires_approval(&self) -> bool {
        // External tools run arbitrary code in another process: gate them.
        true
    }

    async fn execute(&self, input: Value, cancel: CancellationToken) -> Result<ToolOutcome> {
        tokio::select! {
            r = self.connection.call(&self.spec.name, input) => r,
            _ = cancel.cancelled() => Err(Error::Cancelled),
        }
    }
}

/// Connect to an MCP server and register all its tools into the registry.
pub async fn register_server(
    registry: &mut ToolRegistry,
    server_name: &str,
    program: &str,
    args: &[String],
) -> Result<usize> {
    let connection = Arc::new(McpConnection::spawn(server_name, program, args).await?);
    let specs = connection.list_tools().await?;
    let count = specs.len();
    for spec in specs {
        registry.register(Arc::new(McpTool {
            connection: connection.clone(),
            spec,
        }));
    }
    Ok(count)
}

/// Connect to every MCP server in the config; returns (connected, failed).
/// Failures are logged and skipped — one broken server must not prevent startup.
pub async fn register_from_config(
    registry: &mut ToolRegistry,
    servers: &std::collections::BTreeMap<String, crate::config::McpServerConfig>,
) -> (usize, usize) {
    let mut ok = 0;
    let mut failed = 0;
    for (name, cfg) in servers {
        match register_server(registry, name, &cfg.command, &cfg.args).await {
            Ok(n) => {
                tracing::info!(server = %name, tools = n, "mcp server connected");
                ok += 1;
            }
            Err(e) => {
                tracing::warn!(server = %name, error = %e, "mcp server failed to connect");
                failed += 1;
            }
        }
    }
    (ok, failed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn spawn_failure_is_a_tool_error() {
        // A program that certainly doesn't exist.
        let r = McpConnection::spawn("nope", "definitely-not-a-real-binary-xyz", &[]).await;
        assert!(r.is_err());
    }
}
