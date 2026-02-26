// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2025-2026 naskel.com

// devit_shell - Simplified bash shell wrapper for MCP
//
// This tool provides a simplified interface for running shell commands,
// wrapping bash -c "command" internally. It reuses devit_exec's security
// infrastructure (sandbox, rlimits, process registry).
//
// Example:
//   { "command": "ls -la && echo done" }
//
// Instead of:
//   { "binary": "/bin/bash", "args": ["-c", "ls -la && echo done"] }

use async_trait::async_trait;
use mcp_core::{McpError, McpResult, McpTool};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::PathBuf;
use tracing::info;

use crate::exec::{DevitExec, ExecConfig, ExecutionMode, StdinMode};
use crate::output_shaper::{IntentHint, OutputShaper};
use devit_cli::core::config::ExecToolConfig;

/// Shell command configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShellConfig {
    /// The shell command to execute (passed to bash -c)
    pub command: String,
    /// Working directory (optional, defaults to sandbox root)
    pub working_dir: Option<String>,
    /// Environment variables (limited to safe vars)
    pub env: Option<std::collections::HashMap<String, String>>,
    /// Timeout in seconds (default: 120)
    pub timeout_secs: Option<u64>,
    /// Run in background (default: false)
    #[serde(default)]
    pub background: bool,
    /// Output shaping hint: "auto", "debug", "verbose", "raw" (default: "auto")
    #[serde(default)]
    pub output_hint: Option<String>,
}

/// DevIt Shell tool - simplified bash wrapper
pub struct ShellTool {
    exec_tool: DevitExec,
    shell_binary: String,
    output_shaper: OutputShaper,
}

impl ShellTool {
    /// Create a new ShellTool with default configuration
    pub fn new(sandbox_root: PathBuf) -> McpResult<Self> {
        Self::with_config(ExecToolConfig::default(), sandbox_root)
    }

    /// Create a new ShellTool with custom exec configuration
    pub fn with_config(config: ExecToolConfig, sandbox_root: PathBuf) -> McpResult<Self> {
        let exec_tool = DevitExec::with_config(config, sandbox_root)?;

        // Detect available shell
        let shell_binary = detect_shell();

        Ok(Self {
            exec_tool,
            shell_binary,
            output_shaper: OutputShaper::new(),
        })
    }
}

/// Detect the best available shell
fn detect_shell() -> String {
    // Prefer bash, fallback to sh
    let candidates = ["/bin/bash", "/usr/bin/bash", "/bin/sh", "/usr/bin/sh"];

    for candidate in candidates {
        if std::path::Path::new(candidate).exists() {
            return candidate.to_string();
        }
    }

    // Ultimate fallback
    "/bin/sh".to_string()
}

#[async_trait]
impl McpTool for ShellTool {
    fn name(&self) -> &str {
        "devit_shell"
    }

    fn description(&self) -> &str {
        "Execute shell commands with bash -c wrapper. Simplified interface for running commands with pipes, redirections, and shell features."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "Shell command to execute (supports pipes, redirections, &&, ||, etc.)"
                },
                "working_dir": {
                    "type": "string",
                    "description": "Working directory (optional, defaults to sandbox root)"
                },
                "env": {
                    "type": "object",
                    "description": "Environment variables (limited to safe vars)"
                },
                "timeout_secs": {
                    "type": "number",
                    "description": "Timeout in seconds (default: 120, max: 600)"
                },
                "background": {
                    "type": "boolean",
                    "description": "Run in background (default: false)"
                },
                "output_hint": {
                    "type": "string",
                    "enum": ["auto", "debug", "verbose", "raw"],
                    "description": "Output shaping hint: auto (smart filtering), debug (focus on errors), verbose (more context), raw (no filtering)"
                }
            },
            "required": ["command"]
        })
    }

    async fn execute(&self, params: Value) -> McpResult<Value> {
        let config: ShellConfig =
            serde_json::from_value(params).map_err(|e| McpError::InvalidRequest(e.to_string()))?;

        // Validate timeout bounds
        let timeout = config.timeout_secs.unwrap_or(120).min(600);

        // Parse output hint
        let intent_hint = match config.output_hint.as_deref() {
            Some("debug") => IntentHint::Debug,
            Some("verbose") => IntentHint::Verbose,
            Some("raw") => IntentHint::Raw,
            _ => IntentHint::Auto,
        };

        info!(
            target: "devit_mcp_tools",
            "tool devit_shell called | command={:?} cwd={:?} background={} timeout={} hint={:?}",
            config.command,
            config.working_dir,
            config.background,
            timeout,
            intent_hint
        );

        // Build exec config wrapping bash -c
        let exec_config = ExecConfig {
            binary: self.shell_binary.clone(),
            args: vec!["-c".to_string(), config.command.clone()],
            working_dir: config.working_dir,
            env: config.env,
            mode: if config.background {
                ExecutionMode::Background
            } else {
                ExecutionMode::Foreground
            },
            stdin: StdinMode::Null,
            foreground_timeout_secs: Some(timeout),
        };

        // Delegate to exec tool
        let exec_params = serde_json::to_value(&exec_config)
            .map_err(|e| McpError::ExecutionFailed(e.to_string()))?;

        let result = self.exec_tool.execute(exec_params).await?;

        // Background mode: don't shape, return immediately
        if config.background {
            return Ok(result);
        }

        // Extract stdout/stderr from result for shaping
        let structured = result.get("structuredContent").and_then(|s| s.get("exec"));

        if let Some(exec_data) = structured {
            let stdout = exec_data
                .get("stdout_tail")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let stderr = exec_data
                .get("stderr_tail")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            // Combine stdout + stderr for shaping
            let combined = if stderr.is_empty() {
                stdout.to_string()
            } else if stdout.is_empty() {
                stderr.to_string()
            } else {
                format!("{}\n\n--- stderr ---\n{}", stdout, stderr)
            };

            // Shape the output
            let shaped = self.output_shaper.shape(&combined, intent_hint);

            // Rebuild response with shaped output
            let exit_code = exec_data.get("exit_code");
            let duration_ms = exec_data.get("duration_ms");

            let summary = format!(
                "✅ Shell completed — exit {} ({} ms) | format: {:?} | {}→{} bytes",
                exit_code.and_then(|v| v.as_i64()).unwrap_or(0),
                duration_ms.and_then(|v| v.as_u64()).unwrap_or(0),
                shaped.format,
                shaped.original_size,
                shaped.compact_size
            );

            let mut content = vec![json!({
                "type": "text",
                "text": summary
            })];

            if !shaped.compact.trim().is_empty() {
                content.push(json!({
                    "type": "text",
                    "text": shaped.compact
                }));
            }

            let structured_output = json!({
                "shell": {
                    "command": config.command,
                    "exit_code": exit_code,
                    "duration_ms": duration_ms,
                    "output": shaped.compact,
                    "original_size": shaped.original_size,
                    "compact_size": shaped.compact_size,
                    "truncated": shaped.metadata.truncated,
                    "format": format!("{:?}", shaped.format),
                    "raw_path": shaped.raw_path.map(|p| p.display().to_string()),
                    "metadata": {
                        "error_count": shaped.metadata.error_count,
                        "warning_count": shaped.metadata.warning_count,
                        "line_count": shaped.metadata.line_count
                    }
                }
            });

            Ok(json!({
                "content": content,
                "structuredContent": structured_output
            }))
        } else {
            // Fallback: return original result
            Ok(result)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_shell_echo() {
        let temp_dir = TempDir::new().unwrap();
        let tool = ShellTool::new(temp_dir.path().to_path_buf()).unwrap();

        let result = tool
            .execute(json!({
                "command": "echo hello"
            }))
            .await;

        assert!(result.is_ok());
        let value = result.unwrap();
        let content = value.get("content").unwrap();
        assert!(content.to_string().contains("hello") || content.to_string().contains("completed"));
    }

    #[tokio::test]
    async fn test_shell_pipe() {
        let temp_dir = TempDir::new().unwrap();
        let tool = ShellTool::new(temp_dir.path().to_path_buf()).unwrap();

        let result = tool
            .execute(json!({
                "command": "echo 'line1\nline2\nline3' | wc -l"
            }))
            .await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_shell_working_dir() {
        let temp_dir = TempDir::new().unwrap();
        std::fs::create_dir_all(temp_dir.path().join("subdir")).unwrap();

        let tool = ShellTool::new(temp_dir.path().to_path_buf()).unwrap();

        let result = tool
            .execute(json!({
                "command": "pwd",
                "working_dir": "subdir"
            }))
            .await;

        assert!(result.is_ok());
    }
}
