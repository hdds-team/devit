// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2025-2026 naskel.com

//! Settings IPC commands

use crate::state::{AppState, Settings};
use parking_lot::RwLock;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::State;

/// Get the settings file path
fn settings_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("devit-studio").join("settings.json"))
}

/// Load settings from disk
pub fn load_settings() -> Settings {
    if let Some(path) = settings_path() {
        if path.exists() {
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Ok(settings) = serde_json::from_str(&content) {
                    tracing::info!("Loaded settings from {:?}", path);
                    return settings;
                }
            }
        }
    }
    tracing::info!("Using default settings");
    Settings::default()
}

/// Save settings to disk
pub fn save_settings(settings: &Settings) -> Result<(), String> {
    let path = settings_path().ok_or("Could not determine config directory")?;

    // Create parent directory if needed
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create config directory: {}", e))?;
    }

    let content = serde_json::to_string_pretty(settings)
        .map_err(|e| format!("Failed to serialize settings: {}", e))?;

    std::fs::write(&path, content).map_err(|e| format!("Failed to write settings: {}", e))?;

    tracing::info!("Saved settings to {:?}", path);
    Ok(())
}

/// Get current settings
#[tauri::command]
pub async fn get_settings(state: State<'_, Arc<RwLock<AppState>>>) -> Result<Settings, String> {
    let st = state.read();
    Ok(st.settings.clone())
}

/// Update settings
#[tauri::command]
pub async fn set_settings(
    settings: Settings,
    state: State<'_, Arc<RwLock<AppState>>>,
) -> Result<(), String> {
    // Save to disk first
    save_settings(&settings)?;

    // Update in-memory state
    let mut st = state.write();
    st.settings = settings;

    Ok(())
}
