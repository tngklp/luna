//! Built-in tools: filesystem and shell.

use super::registry::{Tool, ToolOutcome};
use crate::error::{Error, Result};
use async_trait::async_trait;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use tokio_util::sync::CancellationToken;

// ---------- filesystem ----------

fn resolve_path(raw: &str) -> PathBuf {
    let p = Path::new(raw);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(p)
    }
}

pub struct ReadFile;

#[async_trait]
impl Tool for ReadFile {
    fn name(&self) -> &str {
        "read_file"
    }

    fn description(&self) -> &str {
        "Read the full text content of a file at `path`."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "File path (absolute or cwd-relative)" }
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, input: Value, cancel: CancellationToken) -> Result<ToolOutcome> {
        let raw = input["path"]
            .as_str()
            .ok_or_else(|| Error::Tool("read_file: missing `path`".into()))?;
        let path = resolve_path(raw);
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        let path_display = path.display().to_string();
        let bytes = tokio::task::spawn_blocking(move || std::fs::read(&path))
            .await
            .map_err(|e| Error::Tool(format!("read_file join error: {e}")))?
            .map_err(|e| Error::Tool(format!("read_file {path_display}: {e}")))?;
        let text = String::from_utf8_lossy(&bytes).to_string();
        Ok(ToolOutcome::ok(text))
    }
}

pub struct WriteFile;

#[async_trait]
impl Tool for WriteFile {
    fn name(&self) -> &str {
        "write_file"
    }

    fn description(&self) -> &str {
        "Write `content` to a file at `path`, creating parent directories. Overwrites existing files."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "File path" },
                "content": { "type": "string", "description": "Full file content" }
            },
            "required": ["path", "content"]
        })
    }

    fn requires_approval(&self) -> bool {
        true
    }

    async fn execute(&self, input: Value, _cancel: CancellationToken) -> Result<ToolOutcome> {
        let raw = input["path"]
            .as_str()
            .ok_or_else(|| Error::Tool("write_file: missing `path`".into()))?;
        let content = input["content"]
            .as_str()
            .ok_or_else(|| Error::Tool("write_file: missing `content`".into()))?;
        let path = resolve_path(raw);
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| Error::Tool(format!("write_file mkdir {}: {e}", parent.display())))?;
        }
        tokio::fs::write(&path, content)
            .await
            .map_err(|e| Error::Tool(format!("write_file {}: {e}", path.display())))?;
        Ok(ToolOutcome::ok(format!(
            "wrote {} bytes to {}",
            content.len(),
            path.display()
        )))
    }
}

pub struct ListDir;

#[async_trait]
impl Tool for ListDir {
    fn name(&self) -> &str {
        "list_dir"
    }

    fn description(&self) -> &str {
        "List entries of a directory at `path` (non-recursive)."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Directory path" }
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, input: Value, _cancel: CancellationToken) -> Result<ToolOutcome> {
        let raw = input["path"]
            .as_str()
            .ok_or_else(|| Error::Tool("list_dir: missing `path`".into()))?;
        let path = resolve_path(raw);
        let mut rd = tokio::fs::read_dir(&path)
            .await
            .map_err(|e| Error::Tool(format!("list_dir {}: {e}", path.display())))?;
        let mut lines = Vec::new();
        while let Some(entry) = rd
            .next_entry()
            .await
            .map_err(|e| Error::Tool(format!("list_dir {}: {e}", path.display())))?
        {
            let name = entry.file_name().to_string_lossy().to_string();
            if entry.file_type().await.map(|t| t.is_dir()).unwrap_or(false) {
                lines.push(format!("{name}/"));
            } else {
                lines.push(name);
            }
        }
        lines.sort();
        Ok(ToolOutcome::ok(lines.join("\n")))
    }
}

pub struct GlobFiles;

#[async_trait]
impl Tool for GlobFiles {
    fn name(&self) -> &str {
        "glob"
    }

    fn description(&self) -> &str {
        "Find files matching a glob `pattern` (e.g. `src/**/*.rs`), rooted at `path` (default cwd)."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": { "type": "string", "description": "Glob pattern" },
                "path": { "type": "string", "description": "Root directory (default: cwd)" }
            },
            "required": ["pattern"]
        })
    }

    async fn execute(&self, input: Value, _cancel: CancellationToken) -> Result<ToolOutcome> {
        let pattern = input["pattern"]
            .as_str()
            .ok_or_else(|| Error::Tool("glob: missing `pattern`".into()))?;
        let root = input["path"]
            .as_str()
            .map(resolve_path)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
        let full = root.join(pattern).to_string_lossy().to_string();
        let mut matches: Vec<String> = glob::glob(&full)
            .map_err(|e| Error::Tool(format!("glob pattern error: {e}")))?
            .filter_map(|entry| entry.ok())
            .map(|p| p.to_string_lossy().to_string())
            .collect();
        matches.sort();
        Ok(ToolOutcome::ok(matches.join("\n")))
    }
}

// ---------- shell ----------

pub struct Shell;

#[async_trait]
impl Tool for Shell {
    fn name(&self) -> &str {
        "shell"
    }

    fn description(&self) -> &str {
        "Run a shell command and return stdout+stderr. On Windows uses PowerShell; otherwise sh."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": { "type": "string", "description": "The command to run" },
                "working_dir": { "type": "string", "description": "Optional working directory" }
            },
            "required": ["command"]
        })
    }

    fn requires_approval(&self) -> bool {
        true
    }

    async fn execute(&self, input: Value, cancel: CancellationToken) -> Result<ToolOutcome> {
        let command = input["command"]
            .as_str()
            .ok_or_else(|| Error::Tool("shell: missing `command`".into()))?
            .to_string();
        let working_dir = input["working_dir"].as_str().map(PathBuf::from);

        #[cfg(windows)]
        let (program, args): (String, Vec<String>) = (
            "powershell".into(),
            vec!["-NoProfile".into(), "-Command".into(), command],
        );
        #[cfg(not(windows))]
        let (program, args): (String, Vec<String>) = ("sh".into(), vec!["-c".into(), command]);

        let mut cmd = tokio::process::Command::new(&program);
        cmd.args(&args).stdin(std::process::Stdio::null());
        if let Some(dir) = &working_dir {
            cmd.current_dir(dir);
        }
        cmd.stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        let child = cmd
            .spawn()
            .map_err(|e| Error::Tool(format!("shell spawn: {e}")))?;

        // Cancellation: kill the child when the token fires.
        let kill_token = cancel.clone();
        let child_id = child.id();
        let killer = tokio::spawn(async move {
            kill_token.cancelled().await;
            if let Some(pid) = child_id {
                unsafe {
                    // Best-effort kill on cancellation.
                    let _ = libc_like_kill(pid);
                }
            }
        });

        let out = child
            .wait_with_output()
            .await
            .map_err(|e| Error::Tool(format!("shell wait: {e}")))?;
        killer.abort();

        let mut text = String::from_utf8_lossy(&out.stdout).to_string();
        let stderr = String::from_utf8_lossy(&out.stderr);
        if !stderr.is_empty() {
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str("--- stderr ---\n");
            text.push_str(&stderr);
        }
        if cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        if !out.status.success() {
            text.push_str(&format!(
                "\n[exit code: {}]",
                out.status.code().unwrap_or(-1)
            ));
        }
        Ok(ToolOutcome {
            content: text,
            is_error: !out.status.success(),
        })
    }
}

// Best-effort cross-platform kill without pulling in the `libc` crate.
unsafe fn libc_like_kill(pid: u32) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        // taskkill /PID <pid> /T /F
        std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
            .output()
            .map(|_| ())
    }
    #[cfg(not(windows))]
    {
        std::process::Command::new("kill")
            .args(["-9", &pid.to_string()])
            .output()
            .map(|_| ())
    }
}

#[cfg(windows)]
trait CreationFlags {
    fn creation_flags(&mut self, flags: u32) -> &mut Self;
}

#[cfg(windows)]
impl CreationFlags for std::process::Command {
    fn creation_flags(&mut self, flags: u32) -> &mut Self {
        std::os::windows::process::CommandExt::creation_flags(self, flags)
    }
}

/// Register all built-ins into a registry.
pub fn register_all(registry: &mut super::registry::ToolRegistry) {
    registry.register(std::sync::Arc::new(ReadFile));
    registry.register(std::sync::Arc::new(WriteFile));
    registry.register(std::sync::Arc::new(ListDir));
    registry.register(std::sync::Arc::new(GlobFiles));
    registry.register(std::sync::Arc::new(Shell));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn read_write_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub").join("a.txt");
        let wf = WriteFile;
        let out = wf
            .execute(
                json!({"path": path.to_string_lossy(), "content": "hello luna"}),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        assert!(!out.is_error);

        let rf = ReadFile;
        let out = rf
            .execute(
                json!({"path": path.to_string_lossy()}),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(out.content, "hello luna");
    }

    #[tokio::test]
    async fn list_dir_lists() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("x.txt"), "x").unwrap();
        std::fs::create_dir(dir.path().join("subdir")).unwrap();
        let ld = ListDir;
        let out = ld
            .execute(
                json!({"path": dir.path().to_string_lossy()}),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(out.content, "subdir/\nx.txt");
    }

    #[tokio::test]
    async fn glob_finds_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.rs"), "").unwrap();
        std::fs::write(dir.path().join("b.rs"), "").unwrap();
        let g = GlobFiles;
        let out = g
            .execute(
                json!({"pattern": "*.rs", "path": dir.path().to_string_lossy()}),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        let lines: Vec<&str> = out.content.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].ends_with("a.rs"));
    }

    #[tokio::test]
    async fn shell_echoes() {
        let sh = Shell;
        let cmd = if cfg!(windows) {
            "Write-Output luna-test-123"
        } else {
            "echo luna-test-123"
        };
        let out = sh
            .execute(json!({"command": cmd}), CancellationToken::new())
            .await
            .unwrap();
        assert!(
            out.content.contains("luna-test-123"),
            "got: {}",
            out.content
        );
    }
}
