// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2025-2026 naskel.com

//! Ghost cursor IPC commands
//!
//! The ghost cursor is a secondary cursor (different color) that shows
//! AI-generated edits in real-time. User can accept or reject.

use crate::ghost::GhostSession;
use crate::state::AppState;
use devit_backend_core::{ChatMessage, ChatRequest};
use devit_llama_cpp::LlamaCppBackend;
use devit_lmstudio::LmstudioBackend;
use devit_ollama::OllamaBackend;
use parking_lot::RwLock;
use std::sync::Arc;
use tauri::{Emitter, State, Window};

/// Unified stream chunk for ghost (same pattern as llm.rs)
enum GhostStreamChunk {
    Delta(String),
    Done,
    Error(String),
}

#[derive(serde::Deserialize)]
pub struct CodeContext {
    /// Code before cursor
    pub before: String,
    /// Code after cursor
    pub after: String,
}

#[derive(serde::Deserialize)]
pub struct GhostEditRequest {
    /// File being edited
    pub file_path: String,
    /// Cursor position (line, column)
    pub position: Position,
    /// User instruction for the edit
    pub prompt: String,
    /// Context: code before/after cursor
    pub context: Option<CodeContext>,
}

#[derive(serde::Deserialize, serde::Serialize, Clone)]
pub struct Position {
    pub line: u32,
    pub column: u32,
}

#[derive(serde::Serialize, Clone)]
pub struct GhostUpdate {
    /// Session ID
    pub session_id: String,
    /// Current ghost cursor position
    pub position: Position,
    /// Text being inserted (delta)
    pub delta: String,
    /// Full pending text so far
    pub pending_text: String,
    /// Is the stream complete?
    pub done: bool,
}

#[derive(serde::Serialize)]
pub struct GhostStateResponse {
    pub session_id: String,
    pub state: String,
    pub pending_text: String,
    pub start_position: Position,
    pub current_position: Position,
}

/// Start a ghost editing session
#[tauri::command]
pub async fn start_ghost_edit(
    request: GhostEditRequest,
    window: Window,
    state: State<'_, Arc<RwLock<AppState>>>,
) -> Result<String, String> {
    let session_id = uuid::Uuid::new_v4().to_string();

    // Create session
    let session = GhostSession::new(
        session_id.clone(),
        request.file_path.clone(),
        request.position.clone(),
    );

    {
        let mut st = state.write();
        st.ghost_sessions.insert(session_id.clone(), session);
    }

    // Build system prompt with context
    let file_ext = request.file_path.rsplit('.').next().unwrap_or("txt");

    let context_info = match &request.context {
        Some(ctx) => format!(
            "Code before cursor:\n```{}\n{}\n```\n\nCode after cursor:\n```{}\n{}\n```",
            file_ext, ctx.before, file_ext, ctx.after
        ),
        None => String::new(),
    };

    let system_prompt = format!(
        "You are a code completion assistant. Generate ONLY the code to insert at the cursor position. \
        Do NOT include any explanations, markdown formatting, or code blocks. \
        Output raw code only.\n\n\
        File: {}\n\n{}",
        request.file_path,
        context_info
    );

    let user_prompt = request.prompt;

    // Create LLM request
    let messages = vec![
        ChatMessage {
            role: "system".to_string(),
            content: system_prompt,
            tool_calls: None,
            tool_name: None,
            images: None,
        },
        ChatMessage {
            role: "user".to_string(),
            content: user_prompt,
            tool_calls: None,
            tool_name: None,
            images: None,
        },
    ];

    let chat_request = ChatRequest::new(messages);

    // Get current provider and model from app state
    let (provider_id, model_name, ollama_url, lmstudio_url, llamacpp_url) = {
        let st = state.read();
        let provider = st
            .current_provider
            .clone()
            .unwrap_or_else(|| "ollama".into());
        let model = st
            .current_model
            .clone()
            .unwrap_or_else(|| "qwen2.5-coder:7b".into());
        (
            provider,
            model,
            st.settings.ollama_url.clone(),
            st.settings.lmstudio_url.clone(),
            st.settings.llamacpp_url.clone(),
        )
    };

    // Create a channel for unified streaming
    let (tx, mut rx) = tokio::sync::mpsc::channel::<GhostStreamChunk>(100);

    // Spawn backend-specific streaming task
    match provider_id.as_str() {
        "lmstudio" => {
            let backend = LmstudioBackend::new(
                format!("{}/v1", lmstudio_url.trim_end_matches('/')),
                model_name,
            );
            let mut stream = backend
                .chat_stream(chat_request)
                .await
                .map_err(|e| e.to_string())?;
            let tx = tx.clone();
            tokio::spawn(async move {
                while let Some(chunk) = stream.recv().await {
                    match chunk {
                        devit_lmstudio::StreamChunk::Delta(s) => {
                            let _ = tx.send(GhostStreamChunk::Delta(s)).await;
                        }
                        devit_lmstudio::StreamChunk::Thinking(_) => {} // Ignore thinking in ghost mode
                        devit_lmstudio::StreamChunk::Done(_) => {
                            let _ = tx.send(GhostStreamChunk::Done).await;
                            break;
                        }
                        devit_lmstudio::StreamChunk::Error(e) => {
                            let _ = tx.send(GhostStreamChunk::Error(e)).await;
                            break;
                        }
                    }
                }
            });
        }
        "llamacpp" => {
            let backend = LlamaCppBackend::new(
                format!("{}/v1", llamacpp_url.trim_end_matches('/')),
                model_name,
            );
            let mut stream = backend
                .chat_stream(chat_request)
                .await
                .map_err(|e| e.to_string())?;
            let tx = tx.clone();
            tokio::spawn(async move {
                while let Some(chunk) = stream.recv().await {
                    match chunk {
                        devit_llama_cpp::StreamChunk::Delta(s) => {
                            let _ = tx.send(GhostStreamChunk::Delta(s)).await;
                        }
                        devit_llama_cpp::StreamChunk::Thinking(_) => {}
                        devit_llama_cpp::StreamChunk::Done(_) => {
                            let _ = tx.send(GhostStreamChunk::Done).await;
                            break;
                        }
                        devit_llama_cpp::StreamChunk::Error(e) => {
                            let _ = tx.send(GhostStreamChunk::Error(e)).await;
                            break;
                        }
                    }
                }
            });
        }
        _ => {
            // Default to Ollama
            let backend = OllamaBackend::new(ollama_url, model_name);
            let mut stream = backend
                .chat_stream(chat_request)
                .await
                .map_err(|e| e.to_string())?;
            let tx = tx.clone();
            tokio::spawn(async move {
                while let Some(chunk) = stream.recv().await {
                    match chunk {
                        devit_ollama::StreamChunk::Delta(s) => {
                            let _ = tx.send(GhostStreamChunk::Delta(s)).await;
                        }
                        devit_ollama::StreamChunk::Thinking(_) => {}
                        devit_ollama::StreamChunk::Done(_) => {
                            let _ = tx.send(GhostStreamChunk::Done).await;
                            break;
                        }
                        devit_ollama::StreamChunk::Error(e) => {
                            let _ = tx.send(GhostStreamChunk::Error(e)).await;
                            break;
                        }
                    }
                }
            });
        }
    }

    let sid = session_id.clone();
    let start_pos = request.position.clone();
    let state_clone = state.inner().clone();

    tokio::spawn(async move {
        let mut pos = start_pos;
        let mut pending = String::new();

        loop {
            match rx.recv().await {
                Some(GhostStreamChunk::Delta(text)) => {
                    // Update position for each character
                    for c in text.chars() {
                        if c == '\n' {
                            pos.line += 1;
                            pos.column = 0;
                        } else {
                            pos.column += 1;
                        }
                    }
                    pending.push_str(&text);

                    // Update session state
                    {
                        let mut st = state_clone.write();
                        if let Some(session) = st.ghost_sessions.get_mut(&sid) {
                            session.pending_text = pending.clone();
                            session.current_line = pos.line;
                            session.current_column = pos.column;
                        }
                    }

                    let _ = window.emit(
                        "ghost:update",
                        GhostUpdate {
                            session_id: sid.clone(),
                            position: pos.clone(),
                            delta: text,
                            pending_text: pending.clone(),
                            done: false,
                        },
                    );
                }
                Some(GhostStreamChunk::Done) | None => {
                    // Stream complete
                    let _ = window.emit(
                        "ghost:update",
                        GhostUpdate {
                            session_id: sid.clone(),
                            position: pos,
                            delta: String::new(),
                            pending_text: pending,
                            done: true,
                        },
                    );
                    break;
                }
                Some(GhostStreamChunk::Error(err)) => {
                    // Emit error and stop
                    let _ = window.emit(
                        "ghost:update",
                        GhostUpdate {
                            session_id: sid.clone(),
                            position: pos,
                            delta: format!("Error: {}", err),
                            pending_text: pending,
                            done: true,
                        },
                    );
                    break;
                }
            }
        }
    });

    Ok(session_id)
}

/// Accept ghost edit - insert pending text permanently
#[tauri::command]
pub async fn accept_ghost(
    session_id: String,
    state: State<'_, Arc<RwLock<AppState>>>,
) -> Result<String, String> {
    let mut st = state.write();

    let session = st
        .ghost_sessions
        .remove(&session_id)
        .ok_or("Session not found")?;

    // Return the text to be inserted
    Ok(session.pending_text)
}

/// Reject ghost edit - discard pending text
#[tauri::command]
pub async fn reject_ghost(
    session_id: String,
    state: State<'_, Arc<RwLock<AppState>>>,
) -> Result<(), String> {
    let mut st = state.write();
    st.ghost_sessions.remove(&session_id);
    Ok(())
}

/// Get current ghost state for a session
#[tauri::command]
pub async fn get_ghost_state(
    session_id: String,
    state: State<'_, Arc<RwLock<AppState>>>,
) -> Result<Option<GhostStateResponse>, String> {
    let st = state.read();

    Ok(st
        .ghost_sessions
        .get(&session_id)
        .map(|s| GhostStateResponse {
            session_id: s.id.clone(),
            state: format!("{:?}", s.state),
            pending_text: s.pending_text.clone(),
            start_position: Position {
                line: s.start_line,
                column: s.start_column,
            },
            current_position: Position {
                line: s.current_line,
                column: s.current_column,
            },
        }))
}
