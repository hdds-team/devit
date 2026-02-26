// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2025-2026 naskel.com

//! devit_git — Git write operations for Claude Desktop convenience
//!
//! Actions: commit, push, branch, stash, checkout, tag
//! Direct git execution, no sandbox overhead.

use std::sync::Arc;

use async_trait::async_trait;
use mcp_core::{McpError, McpResult, McpTool};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::process::Command;
use tracing::info;

use crate::file_read::FileSystemContext;

#[derive(Debug, Deserialize)]
struct GitParams {
    action: String,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    files: Option<Vec<String>>,
    #[serde(default)]
    branch: Option<String>,
    #[serde(default)]
    remote: Option<String>,
    #[serde(default)]
    force: bool,
}

pub struct GitOpsTool {
    context: Arc<FileSystemContext>,
}

impl GitOpsTool {
    pub fn new(context: Arc<FileSystemContext>) -> Self {
        Self { context }
    }

    fn cwd(&self) -> &std::path::Path {
        self.context.root()
    }

    async fn run_git(&self, args: &[&str]) -> Result<(i32, String, String), McpError> {
        let output = Command::new("git")
            .args(args)
            .current_dir(self.cwd())
            .output()
            .await
            .map_err(|e| McpError::ExecutionFailed(format!("Failed to spawn git: {e}")))?;

        let exit_code = output.status.code().unwrap_or(-1);
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        Ok((exit_code, stdout, stderr))
    }

    async fn action_commit(&self, params: &GitParams) -> McpResult<Value> {
        let message = params
            .message
            .as_deref()
            .ok_or_else(|| McpError::InvalidRequest("commit requires 'message'".into()))?;

        // Stage files
        match &params.files {
            Some(files) if !files.is_empty() => {
                let file_refs: Vec<&str> = files.iter().map(|s| s.as_str()).collect();
                let mut args = vec!["add"];
                args.extend(&file_refs);
                let (code, _, stderr) = self.run_git(&args).await?;
                if code != 0 {
                    return Ok(json!({
                        "content": [{"type": "text", "text": format!("git add failed: {}", stderr.trim())}],
                        "structuredContent": {"git": {"action": "commit", "success": false, "error": stderr.trim()}}
                    }));
                }
            }
            _ => {
                // No files specified — stage all modified
                let (code, _, stderr) = self.run_git(&["add", "-u"]).await?;
                if code != 0 {
                    return Ok(json!({
                        "content": [{"type": "text", "text": format!("git add -u failed: {}", stderr.trim())}],
                        "structuredContent": {"git": {"action": "commit", "success": false, "error": stderr.trim()}}
                    }));
                }
            }
        }

        // Commit
        let (code, stdout, stderr) = self.run_git(&["commit", "-m", message]).await?;
        let success = code == 0;
        let output = if success { &stdout } else { &stderr };

        Ok(json!({
            "content": [{"type": "text", "text": format!("git commit — {} | {}", if success {"OK"} else {"FAILED"}, output.trim())}],
            "structuredContent": {"git": {"action": "commit", "success": success, "exit_code": code, "message": message, "output": output.trim()}}
        }))
    }

    async fn action_push(&self, params: &GitParams) -> McpResult<Value> {
        let remote = params.remote.as_deref().unwrap_or("origin");

        let mut args = vec!["push", remote];
        if let Some(ref branch) = params.branch {
            args.push(branch);
        }
        if params.force {
            args.push("--force-with-lease");
        }

        let (code, stdout, stderr) = self.run_git(&args).await?;
        let success = code == 0;
        let output = format!("{}{}", stdout, stderr);

        Ok(json!({
            "content": [{"type": "text", "text": format!("git push — {} | {}", if success {"OK"} else {"FAILED"}, output.trim())}],
            "structuredContent": {"git": {"action": "push", "success": success, "exit_code": code, "remote": remote, "output": output.trim()}}
        }))
    }

    async fn action_branch(&self, params: &GitParams) -> McpResult<Value> {
        let branch = params
            .branch
            .as_deref()
            .ok_or_else(|| McpError::InvalidRequest("branch requires 'branch' name".into()))?;

        // Create and switch
        let (code, _stdout, _stderr) = self.run_git(&["checkout", "-b", branch]).await?;
        if code == 0 {
            return Ok(json!({
                "content": [{"type": "text", "text": format!("git branch — created & switched to '{}'", branch)}],
                "structuredContent": {"git": {"action": "branch", "success": true, "branch": branch}}
            }));
        }

        // Branch exists? Just switch
        let (code2, stdout2, stderr2) = self.run_git(&["checkout", branch]).await?;
        let success = code2 == 0;
        let output = if success { &stdout2 } else { &stderr2 };

        Ok(json!({
            "content": [{"type": "text", "text": format!("git branch — {} | {}", if success {"switched"} else {"FAILED"}, output.trim())}],
            "structuredContent": {"git": {"action": "branch", "success": success, "branch": branch, "output": output.trim()}}
        }))
    }

    async fn action_stash(&self, params: &GitParams) -> McpResult<Value> {
        let sub = params.message.as_deref().unwrap_or("push");
        let args = match sub {
            "pop" => vec!["stash", "pop"],
            "list" => vec!["stash", "list"],
            "drop" => vec!["stash", "drop"],
            _ => vec!["stash", "push"],
        };

        let arg_refs: Vec<&str> = args.iter().map(|s| &**s).collect();
        let (code, stdout, stderr) = self.run_git(&arg_refs).await?;
        let success = code == 0;
        let output = if stdout.is_empty() { &stderr } else { &stdout };

        Ok(json!({
            "content": [{"type": "text", "text": format!("git stash {} — {} | {}", sub, if success {"OK"} else {"FAILED"}, output.trim())}],
            "structuredContent": {"git": {"action": "stash", "sub": sub, "success": success, "output": output.trim()}}
        }))
    }

    async fn action_checkout(&self, params: &GitParams) -> McpResult<Value> {
        let target = params
            .branch
            .as_deref()
            .or(params
                .files
                .as_ref()
                .and_then(|f| f.first().map(|s| s.as_str())))
            .ok_or_else(|| {
                McpError::InvalidRequest("checkout requires 'branch' or 'files'".into())
            })?;

        let (code, stdout, stderr) = self.run_git(&["checkout", target]).await?;
        let success = code == 0;
        let output = format!("{}{}", stdout, stderr);

        Ok(json!({
            "content": [{"type": "text", "text": format!("git checkout {} — {} | {}", target, if success {"OK"} else {"FAILED"}, output.trim())}],
            "structuredContent": {"git": {"action": "checkout", "success": success, "target": target, "output": output.trim()}}
        }))
    }

    async fn action_status(&self) -> McpResult<Value> {
        let (_, stdout, _) = self.run_git(&["status", "--short", "--branch"]).await?;
        Ok(json!({
            "content": [{"type": "text", "text": format!("git status:\n{}", stdout.trim())}],
            "structuredContent": {"git": {"action": "status", "success": true, "output": stdout.trim()}}
        }))
    }
}

#[async_trait]
impl McpTool for GitOpsTool {
    fn name(&self) -> &str {
        "devit_git"
    }

    fn description(&self) -> &str {
        "Git write operations: commit, push, branch, stash, checkout, status. \
         Direct execution, no sandbox overhead."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["commit", "push", "branch", "stash", "checkout", "status"],
                    "description": "Git action to perform"
                },
                "message": {
                    "type": "string",
                    "description": "Commit message (for commit), or stash sub-action: push|pop|list|drop (for stash)"
                },
                "files": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Files to stage (for commit). Omit to stage all modified."
                },
                "branch": {
                    "type": "string",
                    "description": "Branch name (for branch/checkout/push)"
                },
                "remote": {
                    "type": "string",
                    "description": "Remote name (default: origin)"
                },
                "force": {
                    "type": "boolean",
                    "description": "Force push with --force-with-lease (for push)"
                }
            },
            "required": ["action"]
        })
    }

    async fn execute(&self, params: Value) -> McpResult<Value> {
        let params: GitParams = serde_json::from_value(params)
            .map_err(|e| McpError::InvalidRequest(format!("Invalid params: {e}")))?;

        info!(
            target: "devit_mcp_tools",
            "devit_git | action={} | cwd={}",
            params.action,
            self.cwd().display()
        );

        match params.action.as_str() {
            "commit" => self.action_commit(&params).await,
            "push" => self.action_push(&params).await,
            "branch" => self.action_branch(&params).await,
            "stash" => self.action_stash(&params).await,
            "checkout" => self.action_checkout(&params).await,
            "status" => self.action_status().await,
            other => Err(McpError::InvalidRequest(format!(
                "Unknown action '{}'. Valid: commit, push, branch, stash, checkout, status",
                other
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_params_deserialize() {
        let v = json!({"action": "commit", "message": "test", "files": ["a.rs", "b.rs"]});
        let p: GitParams = serde_json::from_value(v).unwrap();
        assert_eq!(p.action, "commit");
        assert_eq!(p.files.unwrap().len(), 2);
        assert!(!p.force);
    }

    #[test]
    fn test_params_minimal() {
        let v = json!({"action": "status"});
        let p: GitParams = serde_json::from_value(v).unwrap();
        assert_eq!(p.action, "status");
        assert!(p.message.is_none());
        assert!(p.files.is_none());
    }
}
