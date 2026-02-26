// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2025-2026 naskel.com

//! Structured Edit IPC - Multi-file edits with RAG context and hunk-based review
//!
//! This module handles:
//! 1. Query context engine for relevant code
//! 2. Send request to LLM with context
//! 3. Parse JSON edit response
//! 4. Return hunks for UI review

use crate::ipc::context::ContextEngineState;
use crate::state::AppState;
use context_engine::{query::QueryPlanner, DiffApplier, EditHunk, HunkStatus};
use devit_backend_core::{ChatMessage, ChatRequest};
use devit_llama_cpp::LlamaCppBackend;
use devit_lmstudio::LmstudioBackend;
use devit_ollama::OllamaBackend;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use tauri::{Emitter, State, Window};
use tokio::sync::RwLock as AsyncRwLock;

/// Active edit session with hunks
pub struct EditSession {
    pub id: String,
    pub hunks: Vec<EditHunk>,
    pub original_contents: HashMap<String, String>,
}

/// Managed state for edit sessions
pub type EditSessionState = Arc<AsyncRwLock<HashMap<String, EditSession>>>;

/// Request for structured edit
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StructuredEditRequest {
    /// User instruction
    pub prompt: String,
    /// Optional: specific files to focus on
    pub focus_files: Option<Vec<String>>,
    /// Optional: include file at cursor
    pub current_file: Option<CurrentFileContext>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CurrentFileContext {
    pub path: String,
    pub content: String,
    pub cursor_line: Option<usize>,
}

/// Response with hunks for UI
#[derive(serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct StructuredEditResponse {
    pub session_id: String,
    pub hunks: Vec<HunkPayload>,
}

#[derive(serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct HunkPayload {
    pub id: String,
    pub file_path: String,
    pub start_line: usize,
    pub end_line: usize,
    pub original: String,
    pub replacement: String,
    pub description: String,
    pub status: String,
}

impl From<&EditHunk> for HunkPayload {
    fn from(hunk: &EditHunk) -> Self {
        Self {
            id: hunk.id.clone(),
            file_path: hunk.file_path.clone(),
            start_line: hunk.start_line,
            end_line: hunk.end_line,
            original: hunk.original.clone(),
            replacement: hunk.replacement.clone(),
            description: hunk.description.clone(),
            status: match hunk.status {
                HunkStatus::Pending => "pending",
                HunkStatus::Accepted => "accepted",
                HunkStatus::Rejected => "rejected",
                HunkStatus::Applied => "applied",
            }
            .to_string(),
        }
    }
}

/// System prompt for structured edits
const STRUCTURED_EDIT_SYSTEM_PROMPT: &str = r#"You are a code editing assistant. When the user asks you to modify code, respond ONLY with a JSON object containing the edits.

Response format:
{
  "file": "path/to/file.rs",
  "edits": [
    {
      "op": "replace",
      "start_line": 10,
      "end_line": 15,
      "content": "new code here"
    }
  ]
}

Edit operations:
- "replace": Replace lines start_line through end_line with content
- "insert_after": Insert content after the specified line (use after_line instead of start_line/end_line)
- "delete": Delete lines start_line through end_line (no content field)

Rules:
1. Output ONLY valid JSON, no markdown code blocks
2. Line numbers are 1-indexed
3. Preserve proper indentation in content
4. Make minimal, focused changes
5. If multiple files need changes, output one JSON per file

"#;

/// Request a structured edit with RAG context
#[tauri::command]
pub async fn request_structured_edit(
    request: StructuredEditRequest,
    window: Window,
    app_state: State<'_, Arc<RwLock<AppState>>>,
    context_state: State<'_, ContextEngineState>,
    session_state: State<'_, EditSessionState>,
) -> Result<StructuredEditResponse, String> {
    let session_id = uuid::Uuid::new_v4().to_string();

    // Build context from context engine
    let mut context_text = String::new();

    // Try to get RAG context
    {
        let ctx_guard = context_state.read().await;
        if let Some(ref inner) = *ctx_guard {
            // Query for relevant context
            let chunks = inner
                .engine
                .query(&request.prompt, 4096)
                .await
                .map_err(|e| format!("Context query failed: {}", e))?;

            if !chunks.is_empty() {
                context_text.push_str("## Relevant Code Context\n\n");
                context_text.push_str(&QueryPlanner::format_context(&chunks));
            }
        }
    }

    // Add current file context if provided
    if let Some(ref current) = request.current_file {
        context_text.push_str(&format!(
            "\n## Current File: {}\n```\n{}\n```\n",
            current.path, current.content
        ));
        if let Some(line) = current.cursor_line {
            context_text.push_str(&format!("Cursor at line: {}\n", line));
        }
    }

    // Build system prompt
    let system_prompt = format!("{}\n{}", STRUCTURED_EDIT_SYSTEM_PROMPT, context_text);

    // Create chat messages
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
            content: request.prompt.clone(),
            tool_calls: None,
            tool_name: None,
            images: None,
        },
    ];

    let chat_request = ChatRequest::new(messages);

    // Get provider settings
    let (provider_id, model_name, ollama_url, lmstudio_url, llamacpp_url) = {
        let st = app_state.read();
        (
            st.current_provider
                .clone()
                .unwrap_or_else(|| "ollama".into()),
            st.current_model
                .clone()
                .unwrap_or_else(|| "qwen2.5-coder:7b".into()),
            st.settings.ollama_url.clone(),
            st.settings.lmstudio_url.clone(),
            st.settings.llamacpp_url.clone(),
        )
    };

    // Call LLM (non-streaming for structured output)
    let response_text = call_llm_sync(
        &provider_id,
        &model_name,
        &ollama_url,
        &lmstudio_url,
        &llamacpp_url,
        chat_request,
    )
    .await?;

    // Parse JSON response into hunks
    let mut hunks = DiffApplier::parse_edit(&response_text).map_err(|e| {
        format!(
            "Failed to parse LLM response as JSON edit: {}\n\nRaw response:\n{}",
            e, response_text
        )
    })?;

    // Fill original content for hunks
    let mut original_contents = HashMap::new();
    for hunk in &mut hunks {
        if !original_contents.contains_key(&hunk.file_path) {
            // Try to read file content
            if let Ok(content) = tokio::fs::read_to_string(&hunk.file_path).await {
                original_contents.insert(hunk.file_path.clone(), content);
            }
        }

        if let Some(content) = original_contents.get(&hunk.file_path) {
            let _ = DiffApplier::fill_original_content(std::slice::from_mut(hunk), content);
        }
    }

    // Store session
    let session = EditSession {
        id: session_id.clone(),
        hunks: hunks.clone(),
        original_contents,
    };

    {
        let mut sessions = session_state.write().await;
        sessions.insert(session_id.clone(), session);
    }

    // Build response
    let hunk_payloads: Vec<HunkPayload> = hunks.iter().map(HunkPayload::from).collect();

    Ok(StructuredEditResponse {
        session_id,
        hunks: hunk_payloads,
    })
}

/// Accept or reject a hunk
#[tauri::command]
pub async fn update_hunk_status(
    session_id: String,
    hunk_id: String,
    accepted: bool,
    session_state: State<'_, EditSessionState>,
) -> Result<(), String> {
    let mut sessions = session_state.write().await;
    let session = sessions.get_mut(&session_id).ok_or("Session not found")?;

    let hunk = session
        .hunks
        .iter_mut()
        .find(|h| h.id == hunk_id)
        .ok_or("Hunk not found")?;

    hunk.status = if accepted {
        HunkStatus::Accepted
    } else {
        HunkStatus::Rejected
    };

    Ok(())
}

/// Apply all accepted hunks
#[tauri::command]
pub async fn apply_accepted_hunks(
    session_id: String,
    session_state: State<'_, EditSessionState>,
) -> Result<Vec<String>, String> {
    let mut sessions = session_state.write().await;
    let session = sessions.get_mut(&session_id).ok_or("Session not found")?;

    let mut modified_files = Vec::new();

    // Group hunks by file
    let mut hunks_by_file: HashMap<String, Vec<&EditHunk>> = HashMap::new();
    for hunk in &session.hunks {
        hunks_by_file
            .entry(hunk.file_path.clone())
            .or_default()
            .push(hunk);
    }

    // Apply hunks to each file
    for (file_path, file_hunks) in hunks_by_file {
        let original = session
            .original_contents
            .get(&file_path)
            .ok_or(format!("Original content not found for {}", file_path))?;

        // Convert to owned hunks for apply
        let owned_hunks: Vec<EditHunk> = file_hunks.into_iter().cloned().collect();

        let new_content = DiffApplier::apply_hunks(original, &owned_hunks)
            .map_err(|e| format!("Failed to apply hunks to {}: {}", file_path, e))?;

        // Write file
        tokio::fs::write(&file_path, &new_content)
            .await
            .map_err(|e| format!("Failed to write {}: {}", file_path, e))?;

        modified_files.push(file_path);
    }

    // Mark hunks as applied
    for hunk in &mut session.hunks {
        if hunk.status == HunkStatus::Accepted {
            hunk.status = HunkStatus::Applied;
        }
    }

    Ok(modified_files)
}

/// Cancel/discard an edit session
#[tauri::command]
pub async fn cancel_edit_session(
    session_id: String,
    session_state: State<'_, EditSessionState>,
) -> Result<(), String> {
    let mut sessions = session_state.write().await;
    sessions.remove(&session_id);
    Ok(())
}

/// Get current state of an edit session
#[tauri::command]
pub async fn get_edit_session(
    session_id: String,
    session_state: State<'_, EditSessionState>,
) -> Result<Option<StructuredEditResponse>, String> {
    let sessions = session_state.read().await;

    Ok(sessions
        .get(&session_id)
        .map(|session| StructuredEditResponse {
            session_id: session.id.clone(),
            hunks: session.hunks.iter().map(HunkPayload::from).collect(),
        }))
}

/// Helper: Call LLM synchronously (collect full response)
async fn call_llm_sync(
    provider_id: &str,
    model_name: &str,
    ollama_url: &str,
    lmstudio_url: &str,
    llamacpp_url: &str,
    request: ChatRequest,
) -> Result<String, String> {
    let mut response = String::new();

    match provider_id {
        "lmstudio" => {
            let backend = LmstudioBackend::new(
                format!("{}/v1", lmstudio_url.trim_end_matches('/')),
                model_name.to_string(),
            );
            let mut stream = backend
                .chat_stream(request)
                .await
                .map_err(|e| e.to_string())?;
            while let Some(chunk) = stream.recv().await {
                match chunk {
                    devit_lmstudio::StreamChunk::Delta(s) => response.push_str(&s),
                    devit_lmstudio::StreamChunk::Done(_) => break,
                    devit_lmstudio::StreamChunk::Error(e) => return Err(e),
                    _ => {}
                }
            }
        }
        "llamacpp" => {
            let backend = LlamaCppBackend::new(
                format!("{}/v1", llamacpp_url.trim_end_matches('/')),
                model_name.to_string(),
            );
            let mut stream = backend
                .chat_stream(request)
                .await
                .map_err(|e| e.to_string())?;
            while let Some(chunk) = stream.recv().await {
                match chunk {
                    devit_llama_cpp::StreamChunk::Delta(s) => response.push_str(&s),
                    devit_llama_cpp::StreamChunk::Done(_) => break,
                    devit_llama_cpp::StreamChunk::Error(e) => return Err(e),
                    _ => {}
                }
            }
        }
        _ => {
            let backend = OllamaBackend::new(ollama_url.to_string(), model_name.to_string());
            let mut stream = backend
                .chat_stream(request)
                .await
                .map_err(|e| e.to_string())?;
            while let Some(chunk) = stream.recv().await {
                match chunk {
                    devit_ollama::StreamChunk::Delta(s) => response.push_str(&s),
                    devit_ollama::StreamChunk::Done(_) => break,
                    devit_ollama::StreamChunk::Error(e) => return Err(e),
                    _ => {}
                }
            }
        }
    }

    Ok(response)
}
