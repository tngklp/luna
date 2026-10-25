//! Shell execution.
//!
//! Output is streamed line by line so a long build shows progress instead of
//! appearing frozen, and the process is killed when the turn is cancelled.

use std::process::Stdio;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, BufReader};

use super::{optional_str, optional_usize, required_str, Tool, ToolClass, ToolContext, ToolOutput};
use crate::error::{LunaError, Result};

pub struct BashTool;

/// The shell to run commands with, per platform.
fn shell_command(command: &str) -> (String, Vec<String>) {
    if cfg!(windows) {
        // `cmd /C` keeps behaviour predictable for the common case; PowerShell
        // is available explicitly when a script needs it.
        ("cmd".to_string(), vec!["/C".to_string(), command.to_string()])
    } else {
        ("sh".to_string(), vec!["-c".to_string(), command.to_string()])
    }
}

#[async_trait]
impl Tool for BashTool {
    fn name(&self) -> &str {
        "bash"
    }

    fn description(&self) -> &str {
        "Run a shell command in the working directory and return its combined \
         output. Use it for builds, tests, git and anything else the other tools \
         cannot do. Long output is truncated."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": { "type": "string", "description": "The command to run" },
                "cwd": { "type": "string", "description": "Directory to run in, defaults to the working directory" },
                "timeout_secs": { "type": "integer", "description": "Kill the command after this many seconds" }
            },
            "required": ["command"]
        })
    }

    fn class(&self) -> ToolClass {
        ToolClass::Bash
    }

    async fn run(&self, args: Value, ctx: &ToolContext) -> Result<ToolOutput> {
        let command = required_str(&args, "command")?;
        let cwd = match optional_str(&args, "cwd") {
            Some(path) => ctx.resolve(&path)?,
            None => ctx.cwd.clone(),
        };
        let timeout = Duration::from_secs(
            optional_usize(&args, "timeout_secs")
                .map(|secs| secs as u64)
                .unwrap_or(ctx.bash_timeout_secs),
        );

        let (program, shell_args) = shell_command(&command);
        let mut child = tokio::process::Command::new(&program)
            .args(&shell_args)
            .current_dir(&cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|err| LunaError::tool(format!("could not start `{program}`: {err}")))?;

        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        // Both pipes feed one buffer so the transcript reads in real order.
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel::<String>();
        for pipe in [stdout.map(Pipe::Out), stderr.map(Pipe::Err)].into_iter().flatten() {
            let sender = sender.clone();
            tokio::spawn(async move {
                match pipe {
                    Pipe::Out(stream) => {
                        let mut lines = BufReader::new(stream).lines();
                        while let Ok(Some(line)) = lines.next_line().await {
                            let _ = sender.send(line);
                        }
                    }
                    Pipe::Err(stream) => {
                        let mut lines = BufReader::new(stream).lines();
                        while let Ok(Some(line)) = lines.next_line().await {
                            let _ = sender.send(line);
                        }
                    }
                }
            });
        }
        drop(sender);

        let mut collected = String::new();
        let mut truncated = false;
        let deadline = tokio::time::sleep(timeout);
        tokio::pin!(deadline);

        let status = loop {
            tokio::select! {
                biased;
                _ = ctx.cancel.cancelled() => {
                    let _ = child.kill().await;
                    return Err(LunaError::Cancelled);
                }
                _ = &mut deadline => {
                    let _ = child.kill().await;
                    return Ok(ToolOutput::error(format!(
                        "the command timed out after {}s",
                        timeout.as_secs()
                    )));
                }
                line = receiver.recv() => {
                    match line {
                        Some(line) => {
                            if collected.len() < super::MAX_OUTPUT_CHARS {
                                collected.push_str(&line);
                                collected.push('\n');
                                ctx.emit_progress(line);
                            } else {
                                truncated = true;
                            }
                        }
                        None => break child.wait().await,
                    }
                }
            }
        };

        let status = status.map_err(|err| LunaError::tool(format!("the command failed: {err}")))?;
        let code = status.code().unwrap_or(-1);

        let mut body = collected.trim_end().to_string();
        if body.is_empty() {
            body.push_str("(no output)");
        }
        if truncated {
            body.push_str("\n… output truncated");
        }
        if !status.success() {
            return Ok(ToolOutput::error(format!("exit code {code}\n{body}")));
        }
        Ok(ToolOutput::ok(format!("exit code {code}\n{body}")))
    }
}

enum Pipe {
    Out(tokio::process::ChildStdout),
    Err(tokio::process::ChildStderr),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn runs_a_command_and_reports_output() {
        let ctx = ToolContext::new(std::env::temp_dir());
        let output = BashTool.run(json!({ "command": "echo hello" }), &ctx).await.unwrap();
        assert!(!output.is_error);
        assert!(output.content.contains("hello"));
        assert!(output.content.contains("exit code 0"));
    }

    #[tokio::test]
    async fn a_failing_command_is_an_error_result() {
        let ctx = ToolContext::new(std::env::temp_dir());
        let output = BashTool.run(json!({ "command": "exit 3" }), &ctx).await.unwrap();
        assert!(output.is_error);
        assert!(output.content.contains("exit code 3"));
    }

    #[tokio::test]
    async fn timeouts_are_reported() {
        let ctx = ToolContext::new(std::env::temp_dir());
        let command = if cfg!(windows) { "ping -n 10 127.0.0.1 >nul" } else { "sleep 10" };
        let output =
            BashTool.run(json!({ "command": command, "timeout_secs": 1 }), &ctx).await.unwrap();
        assert!(output.is_error);
        assert!(output.content.contains("timed out"));
    }
}
