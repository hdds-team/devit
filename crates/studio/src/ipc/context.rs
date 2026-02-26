// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2025-2026 naskel.com

//! Context Engine IPC commands for RAG-based code context

use context_engine::{CancellationToken, ContextChunk, ContextEngine, ContextEngineConfig};
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State, WebviewWindow};
use tokio::sync::RwLock;

/// Internal state for context engine with cancellation support
pub struct ContextEngineInner {
    pub engine: Arc<ContextEngine>,
    pub cancel_token: CancellationToken,
}

/// Managed state for context engine
/// Wrapped in Arc<RwLock<...>> for thread-safe access
pub type ContextEngineState = Arc<RwLock<Option<ContextEngineInner>>>;

/// Progress event payload
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexProgressPayload {
    pub current_file: String,
    pub files_done: usize,
    pub files_total: usize,
    pub chunks_created: usize,
}

/// Stats event payload
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexStatsPayload {
    pub files_indexed: usize,
    pub chunks_created: usize,
    pub total_tokens: usize,
    pub duration_ms: u64,
}

/// Status event for StatusBar
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextStatusPayload {
    pub status: String, // "initializing", "indexing", "ready", "error"
    pub message: String,
    pub progress: Option<(usize, usize)>, // (current, total)
}

/// Auto-initialize context engine for a workspace (non-blocking)
/// This spawns a background task that emits progress events
pub fn auto_init_context_engine(
    app: AppHandle,
    workspace_path: PathBuf,
    ollama_url: Option<String>,
    embedding_model: Option<String>,
) {
    let window = app.get_webview_window("main");

    // Use Tauri's async runtime instead of tokio::spawn directly
    // This ensures we're using the runtime that Tauri manages
    tauri::async_runtime::spawn(async move {
        // Emit initializing status
        if let Some(ref w) = window {
            let _ = w.emit(
                "context:status",
                ContextStatusPayload {
                    status: "initializing".to_string(),
                    message: "Initializing context engine...".to_string(),
                    progress: None,
                },
            );
        }

        // Get context state from app
        let context_state: ContextEngineState = match app.try_state::<ContextEngineState>() {
            Some(s) => (*s).clone(),
            None => {
                tracing::error!("Context state not found");
                return;
            }
        };

        // Check if already initialized
        {
            let guard = context_state.read().await;
            if guard.is_some() {
                tracing::info!("Context engine already initialized, skipping");
                if let Some(ref w) = window {
                    let _ = w.emit(
                        "context:status",
                        ContextStatusPayload {
                            status: "ready".to_string(),
                            message: "Context engine ready".to_string(),
                            progress: None,
                        },
                    );
                }
                return;
            }
        }

        // Initialize context engine
        let config = ContextEngineConfig {
            workspace_root: workspace_path.clone(),
            store_path: PathBuf::from(".devit/embeddings"),
            ollama_url: ollama_url.unwrap_or_else(|| {
                std::env::var("OLLAMA_HOST")
                    .or_else(|_| std::env::var("DEVIT_OLLAMA_URL"))
                    .unwrap_or_else(|_| "http://localhost:11434".to_string())
            }),
            embedding_model: embedding_model.unwrap_or_else(|| "nomic-embed-text".to_string()),
            ..Default::default()
        };

        let engine = match ContextEngine::new(config).await {
            Ok(e) => e,
            Err(err) => {
                tracing::error!("Failed to initialize context engine: {}", err);
                if let Some(ref w) = window {
                    let _ = w.emit(
                        "context:status",
                        ContextStatusPayload {
                            status: "error".to_string(),
                            message: format!("Init failed: {}", err),
                            progress: None,
                        },
                    );
                }
                return;
            }
        };

        // Create cancellation token for this indexing session
        let cancel_token = CancellationToken::new();

        // Store engine with its cancellation token
        let engine = Arc::new(engine);
        {
            let mut guard = context_state.write().await;
            *guard = Some(ContextEngineInner {
                engine: Arc::clone(&engine),
                cancel_token: cancel_token.clone(),
            });
        }

        tracing::info!("Context engine initialized for {:?}", workspace_path);

        // NOTE: We don't watch the entire workspace anymore - it causes inotify limits
        // on large projects. Instead, we rely on:
        // 1. Individual file watches when files are opened in the editor
        // 2. Reindexing on file save events
        // The Editor component already calls watcher.watch(path) for open files.

        // Now index workspace
        // NOTE: We use our local Arc clone, NOT the state lock
        // This allows workspace changes to clear the state without blocking on indexing
        if let Some(ref w) = window {
            let _ = w.emit(
                "context:status",
                ContextStatusPayload {
                    status: "indexing".to_string(),
                    message: "Indexing workspace...".to_string(),
                    progress: Some((0, 0)),
                },
            );
        }

        // Use local Arc clone for indexing - no lock held!
        // Pass the cancel_token so workspace changes can abort this indexing
        {
            let window_clone = window.clone();

            let result = engine
                .index_workspace(
                    |progress| {
                        if let Some(ref w) = window_clone {
                            let _ = w.emit(
                                "context:status",
                                ContextStatusPayload {
                                    status: "indexing".to_string(),
                                    message: format!(
                                        "Indexing: {}",
                                        truncate_path(&progress.current_file, 40)
                                    ),
                                    progress: Some((progress.files_done, progress.files_total)),
                                },
                            );
                            let _ = w.emit(
                                "context:index_progress",
                                IndexProgressPayload {
                                    current_file: progress.current_file,
                                    files_done: progress.files_done,
                                    files_total: progress.files_total,
                                    chunks_created: progress.chunks_created,
                                },
                            );
                        }
                    },
                    cancel_token,
                )
                .await;

            match result {
                Ok(stats) => {
                    tracing::info!(
                        "Indexing complete: {} files, {} chunks in {}ms",
                        stats.files_indexed,
                        stats.chunks_created,
                        stats.duration_ms
                    );
                    if let Some(ref w) = window {
                        let _ = w.emit(
                            "context:status",
                            ContextStatusPayload {
                                status: "ready".to_string(),
                                message: format!("Indexed {} files", stats.files_indexed),
                                progress: None,
                            },
                        );
                        let _ = w.emit(
                            "context:index_complete",
                            IndexStatsPayload {
                                files_indexed: stats.files_indexed,
                                chunks_created: stats.chunks_created,
                                total_tokens: stats.total_tokens,
                                duration_ms: stats.duration_ms,
                            },
                        );
                    }
                }
                Err(context_engine::ContextError::Cancelled) => {
                    // Cancelled by workspace change - this is expected, don't emit error
                    tracing::info!("Indexing cancelled (workspace changed)");
                }
                Err(err) => {
                    tracing::error!("Indexing failed: {}", err);
                    if let Some(ref w) = window {
                        let _ = w.emit(
                            "context:status",
                            ContextStatusPayload {
                                status: "error".to_string(),
                                message: format!("Indexing failed: {}", err),
                                progress: None,
                            },
                        );
                    }
                }
            }
        }
    });
}

/// Truncate path for display
fn truncate_path(path: &str, max_len: usize) -> String {
    if path.len() <= max_len {
        path.to_string()
    } else {
        format!("...{}", &path[path.len() - max_len + 3..])
    }
}

/// Initialize context engine for a workspace
#[tauri::command]
pub async fn init_context_engine(
    workspace_path: String,
    ollama_url: Option<String>,
    embedding_model: Option<String>,
    state: State<'_, ContextEngineState>,
) -> Result<(), String> {
    let config = ContextEngineConfig {
        workspace_root: PathBuf::from(&workspace_path),
        store_path: PathBuf::from(".devit/embeddings"),
        ollama_url: ollama_url.unwrap_or_else(|| {
            std::env::var("OLLAMA_HOST")
                .or_else(|_| std::env::var("DEVIT_OLLAMA_URL"))
                .unwrap_or_else(|_| "http://localhost:11434".to_string())
        }),
        embedding_model: embedding_model.unwrap_or_else(|| "nomic-embed-text".to_string()),
        ..Default::default()
    };

    let engine = ContextEngine::new(config)
        .await
        .map_err(|e| format!("Failed to initialize context engine: {}", e))?;

    let mut guard = state.write().await;
    *guard = Some(ContextEngineInner {
        engine: Arc::new(engine),
        cancel_token: CancellationToken::new(),
    });

    tracing::info!("Context engine initialized for {}", workspace_path);
    Ok(())
}

/// Index the entire workspace
#[tauri::command]
pub async fn index_workspace(
    window: WebviewWindow,
    state: State<'_, ContextEngineState>,
) -> Result<IndexStatsPayload, String> {
    // Clone Arc and token, release lock immediately to avoid blocking workspace changes
    let (engine, cancel_token) = {
        let guard = state.read().await;
        let inner = guard.as_ref().ok_or("Context engine not initialized")?;
        (Arc::clone(&inner.engine), inner.cancel_token.clone())
    };

    let stats = engine
        .index_workspace(
            |progress| {
                let payload = IndexProgressPayload {
                    current_file: progress.current_file,
                    files_done: progress.files_done,
                    files_total: progress.files_total,
                    chunks_created: progress.chunks_created,
                };
                let _ = window.emit("context:index_progress", payload);
            },
            cancel_token,
        )
        .await
        .map_err(|e| format!("Indexing failed: {}", e))?;

    let result = IndexStatsPayload {
        files_indexed: stats.files_indexed,
        chunks_created: stats.chunks_created,
        total_tokens: stats.total_tokens,
        duration_ms: stats.duration_ms,
    };

    let _ = window.emit("context:index_complete", result.clone());
    Ok(result)
}

/// Reindex a single file (called after file change)
#[tauri::command]
pub async fn reindex_file(
    file_path: String,
    state: State<'_, ContextEngineState>,
) -> Result<(), String> {
    let guard = state.read().await;
    let inner = guard.as_ref().ok_or("Context engine not initialized")?;

    let path = PathBuf::from(&file_path);

    inner
        .engine
        .index_file(&path)
        .await
        .map_err(|e| format!("Reindex failed: {}", e))?;

    tracing::debug!("Reindexed: {}", file_path);
    Ok(())
}

/// Invalidate a file from the index (called after file deletion)
#[tauri::command]
pub async fn invalidate_file(
    file_path: String,
    state: State<'_, ContextEngineState>,
) -> Result<(), String> {
    let guard = state.read().await;
    let inner = guard.as_ref().ok_or("Context engine not initialized")?;

    let path = PathBuf::from(&file_path);

    inner
        .engine
        .invalidate_file(&path)
        .await
        .map_err(|e| format!("Invalidate failed: {}", e))?;

    tracing::debug!("Invalidated from index: {}", file_path);
    Ok(())
}

/// Query for relevant context
#[tauri::command]
pub async fn query_context(
    query: String,
    max_tokens: Option<usize>,
    state: State<'_, ContextEngineState>,
) -> Result<Vec<ContextChunkPayload>, String> {
    let guard = state.read().await;
    let inner = guard.as_ref().ok_or("Context engine not initialized")?;

    let max_tokens = max_tokens.unwrap_or(4096);

    let chunks = inner
        .engine
        .query(&query, max_tokens)
        .await
        .map_err(|e| format!("Query failed: {}", e))?;

    Ok(chunks.into_iter().map(ContextChunkPayload::from).collect())
}

/// Get context for specific files
#[tauri::command]
pub async fn get_file_context(
    file_paths: Vec<String>,
    state: State<'_, ContextEngineState>,
) -> Result<Vec<ContextChunkPayload>, String> {
    let guard = state.read().await;
    let inner = guard.as_ref().ok_or("Context engine not initialized")?;

    let paths: Vec<PathBuf> = file_paths.into_iter().map(PathBuf::from).collect();

    let chunks = inner
        .engine
        .get_file_context(&paths)
        .await
        .map_err(|e| format!("Failed to get file context: {}", e))?;

    Ok(chunks.into_iter().map(ContextChunkPayload::from).collect())
}

/// Serializable context chunk for frontend
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextChunkPayload {
    pub id: String,
    pub file_path: String,
    pub start_line: usize,
    pub end_line: usize,
    pub content: String,
    pub language: String,
    pub chunk_type: String,
    pub symbol_name: Option<String>,
    pub token_count: usize,
    pub score: f32,
}

impl From<ContextChunk> for ContextChunkPayload {
    fn from(chunk: ContextChunk) -> Self {
        Self {
            id: chunk.chunk.id,
            file_path: chunk.chunk.file_path.to_string_lossy().to_string(),
            start_line: chunk.chunk.start_line,
            end_line: chunk.chunk.end_line,
            content: chunk.chunk.content,
            language: chunk.chunk.language,
            chunk_type: chunk.chunk.chunk_type.as_str().to_string(),
            symbol_name: chunk.chunk.symbol_name,
            token_count: chunk.chunk.token_count,
            score: chunk.score,
        }
    }
}
