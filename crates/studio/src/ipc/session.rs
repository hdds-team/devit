// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2025-2026 naskel.com

//! Session IPC commands for devit-studio
//!
//! Provides session persistence using the shared session module from devit-common.

use crate::state::AppState;
use devit_common::session::{
    get_latest_session, get_session_by_id, list_sessions, Session, StoredMessage,
};
use parking_lot::RwLock;
use std::sync::Arc;
use tauri::State;

/// Message from frontend for session storage
#[derive(serde::Deserialize, serde::Serialize, Clone)]
pub struct FrontendMessage {
    pub role: String, // "user" | "assistant" | "system" | "tool_call" | "tool_result"
    pub content: String,
    #[serde(rename = "toolName", skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
}

/// Session info for frontend
#[derive(serde::Serialize)]
pub struct SessionListItem {
    pub id: String,
    pub display_name: String,
    pub created_at: String,
    pub updated_at: String,
    pub message_count: usize,
}

/// Convert frontend message to StoredMessage
fn to_stored_message(msg: &FrontendMessage) -> Option<StoredMessage> {
    match msg.role.as_str() {
        "user" => Some(StoredMessage::User(msg.content.clone())),
        "assistant" => Some(StoredMessage::Assistant(msg.content.clone())),
        "system" => Some(StoredMessage::System(msg.content.clone())),
        "tool_call" => Some(StoredMessage::ToolCall {
            name: msg.tool_name.clone().unwrap_or_default(),
            args: msg.content.clone(),
        }),
        "tool_result" => Some(StoredMessage::ToolResult {
            name: msg.tool_name.clone().unwrap_or_default(),
            result: msg.content.clone(),
        }),
        _ => None,
    }
}

/// Convert StoredMessage to frontend format
fn to_frontend_message(msg: &StoredMessage) -> FrontendMessage {
    match msg {
        StoredMessage::User(s) => FrontendMessage {
            role: "user".to_string(),
            content: s.clone(),
            tool_name: None,
        },
        StoredMessage::Assistant(s) => FrontendMessage {
            role: "assistant".to_string(),
            content: s.clone(),
            tool_name: None,
        },
        StoredMessage::System(s) => FrontendMessage {
            role: "system".to_string(),
            content: s.clone(),
            tool_name: None,
        },
        StoredMessage::ToolCall { name, args } => FrontendMessage {
            role: "tool_call".to_string(),
            content: args.clone(),
            tool_name: Some(name.clone()),
        },
        StoredMessage::ToolResult { name, result } => FrontendMessage {
            role: "tool_result".to_string(),
            content: result.clone(),
            tool_name: Some(name.clone()),
        },
    }
}

/// Save current chat session
#[tauri::command]
pub async fn save_chat_session(
    messages: Vec<FrontendMessage>,
    state: State<'_, Arc<RwLock<AppState>>>,
) -> Result<String, String> {
    let (model, workspace) = {
        let st = state.read();
        let model = st
            .current_provider
            .clone()
            .unwrap_or_else(|| "ollama".into());
        let workspace = st
            .workspace
            .as_ref()
            .map(|w| w.root.to_string_lossy().to_string())
            .unwrap_or_else(|| ".".into());
        (model, workspace)
    };

    let mut session = Session::new_with_app(&model, &workspace, "studio");

    for msg in &messages {
        if let Some(stored) = to_stored_message(msg) {
            session.add_message(stored);
        }
    }

    session.save().map_err(|e| e.to_string())?;

    Ok(session.id.clone())
}

/// Load a chat session by ID or path
#[tauri::command]
pub async fn load_chat_session(
    session_id: Option<String>,
) -> Result<Option<Vec<FrontendMessage>>, String> {
    // If ID provided, load that specific session; otherwise load latest
    let session = if let Some(id) = session_id {
        get_session_by_id(&id, Some("studio")).map_err(|e| e.to_string())?
    } else {
        get_latest_session(Some("studio")).map_err(|e| e.to_string())?
    };

    match session {
        Some(s) => {
            let messages: Vec<FrontendMessage> =
                s.messages.iter().map(to_frontend_message).collect();
            Ok(Some(messages))
        }
        None => Ok(None),
    }
}

/// List available chat sessions
#[tauri::command]
pub async fn list_chat_sessions(
    date_filter: Option<String>,
) -> Result<Vec<SessionListItem>, String> {
    let sessions =
        list_sessions(date_filter.as_deref(), Some("studio")).map_err(|e| e.to_string())?;

    let mut result = Vec::new();
    for info in sessions {
        // Load session to get details
        if let Ok(session) = Session::load(&info.path) {
            result.push(SessionListItem {
                id: session.id,
                display_name: info.display_name,
                created_at: session.created_at,
                updated_at: session.updated_at,
                message_count: session.messages.len(),
            });
        }
    }

    Ok(result)
}

/// Get the latest session (for auto-restore)
#[tauri::command]
pub async fn get_latest_chat_session() -> Result<Option<Vec<FrontendMessage>>, String> {
    load_chat_session(None).await
}
