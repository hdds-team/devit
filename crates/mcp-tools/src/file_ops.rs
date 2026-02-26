// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2025-2026 naskel.com

//! devit_file_ops — File management operations for Claude Desktop
//!
//! Actions: rename, move, copy, delete, mkdir
//! Paths are relative to project root via FileSystemContext.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use mcp_core::{McpError, McpResult, McpTool};
use serde::Deserialize;
use serde_json::{json, Value};
use tracing::info;

use crate::file_read::FileSystemContext;

#[derive(Debug, Deserialize)]
struct FileOpsParams {
    action: String,
    path: Option<String>,
    from: Option<String>,
    to: Option<String>,
    #[serde(default)]
    recursive: bool,
}

pub struct FileOpsTool {
    context: Arc<FileSystemContext>,
}

impl FileOpsTool {
    pub fn new(context: Arc<FileSystemContext>) -> Self {
        Self { context }
    }

    fn resolve_read(&self, path: &str) -> McpResult<PathBuf> {
        self.context.resolve_read_path(path)
    }

    fn resolve_write(&self, path: &str) -> McpResult<PathBuf> {
        self.context.resolve_write_path(path)
    }

    fn require_from_to(&self, params: &FileOpsParams) -> McpResult<(PathBuf, PathBuf)> {
        let from = params
            .from
            .as_deref()
            .ok_or_else(|| McpError::InvalidRequest("'from' is required".into()))?;
        let to = params
            .to
            .as_deref()
            .ok_or_else(|| McpError::InvalidRequest("'to' is required".into()))?;
        let from_path = self.resolve_read(from)?;
        let to_path = self.resolve_write(to)?;
        Ok((from_path, to_path))
    }

    fn require_path_read(&self, params: &FileOpsParams) -> McpResult<PathBuf> {
        let path = params
            .path
            .as_deref()
            .ok_or_else(|| McpError::InvalidRequest("'path' is required".into()))?;
        self.resolve_read(path)
    }

    fn require_path_write(&self, params: &FileOpsParams) -> McpResult<PathBuf> {
        let path = params
            .path
            .as_deref()
            .ok_or_else(|| McpError::InvalidRequest("'path' is required".into()))?;
        self.resolve_write(path)
    }

    async fn action_rename(&self, params: &FileOpsParams) -> McpResult<Value> {
        let (from, to) = self.require_from_to(params)?;

        if !from.exists() {
            return Err(McpError::InvalidRequest(format!(
                "Source does not exist: {}",
                from.display()
            )));
        }

        // Ensure parent of target exists
        if let Some(parent) = to.parent() {
            if !parent.exists() {
                tokio::fs::create_dir_all(parent).await.map_err(|e| {
                    McpError::ExecutionFailed(format!("Cannot create parent dir: {e}"))
                })?;
            }
        }

        tokio::fs::rename(&from, &to)
            .await
            .map_err(|e| McpError::ExecutionFailed(format!("rename failed: {e}")))?;

        Ok(json!({
            "content": [{"type": "text", "text": format!("renamed {} → {}", from.display(), to.display())}],
            "structuredContent": {"file_ops": {"action": "rename", "success": true, "from": from.display().to_string(), "to": to.display().to_string()}}
        }))
    }

    async fn action_copy(&self, params: &FileOpsParams) -> McpResult<Value> {
        let (from, to) = self.require_from_to(params)?;

        if !from.exists() {
            return Err(McpError::InvalidRequest(format!(
                "Source does not exist: {}",
                from.display()
            )));
        }

        if let Some(parent) = to.parent() {
            if !parent.exists() {
                tokio::fs::create_dir_all(parent).await.map_err(|e| {
                    McpError::ExecutionFailed(format!("Cannot create parent dir: {e}"))
                })?;
            }
        }

        if from.is_dir() {
            copy_dir_recursive(&from, &to)
                .await
                .map_err(|e| McpError::ExecutionFailed(format!("copy directory failed: {e}")))?;
        } else {
            tokio::fs::copy(&from, &to)
                .await
                .map_err(|e| McpError::ExecutionFailed(format!("copy failed: {e}")))?;
        }

        Ok(json!({
            "content": [{"type": "text", "text": format!("copied {} → {}", from.display(), to.display())}],
            "structuredContent": {"file_ops": {"action": "copy", "success": true, "from": from.display().to_string(), "to": to.display().to_string()}}
        }))
    }

    async fn action_delete(&self, params: &FileOpsParams) -> McpResult<Value> {
        let path = self.require_path_read(params)?;

        if !path.exists() {
            return Err(McpError::InvalidRequest(format!(
                "Path does not exist: {}",
                path.display()
            )));
        }

        if path.is_dir() {
            if params.recursive {
                tokio::fs::remove_dir_all(&path)
                    .await
                    .map_err(|e| McpError::ExecutionFailed(format!("delete dir failed: {e}")))?;
            } else {
                tokio::fs::remove_dir(&path).await.map_err(|e| {
                    McpError::ExecutionFailed(format!(
                        "delete dir failed (not empty? use recursive: true): {e}"
                    ))
                })?;
            }
        } else {
            tokio::fs::remove_file(&path)
                .await
                .map_err(|e| McpError::ExecutionFailed(format!("delete file failed: {e}")))?;
        }

        Ok(json!({
            "content": [{"type": "text", "text": format!("deleted {}", path.display())}],
            "structuredContent": {"file_ops": {"action": "delete", "success": true, "path": path.display().to_string()}}
        }))
    }

    async fn action_mkdir(&self, params: &FileOpsParams) -> McpResult<Value> {
        let path = self.require_path_write(params)?;

        tokio::fs::create_dir_all(&path)
            .await
            .map_err(|e| McpError::ExecutionFailed(format!("mkdir failed: {e}")))?;

        Ok(json!({
            "content": [{"type": "text", "text": format!("created {}", path.display())}],
            "structuredContent": {"file_ops": {"action": "mkdir", "success": true, "path": path.display().to_string()}}
        }))
    }
}

/// Recursive directory copy
async fn copy_dir_recursive(src: &PathBuf, dst: &PathBuf) -> std::io::Result<()> {
    tokio::fs::create_dir_all(dst).await?;
    let mut entries = tokio::fs::read_dir(src).await?;
    while let Some(entry) = entries.next_entry().await? {
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if entry.file_type().await?.is_dir() {
            Box::pin(copy_dir_recursive(&src_path, &dst_path)).await?;
        } else {
            tokio::fs::copy(&src_path, &dst_path).await?;
        }
    }
    Ok(())
}

#[async_trait]
impl McpTool for FileOpsTool {
    fn name(&self) -> &str {
        "devit_file_ops"
    }

    fn description(&self) -> &str {
        "File management: rename, copy, delete, mkdir. \
         Paths are relative to project root."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["rename", "move", "copy", "delete", "mkdir"],
                    "description": "File operation (rename and move are equivalent)"
                },
                "path": {
                    "type": "string",
                    "description": "Target path (for delete, mkdir)"
                },
                "from": {
                    "type": "string",
                    "description": "Source path (for rename, move, copy)"
                },
                "to": {
                    "type": "string",
                    "description": "Destination path (for rename, move, copy)"
                },
                "recursive": {
                    "type": "boolean",
                    "description": "Recursive delete for directories (default: false)"
                }
            },
            "required": ["action"]
        })
    }

    async fn execute(&self, params: Value) -> McpResult<Value> {
        let params: FileOpsParams = serde_json::from_value(params)
            .map_err(|e| McpError::InvalidRequest(format!("Invalid params: {e}")))?;

        info!(
            target: "devit_mcp_tools",
            "devit_file_ops | action={} | cwd={}",
            params.action,
            self.context.root().display()
        );

        match params.action.as_str() {
            "rename" | "move" => self.action_rename(&params).await,
            "copy" => self.action_copy(&params).await,
            "delete" => self.action_delete(&params).await,
            "mkdir" => self.action_mkdir(&params).await,
            other => Err(McpError::InvalidRequest(format!(
                "Unknown action '{}'. Valid: rename, move, copy, delete, mkdir",
                other
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_params_rename() {
        let v = json!({"action": "rename", "from": "a.rs", "to": "b.rs"});
        let p: FileOpsParams = serde_json::from_value(v).unwrap();
        assert_eq!(p.action, "rename");
        assert_eq!(p.from.unwrap(), "a.rs");
        assert_eq!(p.to.unwrap(), "b.rs");
    }

    #[test]
    fn test_params_delete() {
        let v = json!({"action": "delete", "path": "tmp/junk", "recursive": true});
        let p: FileOpsParams = serde_json::from_value(v).unwrap();
        assert_eq!(p.action, "delete");
        assert!(p.recursive);
    }

    #[tokio::test]
    async fn test_mkdir_and_delete() {
        let tmp = tempfile::TempDir::new().unwrap();
        let ctx = Arc::new(
            FileSystemContext::with_allowed_paths(tmp.path().to_path_buf(), vec![]).unwrap(),
        );
        let tool = FileOpsTool::new(ctx);

        // mkdir
        let result = tool
            .execute(json!({"action": "mkdir", "path": "sub/deep"}))
            .await;
        assert!(result.is_ok());
        assert!(tmp.path().join("sub/deep").is_dir());

        // delete recursive
        let result = tool
            .execute(json!({"action": "delete", "path": "sub", "recursive": true}))
            .await;
        assert!(result.is_ok());
        assert!(!tmp.path().join("sub").exists());
    }

    #[tokio::test]
    async fn test_copy_file() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(tmp.path().join("src.txt"), "hello").unwrap();

        let ctx = Arc::new(
            FileSystemContext::with_allowed_paths(tmp.path().to_path_buf(), vec![]).unwrap(),
        );
        let tool = FileOpsTool::new(ctx);

        let result = tool
            .execute(json!({"action": "copy", "from": "src.txt", "to": "dst.txt"}))
            .await;
        assert!(result.is_ok());
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("dst.txt")).unwrap(),
            "hello"
        );
    }

    #[tokio::test]
    async fn test_rejects_traversal_path() {
        let tmp = tempfile::TempDir::new().unwrap();
        let ctx = Arc::new(
            FileSystemContext::with_allowed_paths(tmp.path().to_path_buf(), vec![]).unwrap(),
        );
        let tool = FileOpsTool::new(ctx);

        let result = tool
            .execute(json!({"action": "mkdir", "path": "../escape"}))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_rejects_absolute_path_outside_root() {
        let tmp = tempfile::TempDir::new().unwrap();
        let ctx = Arc::new(
            FileSystemContext::with_allowed_paths(tmp.path().to_path_buf(), vec![]).unwrap(),
        );
        let tool = FileOpsTool::new(ctx);

        let target = std::path::PathBuf::from("/etc/devit-should-not-write-here");
        let result = tool
            .execute(json!({"action": "mkdir", "path": target.to_string_lossy()}))
            .await;
        assert!(result.is_err());
    }
}
