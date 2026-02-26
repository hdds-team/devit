// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2025-2026 naskel.com

//! Workspace IPC commands - file tree, git integration

use crate::ipc::context::ContextEngineState;
use crate::state::{AppState, ToolState};
use crate::workspace::Workspace;
use parking_lot::RwLock;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, State};

/// Type alias for managed tool state
type ManagedToolState = Arc<tokio::sync::RwLock<ToolState>>;

#[derive(serde::Serialize)]
pub struct FileEntry {
    pub path: String,
    pub name: String,
    pub is_dir: bool,
    pub children: Option<Vec<FileEntry>>,
}

#[derive(serde::Serialize)]
pub struct GitStatus {
    pub branch: Option<String>,
    pub modified: Vec<String>,
    pub staged: Vec<String>,
    pub untracked: Vec<String>,
}

/// Open a folder as workspace
#[tauri::command]
pub async fn open_folder(
    app: AppHandle,
    path: String,
    state: State<'_, Arc<RwLock<AppState>>>,
    tool_state: State<'_, ManagedToolState>,
    context_state: State<'_, ContextEngineState>,
) -> Result<FileEntry, String> {
    let path_buf = PathBuf::from(&path);

    if !path_buf.is_dir() {
        return Err("Path is not a directory".into());
    }

    let workspace = Workspace::new(path_buf.clone());

    // Update app state and save last workspace
    {
        let mut st = state.write();
        st.workspace = Some(workspace);
        st.settings.last_workspace = Some(path.clone());

        // Save settings to persist last_workspace
        if let Err(e) = save_last_workspace(&st.settings) {
            tracing::warn!("Failed to save last workspace: {}", e);
        }
    }

    // Update MCP tools working directory
    {
        let mut ts = tool_state.write().await;
        if let Err(e) = ts.set_workspace(path_buf.clone()).await {
            tracing::warn!("Failed to update tool workspace: {}", e);
        } else {
            tracing::info!("Updated MCP tools workspace to: {:?}", path_buf);
        }
    }

    // Reset context engine for new workspace and auto-init
    // First, cancel any ongoing indexing and clear the engine
    {
        let mut guard = context_state.write().await;
        if let Some(ref inner) = *guard {
            // Cancel ongoing indexing - this signals the indexer to stop
            inner.cancel_token.cancel();
            tracing::info!("Cancelled previous indexing");
        }
        *guard = None;
        tracing::info!("Reset context engine for new workspace");
    }

    // Trigger auto-init of context engine for the new workspace (non-blocking)
    crate::ipc::context::auto_init_context_engine(
        app,
        path_buf.clone(),
        None, // Use default Ollama URL
        None, // Use default embedding model
    );

    // Return root tree with depth=1 (immediate children only)
    // Deeper levels are loaded on-demand via list_files
    list_files_internal(&path, Some(1))
}

/// Helper to save settings with last_workspace
fn save_last_workspace(settings: &crate::state::Settings) -> Result<(), String> {
    let path = dirs::config_dir()
        .map(|d| d.join("devit-studio").join("settings.json"))
        .ok_or("Could not determine config directory")?;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create config directory: {}", e))?;
    }

    let content = serde_json::to_string_pretty(settings)
        .map_err(|e| format!("Failed to serialize settings: {}", e))?;

    std::fs::write(&path, content).map_err(|e| format!("Failed to write settings: {}", e))?;

    Ok(())
}

/// Internal file listing (doesn't need state)
fn list_files_internal(path: &str, max_depth: Option<u32>) -> Result<FileEntry, String> {
    let path_buf = PathBuf::from(path);
    let depth = max_depth.unwrap_or(2);
    build_file_tree(&path_buf, depth)
}

/// Get current workspace (called on app startup to restore last folder)
#[tauri::command]
pub async fn get_workspace(
    state: State<'_, Arc<RwLock<AppState>>>,
) -> Result<Option<FileEntry>, String> {
    let st = state.read();
    match &st.workspace {
        Some(ws) => {
            let root = ws.root.clone();
            drop(st); // Release lock before I/O
            list_files_internal(&root.to_string_lossy(), Some(1)).map(Some)
        }
        None => Ok(None),
    }
}

/// List files in directory (recursive to max_depth)
#[tauri::command]
pub async fn list_files(
    path: String,
    max_depth: Option<u32>,
    _state: State<'_, Arc<RwLock<AppState>>>,
) -> Result<FileEntry, String> {
    let path_buf = PathBuf::from(&path);
    let depth = max_depth.unwrap_or(2);

    build_file_tree(&path_buf, depth)
}

/// Get git status for workspace
#[tauri::command]
pub async fn get_git_status(
    state: State<'_, Arc<RwLock<AppState>>>,
) -> Result<Option<GitStatus>, String> {
    let workspace_path = {
        let st = state.read();
        match &st.workspace {
            Some(ws) => ws.root.clone(),
            None => return Ok(None),
        }
    };

    // Check if this is a git repository
    let git_dir = workspace_path.join(".git");
    if !git_dir.exists() {
        return Ok(None);
    }

    // Get current branch
    let branch = get_git_branch(&workspace_path).await;

    // Get modified, staged, untracked files
    let (modified, staged, untracked) = get_git_file_status(&workspace_path).await;

    Ok(Some(GitStatus {
        branch,
        modified,
        staged,
        untracked,
    }))
}

/// Get current git branch name
async fn get_git_branch(workspace: &PathBuf) -> Option<String> {
    let output = tokio::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(workspace)
        .output()
        .await
        .ok()?;

    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        None
    }
}

/// Git diff line change for editor gutter
#[derive(serde::Serialize)]
pub struct GitDiffLine {
    pub line: u32,
    pub kind: String, // "added", "modified", "deleted"
}

/// Get git diff for a specific file (for editor gutter)
#[tauri::command]
pub async fn get_file_diff(
    path: String,
    state: State<'_, Arc<RwLock<AppState>>>,
) -> Result<Vec<GitDiffLine>, String> {
    // Get workspace path
    let workspace_path = {
        let st = state.read();
        match &st.workspace {
            Some(ws) => ws.root.clone(),
            None => return Ok(Vec::new()),
        }
    };

    let path_buf = PathBuf::from(&path);

    // Get relative path from workspace
    let rel_path = match path_buf.strip_prefix(&workspace_path) {
        Ok(p) => p.to_string_lossy().to_string(),
        Err(_) => path.clone(),
    };

    // Run git diff --unified=0 to get line-by-line changes
    let output = tokio::process::Command::new("git")
        .args(["diff", "--unified=0", "--no-color", &rel_path])
        .current_dir(&workspace_path)
        .output()
        .await
        .map_err(|e| format!("Failed to run git diff: {}", e))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut changes: Vec<GitDiffLine> = Vec::new();

    // Parse unified diff hunks: @@ -old,count +new,count @@
    for line in stdout.lines() {
        if line.starts_with("@@") {
            // Extract the +new,count part
            if let Some(plus_idx) = line.find('+') {
                let after_plus = &line[plus_idx + 1..];
                let end_idx = after_plus.find(' ').unwrap_or(after_plus.len());
                let range_str = &after_plus[..end_idx];

                // Parse "line,count" or just "line"
                let (start_line, count) = if let Some(comma) = range_str.find(',') {
                    let line_num: u32 = range_str[..comma].parse().unwrap_or(0);
                    let cnt: u32 = range_str[comma + 1..].parse().unwrap_or(1);
                    (line_num, cnt)
                } else {
                    let line_num: u32 = range_str.parse().unwrap_or(0);
                    (line_num, 1)
                };

                // Generate change markers for each affected line
                if count == 0 {
                    // Deletion (0 lines in new = lines were removed after this position)
                    changes.push(GitDiffLine {
                        line: start_line.max(1),
                        kind: "deleted".to_string(),
                    });
                } else {
                    for i in 0..count {
                        changes.push(GitDiffLine {
                            line: start_line + i,
                            kind: "modified".to_string(),
                        });
                    }
                }
            }
        }
    }

    // Also check git status for untracked files
    let status_output = tokio::process::Command::new("git")
        .args(["status", "--porcelain", &rel_path])
        .current_dir(&workspace_path)
        .output()
        .await;

    if let Ok(out) = status_output {
        let status_line = String::from_utf8_lossy(&out.stdout);
        for line in status_line.lines() {
            if line.starts_with("??") || line.starts_with("A ") {
                // File is new/untracked - mark all lines as added
                // We don't know line count here, so we rely on the diff output
                // But if diff is empty and file is new, we should mark it specially
                if changes.is_empty() {
                    // Read file line count
                    if let Ok(content) = tokio::fs::read_to_string(&path_buf).await {
                        let line_count = u32::try_from(content.lines().count()).unwrap_or(u32::MAX);
                        for i in 1..=line_count {
                            changes.push(GitDiffLine {
                                line: i,
                                kind: "added".to_string(),
                            });
                        }
                    }
                }
            }
        }
    }

    Ok(changes)
}

/// Get modified, staged, and untracked files
async fn get_git_file_status(workspace: &PathBuf) -> (Vec<String>, Vec<String>, Vec<String>) {
    let mut modified = Vec::new();
    let mut staged = Vec::new();
    let mut untracked = Vec::new();

    // Run git status --porcelain
    let output = match tokio::process::Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(workspace)
        .output()
        .await
    {
        Ok(o) => o,
        Err(_) => return (modified, staged, untracked),
    };

    if !output.status.success() {
        return (modified, staged, untracked);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        if line.len() < 3 {
            continue;
        }

        let index_status = line.chars().next().unwrap_or(' ');
        let worktree_status = line.chars().nth(1).unwrap_or(' ');
        let file_path = line[3..].to_string();

        // Staged changes (index has changes)
        if index_status != ' ' && index_status != '?' {
            staged.push(file_path.clone());
        }

        // Modified in worktree (not staged)
        if worktree_status == 'M' || worktree_status == 'D' {
            modified.push(file_path.clone());
        }

        // Untracked files
        if index_status == '?' && worktree_status == '?' {
            untracked.push(file_path);
        }
    }

    (modified, staged, untracked)
}

/// Search result from ripgrep
#[derive(serde::Serialize)]
pub struct SearchResult {
    pub path: String,
    pub line: u32,
    pub column: u32,
    pub text: String,
    pub match_text: String,
}

/// Search in files using ripgrep
#[tauri::command]
pub async fn search_in_files(
    pattern: String,
    glob: Option<String>,
    case_sensitive: Option<bool>,
    state: State<'_, Arc<RwLock<AppState>>>,
) -> Result<Vec<SearchResult>, String> {
    // Get workspace path
    let workspace_path = {
        let st = state.read();
        match &st.workspace {
            Some(ws) => ws.root.clone(),
            None => return Err("No workspace open".into()),
        }
    };

    // Build ripgrep command
    let mut args = vec![
        "--json".to_string(),
        "--max-count=100".to_string(), // Limit results per file
    ];

    if case_sensitive != Some(true) {
        args.push("-i".to_string()); // Case insensitive by default
    }

    if let Some(g) = glob {
        args.push("--glob".to_string());
        args.push(g);
    }

    args.push(pattern);
    args.push(".".to_string());

    tracing::debug!("Running ripgrep: rg {:?}", args);

    let output = tokio::process::Command::new("rg")
        .args(&args)
        .current_dir(&workspace_path)
        .output()
        .await
        .map_err(|e| format!("Failed to run ripgrep: {}. Is ripgrep installed?", e))?;

    // Parse JSON output
    let mut results = Vec::new();
    let stdout = String::from_utf8_lossy(&output.stdout);

    for line in stdout.lines() {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(line) {
            if json["type"] == "match" {
                if let Some(data) = json.get("data") {
                    let path = data["path"]["text"].as_str().unwrap_or("").to_string();
                    let line_num =
                        u32::try_from(data["line_number"].as_u64().unwrap_or(0)).unwrap_or(0);
                    let text = data["lines"]["text"]
                        .as_str()
                        .unwrap_or("")
                        .trim_end()
                        .to_string();

                    // Get match details
                    let (column, match_text) = if let Some(submatches) =
                        data["submatches"].as_array()
                    {
                        if let Some(first) = submatches.first() {
                            let start =
                                u32::try_from(first["start"].as_u64().unwrap_or(0)).unwrap_or(0);
                            let matched = first["match"]["text"].as_str().unwrap_or("").to_string();
                            (start + 1, matched)
                        } else {
                            (1, String::new())
                        }
                    } else {
                        (1, String::new())
                    };

                    // Convert relative path to absolute
                    let full_path = workspace_path.join(&path);

                    results.push(SearchResult {
                        path: full_path.to_string_lossy().to_string(),
                        line: line_num,
                        column,
                        text,
                        match_text,
                    });
                }
            }
        }
    }

    // Limit total results
    if results.len() > 500 {
        results.truncate(500);
    }

    tracing::info!("Search found {} results", results.len());
    Ok(results)
}

fn build_file_tree(path: &PathBuf, depth: u32) -> Result<FileEntry, String> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string_lossy().to_string());

    let is_dir = path.is_dir();

    let children = if is_dir && depth > 0 {
        let mut entries = Vec::new();

        let read_dir =
            std::fs::read_dir(path).map_err(|e| format!("Cannot read directory: {}", e))?;

        for entry in read_dir.flatten() {
            let entry_path = entry.path();
            let entry_name = entry.file_name().to_string_lossy().to_string();

            // Skip hidden files and common ignored dirs
            if entry_name.starts_with('.')
                || entry_name == "node_modules"
                || entry_name == "target"
                || entry_name == "__pycache__"
                || entry_name == "venv"
            {
                continue;
            }

            if let Ok(child) = build_file_tree(&entry_path, depth - 1) {
                entries.push(child);
            }
        }

        // Sort: directories first, then alphabetically
        entries.sort_by(|a, b| match (a.is_dir, b.is_dir) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
        });

        Some(entries)
    } else {
        None
    };

    Ok(FileEntry {
        path: path.to_string_lossy().to_string(),
        name,
        is_dir,
        children,
    })
}
