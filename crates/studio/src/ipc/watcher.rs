// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2025-2026 naskel.com

//! File watcher for detecting external file changes
//!
//! Supports two modes:
//! 1. Explicit file watching (watch_file/unwatch_file) - for editor tabs
//! 2. Workspace watching (watch_workspace) - for context engine indexing

use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use parking_lot::Mutex;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{Emitter, WebviewWindow};

/// Payload for file change events
#[derive(serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FileChangedPayload {
    pub path: String,
    pub kind: String, // "modified", "removed"
}

/// Payload for context index events
#[derive(serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ContextFileChangedPayload {
    pub path: String,
    pub action: String, // "reindex", "invalidate"
}

/// File extensions to watch for context indexing
const INDEXABLE_EXTENSIONS: &[&str] = &[
    "rs", "py", "c", "cpp", "cc", "cxx", "h", "hpp", "hh", "hxx", "js", "jsx", "mjs", "ts", "tsx",
];

/// Global file watcher state
struct WatcherState {
    watcher: Option<RecommendedWatcher>,
    watched_paths: HashSet<PathBuf>,
    workspace_root: Option<PathBuf>,
    context_enabled: bool,
    window: Option<WebviewWindow>,
}

static WATCHER_STATE: std::sync::OnceLock<Arc<Mutex<WatcherState>>> = std::sync::OnceLock::new();

fn get_state() -> &'static Arc<Mutex<WatcherState>> {
    WATCHER_STATE.get_or_init(|| {
        Arc::new(Mutex::new(WatcherState {
            watcher: None,
            watched_paths: HashSet::new(),
            workspace_root: None,
            context_enabled: false,
            window: None,
        }))
    })
}

/// Initialize the file watcher with a window for emitting events
pub fn init_watcher(window: WebviewWindow) {
    let state = get_state();
    let mut guard = state.lock();

    // Store window reference
    guard.window = Some(window.clone());

    // Create watcher if not exists
    if guard.watcher.is_none() {
        let window_clone = window.clone();

        let watcher = RecommendedWatcher::new(
            move |res: Result<Event, notify::Error>| {
                if let Ok(event) = res {
                    handle_event(event, &window_clone);
                }
            },
            Config::default(),
        );

        match watcher {
            Ok(w) => {
                guard.watcher = Some(w);
                tracing::info!("File watcher initialized");
            }
            Err(e) => {
                tracing::error!("Failed to create file watcher: {}", e);
            }
        }
    }
}

fn handle_event(event: Event, window: &WebviewWindow) {
    let kind = match event.kind {
        EventKind::Modify(_) => "modified",
        EventKind::Remove(_) => "removed",
        EventKind::Create(_) => "created",
        _ => return, // Ignore other events
    };

    for path in event.paths {
        let state = get_state();
        let guard = state.lock();
        let path_str = path.to_string_lossy().to_string();

        // Check if this path is being explicitly watched (editor tabs)
        if guard.watched_paths.contains(&path) {
            tracing::debug!("File changed: {} ({})", path_str, kind);

            let _ = window.emit(
                "file:changed",
                FileChangedPayload {
                    path: path_str.clone(),
                    kind: kind.to_string(),
                },
            );
        }

        // Check if we should emit context indexing event
        if guard.context_enabled {
            if let Some(ref workspace_root) = guard.workspace_root {
                // Only process files under workspace root
                if path.starts_with(workspace_root) {
                    // Check if it's an indexable file
                    if is_indexable_file(&path) {
                        let action = if kind == "removed" {
                            "invalidate"
                        } else {
                            "reindex"
                        };

                        tracing::debug!("Context file changed: {} ({})", path_str, action);

                        let _ = window.emit(
                            "context:file_changed",
                            ContextFileChangedPayload {
                                path: path_str,
                                action: action.to_string(),
                            },
                        );
                    }
                }
            }
        }
    }
}

/// Check if a file should be indexed
fn is_indexable_file(path: &PathBuf) -> bool {
    // Skip hidden files/directories
    if path
        .components()
        .any(|c| c.as_os_str().to_string_lossy().starts_with('.'))
    {
        return false;
    }

    // Skip common non-source directories
    let path_str = path.to_string_lossy();
    if path_str.contains("/target/")
        || path_str.contains("/node_modules/")
        || path_str.contains("/__pycache__/")
        || path_str.contains("/dist/")
        || path_str.contains("/build/")
    {
        return false;
    }

    // Check extension
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| INDEXABLE_EXTENSIONS.contains(&ext))
        .unwrap_or(false)
}

/// Start watching a file
#[tauri::command]
pub async fn watch_file(path: String) -> Result<(), String> {
    let path_buf = PathBuf::from(&path);

    let state = get_state();
    let mut guard = state.lock();

    // Add to watched set
    if guard.watched_paths.contains(&path_buf) {
        return Ok(()); // Already watching
    }

    // Add watch
    if let Some(watcher) = &mut guard.watcher {
        watcher
            .watch(&path_buf, RecursiveMode::NonRecursive)
            .map_err(|e| format!("Failed to watch file: {}", e))?;

        guard.watched_paths.insert(path_buf);
        tracing::debug!("Now watching: {}", path);
    } else {
        return Err("Watcher not initialized".to_string());
    }

    Ok(())
}

/// Stop watching a file
#[tauri::command]
pub async fn unwatch_file(path: String) -> Result<(), String> {
    let path_buf = PathBuf::from(&path);

    let state = get_state();
    let mut guard = state.lock();

    if !guard.watched_paths.contains(&path_buf) {
        return Ok(()); // Not watching
    }

    // Remove watch
    if let Some(watcher) = &mut guard.watcher {
        let _ = watcher.unwatch(&path_buf); // Ignore errors
        guard.watched_paths.remove(&path_buf);
        tracing::debug!("Stopped watching: {}", path);
    }

    Ok(())
}

/// Get list of watched files
#[tauri::command]
pub async fn list_watched_files() -> Result<Vec<String>, String> {
    let state = get_state();
    let guard = state.lock();

    Ok(guard
        .watched_paths
        .iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect())
}

/// Enable workspace watching for context engine indexing
#[tauri::command]
pub async fn watch_workspace_for_context(workspace_path: String) -> Result<(), String> {
    let workspace_root = PathBuf::from(&workspace_path);

    let state = get_state();
    let mut guard = state.lock();

    // Set workspace root and enable context watching
    guard.workspace_root = Some(workspace_root.clone());
    guard.context_enabled = true;

    // Add recursive watch on workspace
    if let Some(watcher) = &mut guard.watcher {
        watcher
            .watch(&workspace_root, RecursiveMode::Recursive)
            .map_err(|e| format!("Failed to watch workspace: {}", e))?;

        tracing::info!(
            "Watching workspace for context indexing: {}",
            workspace_path
        );
    } else {
        return Err("Watcher not initialized".to_string());
    }

    Ok(())
}

/// Disable workspace watching for context engine
#[tauri::command]
pub async fn unwatch_workspace_for_context() -> Result<(), String> {
    let state = get_state();
    let mut guard = state.lock();

    if let Some(ref workspace_root) = guard.workspace_root.take() {
        if let Some(watcher) = &mut guard.watcher {
            let _ = watcher.unwatch(workspace_root);
        }
    }

    guard.context_enabled = false;
    tracing::info!("Stopped watching workspace for context indexing");

    Ok(())
}

/// Check if context watching is enabled
#[tauri::command]
pub async fn is_context_watching_enabled() -> bool {
    let state = get_state();
    let guard = state.lock();
    guard.context_enabled
}
