// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2025-2026 naskel.com

//! devit_docker — Docker convenience tool
//!
//! Actions: ps, logs, start, stop, restart, images, inspect.
//! Direct docker CLI wrapper with structured output.

use async_trait::async_trait;
use mcp_core::{McpError, McpResult, McpTool};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::process::Command;
use tracing::info;

const MAX_LOG_LINES: u32 = 200;

#[derive(Debug, Deserialize)]
struct DockerParams {
    /// Action: ps, logs, start, stop, restart, images, inspect
    action: String,
    /// Container name/ID (for logs, start, stop, restart, inspect)
    #[serde(default)]
    container: Option<String>,
    /// Number of log lines (default: 50, max: 200)
    #[serde(default = "default_tail")]
    tail: u32,
    /// Show all containers including stopped (for ps)
    #[serde(default)]
    all: bool,
}

fn default_tail() -> u32 {
    50
}

pub struct DockerTool;

impl DockerTool {
    pub fn new() -> Self {
        Self
    }

    async fn run_docker(&self, args: &[&str]) -> Result<(i32, String, String), McpError> {
        let output = Command::new("docker")
            .args(args)
            .output()
            .await
            .map_err(|e| {
                McpError::ExecutionFailed(format!("docker not found or not running: {e}"))
            })?;

        let exit_code = output.status.code().unwrap_or(-1);
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        Ok((exit_code, stdout, stderr))
    }

    async fn action_ps(&self, all: bool) -> McpResult<Value> {
        let mut args = vec![
            "ps",
            "--format",
            "table {{.ID}}\t{{.Names}}\t{{.Image}}\t{{.Status}}\t{{.Ports}}",
        ];
        if all {
            args.insert(1, "-a");
        }

        let (code, stdout, stderr) = self.run_docker(&args).await?;
        if code != 0 {
            return Err(McpError::ExecutionFailed(format!(
                "docker ps failed: {}",
                stderr.trim()
            )));
        }

        // Also get JSON for structured output
        let mut json_args = vec!["ps", "--format", "{{json .}}"];
        if all {
            json_args.insert(1, "-a");
        }
        let (_code, json_stdout, _) = self.run_docker(&json_args).await?;

        let containers: Vec<Value> = json_stdout
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect();

        Ok(json!({
            "content": [{"type": "text", "text": stdout.trim()}],
            "structuredContent": {
                "docker": {
                    "action": "ps",
                    "count": containers.len(),
                    "containers": containers
                }
            }
        }))
    }

    async fn action_logs(&self, container: &str, tail: u32) -> McpResult<Value> {
        let tail_str = tail.min(MAX_LOG_LINES).to_string();
        let (code, stdout, stderr) = self
            .run_docker(&["logs", "--tail", &tail_str, "--timestamps", container])
            .await?;

        if code != 0 {
            return Err(McpError::ExecutionFailed(format!(
                "docker logs failed: {}",
                stderr.trim()
            )));
        }

        // Docker logs can go to either stdout or stderr
        let output = if stdout.is_empty() { &stderr } else { &stdout };

        Ok(json!({
            "content": [{"type": "text", "text": format!("Logs for '{}' (last {}):\n\n{}", container, tail, output.trim())}],
            "structuredContent": {
                "docker": {
                    "action": "logs",
                    "container": container,
                    "tail": tail,
                    "lines": output.lines().count()
                }
            }
        }))
    }

    async fn action_lifecycle(&self, action: &str, container: &str) -> McpResult<Value> {
        let (code, stdout, stderr) = self.run_docker(&[action, container]).await?;
        let success = code == 0;
        let output = if success { &stdout } else { &stderr };

        Ok(json!({
            "content": [{"type": "text", "text": format!("docker {} {} — {}", action, container, if success {"OK"} else {output.trim()})}],
            "structuredContent": {
                "docker": {
                    "action": action,
                    "container": container,
                    "success": success
                }
            }
        }))
    }

    async fn action_images(&self) -> McpResult<Value> {
        let (code, stdout, stderr) = self
            .run_docker(&[
                "images",
                "--format",
                "table {{.Repository}}\t{{.Tag}}\t{{.Size}}\t{{.CreatedSince}}",
            ])
            .await?;

        if code != 0 {
            return Err(McpError::ExecutionFailed(format!(
                "docker images failed: {}",
                stderr.trim()
            )));
        }

        Ok(json!({
            "content": [{"type": "text", "text": stdout.trim()}],
            "structuredContent": {"docker": {"action": "images"}}
        }))
    }

    async fn action_inspect(&self, container: &str) -> McpResult<Value> {
        let (code, stdout, stderr) = self.run_docker(&["inspect", container]).await?;

        if code != 0 {
            return Err(McpError::ExecutionFailed(format!(
                "docker inspect failed: {}",
                stderr.trim()
            )));
        }

        // Parse JSON and extract useful fields
        let inspect: Value = serde_json::from_str(&stdout).unwrap_or(Value::Null);

        // Compact summary
        let summary = if let Some(arr) = inspect.as_array() {
            if let Some(obj) = arr.first() {
                let state = obj
                    .pointer("/State/Status")
                    .and_then(|v| v.as_str())
                    .unwrap_or("?");
                let image = obj
                    .pointer("/Config/Image")
                    .and_then(|v| v.as_str())
                    .unwrap_or("?");
                let created = obj.get("Created").and_then(|v| v.as_str()).unwrap_or("?");
                format!(
                    "Container: {} | Image: {} | State: {} | Created: {}",
                    container, image, state, created
                )
            } else {
                stdout.clone()
            }
        } else {
            stdout.clone()
        };

        Ok(json!({
            "content": [{"type": "text", "text": summary}],
            "structuredContent": {"docker": {"action": "inspect", "container": container, "data": inspect}}
        }))
    }
}

#[async_trait]
impl McpTool for DockerTool {
    fn name(&self) -> &str {
        "devit_docker"
    }

    fn description(&self) -> &str {
        "Docker convenience: ps (list containers), logs, start, stop, restart, \
         images, inspect. Direct docker CLI wrapper."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["ps", "logs", "start", "stop", "restart", "images", "inspect"],
                    "description": "Docker action"
                },
                "container": {
                    "type": "string",
                    "description": "Container name or ID (for logs, start, stop, restart, inspect)"
                },
                "tail": {
                    "type": "integer",
                    "description": "Number of log lines (default: 50, max: 200)",
                    "default": 50
                },
                "all": {
                    "type": "boolean",
                    "description": "Show all containers including stopped (for ps)",
                    "default": false
                }
            },
            "required": ["action"]
        })
    }

    async fn execute(&self, params: Value) -> McpResult<Value> {
        let params: DockerParams = serde_json::from_value(params)
            .map_err(|e| McpError::InvalidRequest(format!("Invalid params: {e}")))?;

        info!(
            target: "devit_mcp_tools",
            "devit_docker | action={} | container={:?}",
            params.action,
            params.container
        );

        let container_or_err = || -> McpResult<String> {
            params.container.clone().ok_or_else(|| {
                McpError::InvalidRequest("'container' is required for this action".into())
            })
        };

        match params.action.as_str() {
            "ps" => self.action_ps(params.all).await,
            "logs" => {
                let c = container_or_err()?;
                self.action_logs(&c, params.tail).await
            }
            "start" | "stop" | "restart" => {
                let c = container_or_err()?;
                self.action_lifecycle(&params.action, &c).await
            }
            "images" => self.action_images().await,
            "inspect" => {
                let c = container_or_err()?;
                self.action_inspect(&c).await
            }
            other => Err(McpError::InvalidRequest(format!(
                "Unknown action '{}'. Valid: ps, logs, start, stop, restart, images, inspect",
                other
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_params_ps() {
        let v = json!({"action": "ps", "all": true});
        let p: DockerParams = serde_json::from_value(v).unwrap();
        assert_eq!(p.action, "ps");
        assert!(p.all);
        assert_eq!(p.tail, 50);
    }

    #[test]
    fn test_params_logs() {
        let v = json!({"action": "logs", "container": "nginx", "tail": 100});
        let p: DockerParams = serde_json::from_value(v).unwrap();
        assert_eq!(p.container.unwrap(), "nginx");
        assert_eq!(p.tail, 100);
    }
}
