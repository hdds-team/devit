// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2025-2026 naskel.com

//! devit_archive — Archive management tool
//!
//! Actions: extract, create, list.
//! Supports: tar.gz, tar.bz2, tar.xz, zip.

use std::sync::Arc;

use async_trait::async_trait;
use mcp_core::{McpError, McpResult, McpTool};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::process::Command;
use tracing::info;

use crate::file_read::FileSystemContext;

#[derive(Debug, Deserialize)]
struct ArchiveParams {
    /// Action: extract, create, list
    action: String,
    /// Archive file path
    path: String,
    /// Target directory for extraction (default: current dir)
    #[serde(default)]
    target: Option<String>,
    /// Files/dirs to add (for create action)
    #[serde(default)]
    files: Option<Vec<String>>,
    /// Archive format override: tar.gz, tar.bz2, tar.xz, zip (auto-detected from extension)
    #[serde(default)]
    format: Option<String>,
}

pub struct ArchiveTool {
    context: Arc<FileSystemContext>,
}

impl ArchiveTool {
    pub fn new(context: Arc<FileSystemContext>) -> Self {
        Self { context }
    }

    fn resolve(&self, path: &str) -> std::path::PathBuf {
        self.context.root().join(path)
    }

    fn detect_format(path: &str) -> &str {
        if path.ends_with(".tar.gz") || path.ends_with(".tgz") {
            "tar.gz"
        } else if path.ends_with(".tar.bz2") || path.ends_with(".tbz2") {
            "tar.bz2"
        } else if path.ends_with(".tar.xz") || path.ends_with(".txz") {
            "tar.xz"
        } else if path.ends_with(".tar") {
            "tar"
        } else if path.ends_with(".zip") {
            "zip"
        } else {
            "tar.gz" // default
        }
    }

    async fn action_extract(&self, params: &ArchiveParams) -> McpResult<Value> {
        let archive_path = self.resolve(&params.path);
        if !archive_path.exists() {
            return Err(McpError::InvalidRequest(format!(
                "Archive not found: {}",
                archive_path.display()
            )));
        }

        let target = params
            .target
            .as_deref()
            .map(|t| self.resolve(t))
            .unwrap_or_else(|| self.context.root().to_path_buf());

        // Ensure target directory exists
        tokio::fs::create_dir_all(&target)
            .await
            .map_err(|e| McpError::ExecutionFailed(format!("Cannot create target dir: {e}")))?;

        let fmt = params
            .format
            .as_deref()
            .unwrap_or_else(|| Self::detect_format(&params.path));

        let output = match fmt {
            "zip" => {
                Command::new("unzip")
                    .args([
                        "-o",
                        &archive_path.display().to_string(),
                        "-d",
                        &target.display().to_string(),
                    ])
                    .output()
                    .await
            }
            _ => {
                let tar_flag = match fmt {
                    "tar.gz" | "tgz" => "xzf",
                    "tar.bz2" | "tbz2" => "xjf",
                    "tar.xz" | "txz" => "xJf",
                    "tar" => "xf",
                    _ => "xzf",
                };
                Command::new("tar")
                    .args([
                        tar_flag,
                        &archive_path.display().to_string(),
                        "-C",
                        &target.display().to_string(),
                    ])
                    .output()
                    .await
            }
        }
        .map_err(|e| McpError::ExecutionFailed(format!("Extract command failed: {e}")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(McpError::ExecutionFailed(format!(
                "Extract failed: {}",
                stderr.trim()
            )));
        }

        Ok(json!({
            "content": [{"type": "text", "text": format!("Extracted {} → {}", params.path, target.display())}],
            "structuredContent": {"archive": {"action": "extract", "path": params.path, "target": target.display().to_string(), "format": fmt}}
        }))
    }

    async fn action_create(&self, params: &ArchiveParams) -> McpResult<Value> {
        let files = params.files.as_ref().ok_or_else(|| {
            McpError::InvalidRequest("'files' is required for create action".into())
        })?;

        if files.is_empty() {
            return Err(McpError::InvalidRequest("'files' cannot be empty".into()));
        }

        let archive_path = self.resolve(&params.path);

        // Ensure parent dir exists
        if let Some(parent) = archive_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| McpError::ExecutionFailed(format!("Cannot create parent dir: {e}")))?;
        }

        let fmt = params
            .format
            .as_deref()
            .unwrap_or_else(|| Self::detect_format(&params.path));

        let output = match fmt {
            "zip" => {
                let mut args = vec!["-r".to_string(), archive_path.display().to_string()];
                args.extend(files.iter().cloned());
                let arg_refs: Vec<&str> = args.iter().map(|s| &**s).collect();
                Command::new("zip")
                    .args(&arg_refs)
                    .current_dir(self.context.root())
                    .output()
                    .await
            }
            _ => {
                let tar_flag = match fmt {
                    "tar.gz" | "tgz" => "czf",
                    "tar.bz2" | "tbz2" => "cjf",
                    "tar.xz" | "txz" => "cJf",
                    "tar" => "cf",
                    _ => "czf",
                };
                let mut args = vec![tar_flag.to_string(), archive_path.display().to_string()];
                args.extend(files.iter().cloned());
                let arg_refs: Vec<&str> = args.iter().map(|s| &**s).collect();
                Command::new("tar")
                    .args(&arg_refs)
                    .current_dir(self.context.root())
                    .output()
                    .await
            }
        }
        .map_err(|e| McpError::ExecutionFailed(format!("Create command failed: {e}")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(McpError::ExecutionFailed(format!(
                "Create failed: {}",
                stderr.trim()
            )));
        }

        // Get file size
        let size = tokio::fs::metadata(&archive_path)
            .await
            .map(|m| m.len())
            .unwrap_or(0);

        Ok(json!({
            "content": [{"type": "text", "text": format!("Created {} ({:.1} KB, {} files)", params.path, size as f64 / 1024.0, files.len())}],
            "structuredContent": {"archive": {"action": "create", "path": params.path, "format": fmt, "size": size, "file_count": files.len()}}
        }))
    }

    async fn action_list(&self, params: &ArchiveParams) -> McpResult<Value> {
        let archive_path = self.resolve(&params.path);
        if !archive_path.exists() {
            return Err(McpError::InvalidRequest(format!(
                "Archive not found: {}",
                archive_path.display()
            )));
        }

        let fmt = params
            .format
            .as_deref()
            .unwrap_or_else(|| Self::detect_format(&params.path));

        let output = match fmt {
            "zip" => {
                Command::new("unzip")
                    .args(["-l", &archive_path.display().to_string()])
                    .output()
                    .await
            }
            _ => {
                let tar_flag = match fmt {
                    "tar.gz" | "tgz" => "tzf",
                    "tar.bz2" | "tbz2" => "tjf",
                    "tar.xz" | "txz" => "tJf",
                    "tar" => "tf",
                    _ => "tzf",
                };
                Command::new("tar")
                    .args([tar_flag, &archive_path.display().to_string()])
                    .output()
                    .await
            }
        }
        .map_err(|e| McpError::ExecutionFailed(format!("List command failed: {e}")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(McpError::ExecutionFailed(format!(
                "List failed: {}",
                stderr.trim()
            )));
        }

        let listing = String::from_utf8_lossy(&output.stdout).to_string();
        let entry_count = listing.lines().count();

        Ok(json!({
            "content": [{"type": "text", "text": format!("Contents of {} ({} entries):\n\n{}", params.path, entry_count, listing.trim())}],
            "structuredContent": {"archive": {"action": "list", "path": params.path, "format": fmt, "entries": entry_count}}
        }))
    }
}

#[async_trait]
impl McpTool for ArchiveTool {
    fn name(&self) -> &str {
        "devit_archive"
    }

    fn description(&self) -> &str {
        "Archive management: extract, create, list. Supports tar.gz, tar.bz2, tar.xz, zip. \
         Format auto-detected from file extension."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["extract", "create", "list"],
                    "description": "Archive action"
                },
                "path": {
                    "type": "string",
                    "description": "Archive file path (relative to project root)"
                },
                "target": {
                    "type": "string",
                    "description": "Target directory for extraction (default: project root)"
                },
                "files": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Files/directories to add (for create action)"
                },
                "format": {
                    "type": "string",
                    "enum": ["tar.gz", "tar.bz2", "tar.xz", "tar", "zip"],
                    "description": "Format override (auto-detected from extension)"
                }
            },
            "required": ["action", "path"]
        })
    }

    async fn execute(&self, params: Value) -> McpResult<Value> {
        let params: ArchiveParams = serde_json::from_value(params)
            .map_err(|e| McpError::InvalidRequest(format!("Invalid params: {e}")))?;

        info!(
            target: "devit_mcp_tools",
            "devit_archive | action={} | path={}",
            params.action, params.path
        );

        match params.action.as_str() {
            "extract" => self.action_extract(&params).await,
            "create" => self.action_create(&params).await,
            "list" => self.action_list(&params).await,
            other => Err(McpError::InvalidRequest(format!(
                "Unknown action '{}'. Valid: extract, create, list",
                other
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_format() {
        assert_eq!(ArchiveTool::detect_format("file.tar.gz"), "tar.gz");
        assert_eq!(ArchiveTool::detect_format("file.tgz"), "tar.gz");
        assert_eq!(ArchiveTool::detect_format("file.tar.bz2"), "tar.bz2");
        assert_eq!(ArchiveTool::detect_format("file.tar.xz"), "tar.xz");
        assert_eq!(ArchiveTool::detect_format("file.zip"), "zip");
        assert_eq!(ArchiveTool::detect_format("file.tar"), "tar");
        assert_eq!(ArchiveTool::detect_format("file.unknown"), "tar.gz");
    }

    #[test]
    fn test_params_extract() {
        let v = json!({"action": "extract", "path": "backup.tar.gz", "target": "output/"});
        let p: ArchiveParams = serde_json::from_value(v).unwrap();
        assert_eq!(p.action, "extract");
        assert_eq!(p.target.unwrap(), "output/");
    }

    #[test]
    fn test_params_create() {
        let v = json!({"action": "create", "path": "out.zip", "files": ["src/", "README.md"]});
        let p: ArchiveParams = serde_json::from_value(v).unwrap();
        assert_eq!(p.files.unwrap().len(), 2);
    }
}
