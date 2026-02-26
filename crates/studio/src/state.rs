// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2025-2026 naskel.com

//! Application state management

use crate::ghost::GhostSession;
use crate::ipc::context_manager::{ContextConfig, ContextManager};
use crate::lsp::LspRegistry;
use crate::workspace::Workspace;
use mcp_core::McpTool;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

/// User preferences for devit-studio
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Settings {
    /// Theme: "dark" only for v1
    pub theme: String,
    /// Ghost cursor accept mode: "popup" or "chat"
    pub ghost_accept_mode: GhostAcceptMode,
    /// Default LLM provider
    pub default_provider: String,
    /// Font size for editor
    pub font_size: u32,
    /// Font family for editor
    pub font_family: String,
    /// Font size for chat panel
    #[serde(default = "default_chat_font_size")]
    pub chat_font_size: u32,
    /// Font family for chat panel
    #[serde(default = "default_chat_font_family")]
    pub chat_font_family: String,
    /// Last opened workspace path (persisted)
    #[serde(default)]
    pub last_workspace: Option<String>,
    /// Default LLM model name (persisted)
    #[serde(default)]
    pub default_model: Option<String>,
    /// Custom system prompt (None = use default)
    #[serde(default)]
    pub system_prompt: Option<String>,
    /// Ollama server URL
    #[serde(default = "default_ollama_url")]
    pub ollama_url: String,
    /// llama.cpp server URL
    #[serde(default = "default_llamacpp_url")]
    pub llamacpp_url: String,
    /// LM Studio server URL
    #[serde(default = "default_lmstudio_url")]
    pub lmstudio_url: String,
}

fn default_ollama_url() -> String {
    std::env::var("OLLAMA_HOST")
        .or_else(|_| std::env::var("DEVIT_OLLAMA_URL"))
        .unwrap_or_else(|_| "http://127.0.0.1:11434".into())
}

fn default_llamacpp_url() -> String {
    std::env::var("DEVIT_LLAMACPP_URL").unwrap_or_else(|_| "http://127.0.0.1:8000".into())
}

fn default_lmstudio_url() -> String {
    std::env::var("DEVIT_LMSTUDIO_URL").unwrap_or_else(|_| "http://127.0.0.1:1234".into())
}

fn default_chat_font_size() -> u32 {
    14
}

fn default_chat_font_family() -> String {
    "JetBrains Mono, monospace".into()
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GhostAcceptMode {
    /// Show popup near cursor
    Popup,
    /// Show accept/reject in chat panel
    Chat,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: "dark".into(),
            ghost_accept_mode: GhostAcceptMode::Popup,
            default_provider: "ollama".into(),
            font_size: 14,
            font_family: "JetBrains Mono, monospace".into(),
            chat_font_size: 14,
            chat_font_family: "JetBrains Mono, monospace".into(),
            last_workspace: None,
            default_model: None,
            system_prompt: None,
            ollama_url: default_ollama_url(),
            llamacpp_url: default_llamacpp_url(),
            lmstudio_url: default_lmstudio_url(),
        }
    }
}

/// Global application state
#[derive(Default)]
pub struct AppState {
    /// Current workspace (open folder)
    pub workspace: Option<Workspace>,
    /// Open files (path -> content cache)
    pub open_files: HashMap<PathBuf, OpenFile>,
    /// Active ghost editing sessions
    pub ghost_sessions: HashMap<String, GhostSession>,
    /// LSP clients registry
    pub lsp_registry: LspRegistry,
    /// User settings
    pub settings: Settings,
    /// Current LLM provider ID
    pub current_provider: Option<String>,
    /// Current LLM model name
    pub current_model: Option<String>,
}

/// Represents an open file in the editor
#[derive(Debug, Clone)]
pub struct OpenFile {
    pub path: PathBuf,
    pub content: String,
    pub modified: bool,
    pub language: Option<String>,
}

/// MCP Tools state - managed separately for async initialization
pub struct ToolState {
    pub tools: Vec<Arc<dyn McpTool>>,
    pub workspace_path: PathBuf,
}

impl ToolState {
    pub fn new(tools: Vec<Arc<dyn McpTool>>, workspace_path: PathBuf) -> Self {
        Self {
            tools,
            workspace_path,
        }
    }

    /// Update workspace path and reload tools
    pub async fn set_workspace(&mut self, path: PathBuf) -> Result<(), String> {
        self.workspace_path = path.clone();
        self.tools = mcp_tools::default_tools(path)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}

/// Chat context state - manages context window compression
pub struct ChatState {
    pub context_manager: ContextManager,
}

impl ChatState {
    pub fn new(max_context_tokens: usize) -> Self {
        Self {
            context_manager: ContextManager::new(ContextConfig::for_context_size(
                max_context_tokens,
            )),
        }
    }

    pub fn with_default_context() -> Self {
        Self::new(8192) // Default 8k context
    }
}

impl Default for ChatState {
    fn default() -> Self {
        Self::with_default_context()
    }
}
