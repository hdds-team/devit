// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2025-2026 naskel.com

use std::path::PathBuf;

use async_trait::async_trait;
use mcp_core::{McpResult, McpTool};
use serde_json::{json, Value};

use crate::errors::validation_error;

pub struct SoulTool {
    workspace_root: PathBuf,
}

impl SoulTool {
    pub fn new(workspace_root: PathBuf) -> Self {
        Self { workspace_root }
    }

    fn global_soul_path() -> PathBuf {
        dirs_home().join(".devit").join("SOUL.md")
    }

    fn project_soul_path(&self) -> PathBuf {
        self.workspace_root.join(".devit").join("SOUL.md")
    }
}

fn dirs_home() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp"))
}

fn read_file_if_exists(path: &PathBuf) -> Option<String> {
    std::fs::read_to_string(path).ok().filter(|s| !s.trim().is_empty())
}

#[async_trait]
impl McpTool for SoulTool {
    fn name(&self) -> &str {
        "devit_soul"
    }

    fn description(&self) -> &str {
        "Load SOUL.md personality files. Merges global (~/.devit/SOUL.md) and \
         project (<workspace>/.devit/SOUL.md) personality definitions. \
         Use scope parameter to select: merged (default), global, or project."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "scope": {
                    "type": "string",
                    "enum": ["merged", "global", "project"],
                    "default": "merged",
                    "description": "Which SOUL.md to load: merged (global + project), global only, or project only"
                }
            }
        })
    }

    async fn execute(&self, params: Value) -> McpResult<Value> {
        let scope = params
            .get("scope")
            .and_then(Value::as_str)
            .unwrap_or("merged");

        let global_path = Self::global_soul_path();
        let project_path = self.project_soul_path();

        let (content, sources) = match scope {
            "global" => {
                let global = read_file_if_exists(&global_path);
                let found = global.is_some();
                (
                    global.unwrap_or_else(|| format!("No global SOUL.md found at {}", global_path.display())),
                    vec![if found { global_path.to_string_lossy().to_string() } else { String::new() }],
                )
            }
            "project" => {
                let project = read_file_if_exists(&project_path);
                let found = project.is_some();
                (
                    project.unwrap_or_else(|| format!("No project SOUL.md found at {}", project_path.display())),
                    vec![if found { project_path.to_string_lossy().to_string() } else { String::new() }],
                )
            }
            "merged" => {
                let global = read_file_if_exists(&global_path);
                let project = read_file_if_exists(&project_path);

                let mut sources = Vec::new();
                let content = match (&global, &project) {
                    (Some(g), Some(p)) => {
                        sources.push(global_path.to_string_lossy().to_string());
                        sources.push(project_path.to_string_lossy().to_string());
                        format!("{g}\n\n---\n\n{p}")
                    }
                    (Some(g), None) => {
                        sources.push(global_path.to_string_lossy().to_string());
                        g.clone()
                    }
                    (None, Some(p)) => {
                        sources.push(project_path.to_string_lossy().to_string());
                        p.clone()
                    }
                    (None, None) => {
                        "No SOUL.md found. Create ~/.devit/SOUL.md (global) or <workspace>/.devit/SOUL.md (project).".to_string()
                    }
                };
                (content, sources)
            }
            other => {
                return Err(validation_error(&format!(
                    "Unknown scope '{other}'. Valid: merged, global, project"
                )));
            }
        };

        let sources_clean: Vec<&str> = sources.iter().map(|s| s.as_str()).filter(|s| !s.is_empty()).collect();

        Ok(json!({
            "content": [{"type": "text", "text": content}],
            "structuredContent": {
                "soul": {
                    "scope": scope,
                    "sources": sources_clean,
                    "global_path": global_path.to_string_lossy(),
                    "project_path": project_path.to_string_lossy()
                }
            }
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn merged_with_no_files() {
        let dir = tempdir().unwrap();
        // Override HOME so we don't pick up real ~/.devit/SOUL.md
        let fake_home = dir.path().join("fakehome");
        std::fs::create_dir_all(&fake_home).unwrap();
        let prev_home = std::env::var("HOME").ok();
        unsafe { std::env::set_var("HOME", &fake_home); }

        let workspace = dir.path().join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let tool = SoulTool::new(workspace);
        let result = tool.execute(json!({"scope": "merged"})).await.unwrap();
        let text = result["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("No SOUL.md found"));

        // Restore
        match prev_home {
            Some(h) => unsafe { std::env::set_var("HOME", h) },
            None => unsafe { std::env::remove_var("HOME") },
        }
    }

    #[tokio::test]
    async fn project_soul_loaded() {
        let dir = tempdir().unwrap();
        let devit_dir = dir.path().join(".devit");
        std::fs::create_dir_all(&devit_dir).unwrap();
        std::fs::write(devit_dir.join("SOUL.md"), "# Project Soul\nTest content").unwrap();

        let tool = SoulTool::new(dir.path().to_path_buf());
        let result = tool.execute(json!({"scope": "project"})).await.unwrap();
        let text = result["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("Project Soul"));
    }

    #[tokio::test]
    async fn invalid_scope_errors() {
        let dir = tempdir().unwrap();
        let tool = SoulTool::new(dir.path().to_path_buf());
        let result = tool.execute(json!({"scope": "invalid"})).await;
        assert!(result.is_err());
    }
}
