// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2025-2026 naskel.com

//! Session persistence for devit applications
//!
//! Sessions are stored in ~/.devit/YYYY-MM-DD/session_HHMMSS.json
//! This module is shared between devit-chat and devit-studio.

use chrono::{Local, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// A serializable message for session storage
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", content = "content")]
pub enum StoredMessage {
    User(String),
    Assistant(String),
    System(String),
    ToolCall { name: String, args: String },
    ToolResult { name: String, result: String },
}

/// Session metadata and messages
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Session {
    /// Session ID (timestamp-based)
    pub id: String,
    /// Creation timestamp (ISO 8601)
    pub created_at: String,
    /// Last update timestamp (ISO 8601)
    pub updated_at: String,
    /// Model used for this session
    pub model: String,
    /// Workspace path
    pub workspace: String,
    /// Application identifier ("chat" or "studio")
    #[serde(default = "default_app")]
    pub app: String,
    /// All messages in the session
    pub messages: Vec<StoredMessage>,
    /// Input history (user's previous inputs)
    #[serde(default)]
    pub input_history: Vec<String>,
}

fn default_app() -> String {
    "chat".to_string()
}

/// Error type for session operations
#[derive(Debug)]
pub enum SessionError {
    Io(std::io::Error),
    Json(serde_json::Error),
    HomeDir,
}

impl std::fmt::Display for SessionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SessionError::Io(e) => write!(f, "IO error: {}", e),
            SessionError::Json(e) => write!(f, "JSON error: {}", e),
            SessionError::HomeDir => write!(f, "Could not determine home directory"),
        }
    }
}

impl std::error::Error for SessionError {}

impl From<std::io::Error> for SessionError {
    fn from(e: std::io::Error) -> Self {
        SessionError::Io(e)
    }
}

impl From<serde_json::Error> for SessionError {
    fn from(e: serde_json::Error) -> Self {
        SessionError::Json(e)
    }
}

impl Session {
    /// Create a new session
    pub fn new(model: &str, workspace: &str) -> Self {
        Self::new_with_app(model, workspace, "chat")
    }

    /// Create a new session with specific app identifier
    pub fn new_with_app(model: &str, workspace: &str, app: &str) -> Self {
        let now = Utc::now();
        let id = now.format("%Y%m%d_%H%M%S").to_string();
        let timestamp = now.to_rfc3339();

        Self {
            id,
            created_at: timestamp.clone(),
            updated_at: timestamp,
            model: model.to_string(),
            workspace: workspace.to_string(),
            app: app.to_string(),
            messages: Vec::new(),
            input_history: Vec::new(),
        }
    }

    /// Get the session file path
    pub fn file_path(&self) -> Result<PathBuf, SessionError> {
        let base = get_sessions_base_dir()?;
        let date = &self.created_at[..10]; // YYYY-MM-DD from ISO timestamp
        let dir = base.join(date);
        fs::create_dir_all(&dir)?;
        Ok(dir.join(format!("session_{}.json", self.id)))
    }

    /// Save session to disk
    pub fn save(&mut self) -> Result<(), SessionError> {
        self.updated_at = Utc::now().to_rfc3339();
        let path = self.file_path()?;
        let content = serde_json::to_string_pretty(self)?;
        fs::write(path, content)?;
        Ok(())
    }

    /// Load session from a file
    pub fn load(path: &PathBuf) -> Result<Self, SessionError> {
        let content = fs::read_to_string(path)?;
        let session: Session = serde_json::from_str(&content)?;
        Ok(session)
    }

    /// Add a message to the session
    pub fn add_message(&mut self, msg: StoredMessage) {
        self.messages.push(msg);
    }

    /// Set the input history
    pub fn set_input_history(&mut self, history: Vec<String>) {
        self.input_history = history;
    }

    /// Clear all messages
    pub fn clear_messages(&mut self) {
        self.messages.clear();
    }
}

/// Get the base directory for sessions (~/.devit)
pub fn get_sessions_base_dir() -> Result<PathBuf, SessionError> {
    let home = dirs::home_dir().ok_or(SessionError::HomeDir)?;
    let base = home.join(".devit");
    fs::create_dir_all(&base)?;
    Ok(base)
}

/// Session info for listing (without loading full content)
#[derive(Clone, Debug)]
pub struct SessionInfo {
    pub path: PathBuf,
    pub display_name: String,
    pub app: Option<String>,
}

/// List all sessions, optionally filtered by date and/or app
/// Check if a session file matches the app filter.
/// Returns (should_include, app_name).
fn matches_app_filter(path: &std::path::Path, app_filter: Option<&str>) -> (bool, Option<String>) {
    let filter = match app_filter {
        None => return (true, None),
        Some(f) => f,
    };
    match fs::read_to_string(path)
        .ok()
        .and_then(|c| serde_json::from_str::<Session>(&c).ok())
    {
        Some(session) if session.app == filter => (true, Some(session.app)),
        Some(_) => (false, None),
        None => (true, None), // Can't read/parse, include anyway
    }
}

pub fn list_sessions(
    date_filter: Option<&str>,
    app_filter: Option<&str>,
) -> Result<Vec<SessionInfo>, SessionError> {
    let base = get_sessions_base_dir()?;
    let mut sessions = Vec::new();

    let entries: Vec<_> = fs::read_dir(&base)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .collect();

    for entry in entries {
        let dir_name = entry.file_name().to_string_lossy().to_string();

        if let Some(filter) = date_filter {
            if !dir_name.starts_with(filter) {
                continue;
            }
        }

        let files = match fs::read_dir(entry.path()) {
            Ok(f) => f,
            Err(_) => continue,
        };

        for file in files.filter_map(|f| f.ok()) {
            let path = file.path();
            if path.extension().map(|e| e == "json").unwrap_or(false) {
                let (include, app) = matches_app_filter(&path, app_filter);
                if !include {
                    continue;
                }

                let name = path
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default();
                sessions.push(SessionInfo {
                    path,
                    display_name: format!("{}/{}", dir_name, name),
                    app,
                });
            }
        }
    }

    sessions.sort_by(|a, b| b.display_name.cmp(&a.display_name));
    Ok(sessions)
}

/// Get the most recent session (for resuming), optionally filtered by app
pub fn get_latest_session(app_filter: Option<&str>) -> Result<Option<Session>, SessionError> {
    let sessions = list_sessions(None, app_filter)?;
    if let Some(info) = sessions.first() {
        Ok(Some(Session::load(&info.path)?))
    } else {
        Ok(None)
    }
}

/// Get a session by ID, optionally filtered by app
pub fn get_session_by_id(
    session_id: &str,
    app_filter: Option<&str>,
) -> Result<Option<Session>, SessionError> {
    let sessions = list_sessions(None, app_filter)?;
    for info in sessions {
        // Check if filename matches session_<id>.json
        if let Some(stem) = info.path.file_stem() {
            let stem_str = stem.to_string_lossy();
            if stem_str == format!("session_{}", session_id) {
                return Ok(Some(Session::load(&info.path)?));
            }
        }
    }
    Ok(None)
}

/// Get today's session directory
pub fn get_today_session_dir() -> Result<PathBuf, SessionError> {
    let base = get_sessions_base_dir()?;
    let today = Local::now().format("%Y-%m-%d").to_string();
    let dir = base.join(today);
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_creation() {
        let session = Session::new("qwen2.5-coder:7b", "/home/user/project");
        assert!(!session.id.is_empty());
        assert_eq!(session.model, "qwen2.5-coder:7b");
        assert_eq!(session.app, "chat");
        assert!(session.messages.is_empty());
    }

    #[test]
    fn test_session_with_app() {
        let session = Session::new_with_app("model", "/workspace", "studio");
        assert_eq!(session.app, "studio");
    }

    #[test]
    fn test_stored_message_serialization() {
        let msg = StoredMessage::User("Hello".to_string());
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("User"));
        assert!(json.contains("Hello"));
    }
}
