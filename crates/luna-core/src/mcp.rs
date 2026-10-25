//! Model Context Protocol client.
//!
//! MCP servers expose tools over JSON-RPC. Luna speaks the two transports that
//! matter in practice: stdio (a child process) and HTTP with server-sent events.
//!
//! Tools discovered from a server are wrapped in [`McpTool`], so the agent loop
//! cannot tell them apart from built-in ones.

use std::collections::BTreeMap;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::Mutex;

use crate::error::{LunaError, Result};
use crate::tool::{Tool, ToolClass, ToolContext, ToolOutput};

/// How to reach a server.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "transport", rename_all = "snake_case")]
pub enum Transport {
    /// A child process speaking JSON-RPC over stdin/stdout.
    #[serde(rename_all = "snake_case")]
    Stdio {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        env: BTreeMap<String, String>,
    },
    /// An HTTP endpoint. `sse` uses the streaming transport, otherwise plain POST.
    #[serde(rename_all = "snake_case")]
    Http {
        url: String,
        #[serde(default)]
        headers: BTreeMap<String, String>,
        #[serde(default)]
        sse: bool,
    },
}

/// One configured server.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ServerConfig {
    pub name: String,
    #[serde(flatten)]
    pub transport: Transport,
    /// Tools to hide from the agent.
    #[serde(default)]
    pub disabled_tools: Vec<String>,
    /// Whether the server's tools need approval before running.
    #[serde(default = "yes")]
    pub require_approval: bool,
}

fn yes() -> bool {
    true
}

/// A tool as the server describes it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolInfo {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "empty_schema")]
    pub input_schema: Value,
}

fn empty_schema() -> Value {
    json!({ "type": "object", "properties": {} })
}

/// A live connection to one server.
pub struct McpClient {
    name: String,
    transport: Transport,
    next_id: AtomicU64,
    /// Serialises requests: JSON-RPC over stdio is one conversation.
    io: Mutex<Option<StdioIo>>,
    http: reqwest::Client,
    session: Mutex<Option<String>>,
}

struct StdioIo {
    /// Held so the child is killed when the client drops.
    _child: tokio::process::Child,
    stdin: tokio::process::ChildStdin,
    stdout: BufReader<tokio::process::ChildStdout>,
}

impl McpClient {
    pub fn new(config: &ServerConfig) -> Arc<McpClient> {
        Arc::new(McpClient {
            name: config.name.clone(),
            transport: config.transport.clone(),
            next_id: AtomicU64::new(1),
            io: Mutex::new(None),
            http: reqwest::Client::new(),
            session: Mutex::new(None),
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    fn id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    /// Start the child process, if this is a stdio server.
    async fn ensure_stdio(&self) -> Result<()> {
        let Transport::Stdio { command, args, env } = &self.transport else {
            return Ok(());
        };
        let mut guard = self.io.lock().await;
        if guard.is_some() {
            return Ok(());
        }

        let mut child = tokio::process::Command::new(command)
            .args(args)
            .envs(env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|err| {
                LunaError::tool(format!("could not start MCP server `{}`: {err}", self.name))
            })?;

        let stdin =
            child.stdin.take().ok_or_else(|| LunaError::tool("the MCP server has no stdin"))?;
        let stdout =
            child.stdout.take().ok_or_else(|| LunaError::tool("the MCP server has no stdout"))?;
        *guard = Some(StdioIo { _child: child, stdin, stdout: BufReader::new(stdout) });
        Ok(())
    }

    /// Send one JSON-RPC request and wait for its response.
    async fn request(&self, method: &str, params: Value) -> Result<Value> {
        let id = self.id();
        let payload = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });

        match &self.transport {
            Transport::Stdio { .. } => {
                self.ensure_stdio().await?;
                let mut guard = self.io.lock().await;
                let io = guard.as_mut().ok_or_else(|| LunaError::tool("the MCP server is gone"))?;

                let mut line = serde_json::to_string(&payload)?;
                line.push('\n');
                io.stdin
                    .write_all(line.as_bytes())
                    .await
                    .map_err(|err| LunaError::tool(format!("could not write to MCP: {err}")))?;
                io.stdin
                    .flush()
                    .await
                    .map_err(|err| LunaError::tool(format!("could not flush MCP: {err}")))?;

                // Read until the response with our id arrives; notifications and
                // other ids are skipped.
                let mut buffer = String::new();
                loop {
                    buffer.clear();
                    let read = tokio::time::timeout(
                        std::time::Duration::from_secs(60),
                        io.stdout.read_line(&mut buffer),
                    )
                    .await
                    .map_err(|_| LunaError::tool("the MCP server did not answer within 60s"))?
                    .map_err(|err| LunaError::tool(format!("could not read MCP: {err}")))?;
                    if read == 0 {
                        return Err(LunaError::tool(format!(
                            "MCP server `{}` closed the connection",
                            self.name
                        )));
                    }
                    let Ok(value) = serde_json::from_str::<Value>(buffer.trim()) else {
                        continue;
                    };
                    if value.get("id").and_then(|value| value.as_u64()) != Some(id) {
                        continue;
                    }
                    if let Some(error) = value.get("error") {
                        let message = error
                            .get("message")
                            .and_then(|value| value.as_str())
                            .unwrap_or("the MCP server returned an error");
                        return Err(LunaError::tool(message.to_string()));
                    }
                    return Ok(value.get("result").cloned().unwrap_or(Value::Null));
                }
            }
            Transport::Http { url, headers, .. } => {
                let mut builder = self.http.post(url).json(&payload);
                for (name, value) in headers {
                    builder = builder.header(name, value);
                }
                if let Ok(session) = self.session.lock().await.as_ref().ok_or(()) {
                    builder = builder.header("Mcp-Session-Id", session);
                }
                let response = builder
                    .send()
                    .await
                    .map_err(|err| LunaError::tool(format!("MCP request failed: {err}")))?;

                if let Some(session) =
                    response.headers().get("mcp-session-id").and_then(|value| value.to_str().ok())
                {
                    *self.session.lock().await = Some(session.to_string());
                }

                let value: Value = response
                    .json()
                    .await
                    .map_err(|err| LunaError::tool(format!("MCP response was not JSON: {err}")))?;
                if let Some(error) = value.get("error") {
                    let message = error
                        .get("message")
                        .and_then(|value| value.as_str())
                        .unwrap_or("the MCP server returned an error");
                    return Err(LunaError::tool(message.to_string()));
                }
                Ok(value.get("result").cloned().unwrap_or(Value::Null))
            }
        }
    }

    /// Send a notification, which has no response.
    async fn notify(&self, method: &str, params: Value) -> Result<()> {
        let payload = json!({ "jsonrpc": "2.0", "method": method, "params": params });
        if let Transport::Stdio { .. } = &self.transport {
            self.ensure_stdio().await?;
            let mut guard = self.io.lock().await;
            if let Some(io) = guard.as_mut() {
                let mut line = serde_json::to_string(&payload)?;
                line.push('\n');
                let _ = io.stdin.write_all(line.as_bytes()).await;
                let _ = io.stdin.flush().await;
            }
        }
        Ok(())
    }

    /// Perform the handshake and list the server's tools.
    pub async fn initialize(&self) -> Result<Vec<ToolInfo>> {
        self.request(
            "initialize",
            json!({
                "protocolVersion": "2024-11-05",
                "capabilities": { "tools": {} },
                "clientInfo": { "name": "luna", "version": crate::VERSION }
            }),
        )
        .await?;
        self.notify("notifications/initialized", json!({})).await?;

        let result = self.request("tools/list", json!({})).await?;
        let tools = result
            .get("tools")
            .and_then(|value| value.as_array())
            .map(|entries| {
                entries
                    .iter()
                    .filter_map(|entry| {
                        Some(ToolInfo {
                            name: entry.get("name")?.as_str()?.to_string(),
                            description: entry
                                .get("description")
                                .and_then(|value| value.as_str())
                                .unwrap_or("")
                                .to_string(),
                            input_schema: entry
                                .get("inputSchema")
                                .cloned()
                                .unwrap_or_else(empty_schema),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(tools)
    }

    /// Call one of the server's tools.
    pub async fn call(&self, tool: &str, arguments: Value) -> Result<String> {
        let result =
            self.request("tools/call", json!({ "name": tool, "arguments": arguments })).await?;

        if result.get("isError").and_then(|value| value.as_bool()).unwrap_or(false) {
            return Err(LunaError::tool(render_content(&result)));
        }
        Ok(render_content(&result))
    }
}

/// Flatten an MCP content array into text.
fn render_content(result: &Value) -> String {
    let Some(content) = result.get("content").and_then(|value| value.as_array()) else {
        return result.to_string();
    };
    content
        .iter()
        .filter_map(|block| match block.get("type").and_then(|value| value.as_str()) {
            Some("text") => {
                block.get("text").and_then(|value| value.as_str()).map(|v| v.to_string())
            }
            Some("resource") => block
                .get("resource")
                .and_then(|resource| resource.get("text"))
                .and_then(|value| value.as_str())
                .map(|v| v.to_string()),
            Some(other) => Some(format!("[{other} content]")),
            None => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// A tool discovered from an MCP server.
pub struct McpTool {
    client: Arc<McpClient>,
    info: ToolInfo,
    /// The name the agent sees, namespaced so two servers cannot collide.
    qualified: String,
    require_approval: bool,
}

impl McpTool {
    pub fn new(client: Arc<McpClient>, info: ToolInfo, require_approval: bool) -> Self {
        let qualified = format!("{}__{}", client.name(), info.name);
        McpTool { client, info, qualified, require_approval }
    }
}

#[async_trait]
impl Tool for McpTool {
    fn name(&self) -> &str {
        &self.qualified
    }

    fn description(&self) -> &str {
        if self.info.description.is_empty() {
            "A tool provided by an MCP server."
        } else {
            &self.info.description
        }
    }

    fn parameters(&self) -> Value {
        self.info.input_schema.clone()
    }

    fn class(&self) -> ToolClass {
        // An MCP tool can do anything, so it is treated as a command unless the
        // server is configured to be trusted.
        if self.require_approval {
            ToolClass::Bash
        } else {
            ToolClass::Read
        }
    }

    async fn run(&self, args: Value, _ctx: &ToolContext) -> Result<ToolOutput> {
        match self.client.call(&self.info.name, args).await {
            Ok(text) => Ok(ToolOutput::ok(text)),
            Err(err) => Ok(ToolOutput::error(err.to_string())),
        }
    }
}

/// Connect to every configured server and collect its tools.
///
/// A server that fails to start is reported and skipped: one broken MCP server
/// must not stop Luna from running.
pub async fn connect_all(servers: &[ServerConfig]) -> (Vec<Arc<McpTool>>, Vec<String>) {
    let mut tools = Vec::new();
    let mut problems = Vec::new();

    for config in servers {
        let client = McpClient::new(config);
        match client.initialize().await {
            Ok(infos) => {
                for info in infos {
                    if config.disabled_tools.iter().any(|name| name == &info.name) {
                        continue;
                    }
                    tools.push(Arc::new(McpTool::new(
                        client.clone(),
                        info,
                        config.require_approval,
                    )));
                }
            }
            Err(err) => problems.push(format!("{}: {err}", config.name)),
        }
    }

    (tools, problems)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_is_flattened_to_text() {
        let result = json!({
            "content": [
                { "type": "text", "text": "one" },
                { "type": "text", "text": "two" }
            ]
        });
        assert_eq!(render_content(&result), "one\ntwo");
    }

    #[test]
    fn non_text_content_is_labelled() {
        let result = json!({ "content": [{ "type": "image" }] });
        assert_eq!(render_content(&result), "[image content]");
    }

    #[test]
    fn a_result_without_content_falls_back_to_json() {
        let result = json!({ "value": 1 });
        assert!(render_content(&result).contains("\"value\""));
    }

    #[test]
    fn tool_names_are_namespaced_by_server() {
        let config = ServerConfig {
            name: "fs".into(),
            transport: Transport::Stdio {
                command: "true".into(),
                args: vec![],
                env: BTreeMap::new(),
            },
            disabled_tools: vec![],
            require_approval: true,
        };
        let client = McpClient::new(&config);
        let tool = McpTool::new(
            client,
            ToolInfo {
                name: "read_file".into(),
                description: "reads".into(),
                input_schema: empty_schema(),
            },
            true,
        );
        assert_eq!(tool.name(), "fs__read_file");
        assert_eq!(tool.class(), ToolClass::Bash);
    }

    #[test]
    fn a_trusted_server_skips_approval() {
        let config = ServerConfig {
            name: "docs".into(),
            transport: Transport::Http {
                url: "http://localhost:1".into(),
                headers: BTreeMap::new(),
                sse: false,
            },
            disabled_tools: vec![],
            require_approval: false,
        };
        let client = McpClient::new(&config);
        let tool = McpTool::new(
            client,
            ToolInfo {
                name: "search".into(),
                description: String::new(),
                input_schema: empty_schema(),
            },
            false,
        );
        assert_eq!(tool.class(), ToolClass::Read);
        assert_eq!(tool.description(), "A tool provided by an MCP server.");
    }

    #[test]
    fn server_config_parses_from_toml() {
        let text = r#"
            name = "fs"
            transport = "stdio"
            command = "npx"
            args = ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"]
        "#;
        let config: ServerConfig = toml::from_str(text).unwrap();
        assert_eq!(config.name, "fs");
        assert!(config.require_approval);
        match config.transport {
            Transport::Stdio { command, args, .. } => {
                assert_eq!(command, "npx");
                assert_eq!(args.len(), 3);
            }
            _ => panic!("expected stdio"),
        }
    }

    #[test]
    fn http_config_parses() {
        let text = r#"
            name = "docs"
            transport = "http"
            url = "https://example.com/mcp"
            sse = true
        "#;
        let config: ServerConfig = toml::from_str(text).unwrap();
        match config.transport {
            Transport::Http { url, sse, .. } => {
                assert_eq!(url, "https://example.com/mcp");
                assert!(sse);
            }
            _ => panic!("expected http"),
        }
    }
}
