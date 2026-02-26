// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2025-2026 naskel.com

//! devit-studio - Local-first IDE with integrated LLM

#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod ghost;
mod ipc;
mod lsp;
mod state;
mod workspace;

use ipc::context::ContextEngineState;
use ipc::lsp::LspState;
use ipc::settings::load_settings;
use ipc::structured_edit::EditSessionState;
use lsp::LspRegistry;
use parking_lot::RwLock;
use state::{AppState, ToolState};
use std::collections::HashMap;
use std::sync::Arc;
use tauri::Manager;

/// Type alias for tool state management
pub type ManagedToolState = Arc<tokio::sync::RwLock<ToolState>>;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter("devit_studio=info,context_engine=info,tower_lsp=warn")
        .init();

    // Load settings from disk or use defaults
    let settings = load_settings();

    // Determine initial workspace path:
    // 1. Use last_workspace from settings if it exists and is valid
    // 2. Fallback to home directory
    let initial_workspace = settings
        .last_workspace
        .as_ref()
        .map(std::path::PathBuf::from)
        .filter(|p| p.is_dir())
        .or_else(|| dirs::home_dir())
        .unwrap_or_else(|| {
            std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
        });

    tracing::info!("Initial workspace: {:?}", initial_workspace);

    let mut app_state = AppState::default();
    // Initialize current_model from persisted settings
    app_state.current_model = settings.default_model.clone();
    app_state.current_provider = Some(settings.default_provider.clone());
    app_state.settings = settings;
    // Initialize workspace if we have a valid path
    app_state.workspace = Some(workspace::Workspace::new(initial_workspace.clone()));

    let state = Arc::new(RwLock::new(app_state));
    let lsp_state: LspState = Arc::new(tokio::sync::RwLock::new(LspRegistry::new()));

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .setup(move |app| {
            // Initialize MCP tools with the determined workspace path
            let workspace_path = initial_workspace.clone();

            // Use a blocking runtime to initialize tools synchronously during setup
            let tools = tauri::async_runtime::block_on(async {
                mcp_tools::default_tools(workspace_path.clone())
                    .await
                    .unwrap_or_else(|e| {
                        tracing::error!("Failed to initialize MCP tools: {}", e);
                        vec![]
                    })
            });

            tracing::info!(
                "Loaded {} MCP tools for workspace {:?}",
                tools.len(),
                workspace_path
            );

            let tool_state: ManagedToolState = Arc::new(tokio::sync::RwLock::new(ToolState::new(
                tools,
                workspace_path.clone(),
            )));

            app.manage(tool_state);

            // Initialize context engine state (starts empty, init via IPC)
            let context_state: ContextEngineState = Arc::new(tokio::sync::RwLock::new(None));
            app.manage(context_state);

            // Initialize edit session state
            let edit_session_state: EditSessionState =
                Arc::new(tokio::sync::RwLock::new(HashMap::new()));
            app.manage(edit_session_state);

            // Initialize file watcher with main window
            if let Some(window) = app.get_webview_window("main") {
                ipc::watcher::init_watcher(window);
            }

            // Auto-init context engine for workspace (non-blocking background task)
            let app_handle = app.handle().clone();
            ipc::context::auto_init_context_engine(
                app_handle,
                workspace_path,
                None, // Use default Ollama URL
                None, // Use default embedding model
            );

            Ok(())
        })
        .manage(state)
        .manage(lsp_state)
        .invoke_handler(tauri::generate_handler![
            // Editor
            ipc::editor::open_file,
            ipc::editor::reload_file,
            ipc::editor::read_file_base64,
            ipc::editor::save_file,
            ipc::editor::get_symbols,
            // LLM
            ipc::llm::stream_chat,
            ipc::llm::cancel_stream,
            ipc::llm::list_providers,
            ipc::llm::set_provider,
            ipc::llm::set_model,
            ipc::llm::list_models,
            ipc::llm::get_default_system_prompt,
            ipc::llm::estimate_context_tokens,
            ipc::llm::estimate_context_tokens_v2,
            ipc::llm::get_server_props,
            ipc::llm::compact_context,
            // Ghost cursor
            ipc::ghost::start_ghost_edit,
            ipc::ghost::accept_ghost,
            ipc::ghost::reject_ghost,
            ipc::ghost::get_ghost_state,
            // Workspace
            ipc::workspace::list_files,
            ipc::workspace::open_folder,
            ipc::workspace::get_workspace,
            ipc::workspace::get_git_status,
            ipc::workspace::get_file_diff,
            ipc::workspace::search_in_files,
            // LSP
            ipc::lsp::start_lsp,
            ipc::lsp::stop_lsp,
            ipc::lsp::get_completions,
            ipc::lsp::get_hover,
            ipc::lsp::get_diagnostics,
            // Settings
            ipc::settings::get_settings,
            ipc::settings::set_settings,
            // Session
            ipc::session::save_chat_session,
            ipc::session::load_chat_session,
            ipc::session::list_chat_sessions,
            ipc::session::get_latest_chat_session,
            // Terminal
            ipc::terminal::spawn_terminal,
            ipc::terminal::write_terminal,
            ipc::terminal::resize_terminal,
            ipc::terminal::kill_terminal,
            ipc::terminal::list_terminals,
            // File watcher
            ipc::watcher::watch_file,
            ipc::watcher::unwatch_file,
            ipc::watcher::list_watched_files,
            ipc::watcher::watch_workspace_for_context,
            ipc::watcher::unwatch_workspace_for_context,
            ipc::watcher::is_context_watching_enabled,
            // Context engine (RAG)
            ipc::context::init_context_engine,
            ipc::context::index_workspace,
            ipc::context::reindex_file,
            ipc::context::invalidate_file,
            ipc::context::query_context,
            ipc::context::get_file_context,
            // Structured edits (with hunks)
            ipc::structured_edit::request_structured_edit,
            ipc::structured_edit::update_hunk_status,
            ipc::structured_edit::apply_accepted_hunks,
            ipc::structured_edit::cancel_edit_session,
            ipc::structured_edit::get_edit_session,
        ])
        .run(tauri::generate_context!())
        .expect("error while running devit-studio");
}
