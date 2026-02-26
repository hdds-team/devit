// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2025-2026 naskel.com

//! Terminal IPC commands - PTY-based terminal emulation

use crate::state::AppState;
use parking_lot::RwLock;
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::Arc;
use tauri::{Emitter, State, Window};
use tokio::sync::mpsc;

/// Payload for terminal output events
#[derive(serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TerminalOutputPayload {
    pub session_id: String,
    pub data: String,
}

/// Terminal session info (stored in HashMap)
struct TerminalHandle {
    kill_tx: mpsc::Sender<()>,
    write_tx: mpsc::Sender<Vec<u8>>,
}

/// Global terminal sessions
static TERMINAL_SESSIONS: std::sync::OnceLock<parking_lot::Mutex<HashMap<String, TerminalHandle>>> =
    std::sync::OnceLock::new();

fn get_sessions() -> &'static parking_lot::Mutex<HashMap<String, TerminalHandle>> {
    TERMINAL_SESSIONS.get_or_init(|| parking_lot::Mutex::new(HashMap::new()))
}

/// Spawn a new terminal session
#[tauri::command]
pub async fn spawn_terminal(
    cwd: Option<String>,
    window: Window,
    _state: State<'_, Arc<RwLock<AppState>>>,
) -> Result<String, String> {
    let session_id = uuid::Uuid::new_v4().to_string();

    // Create channels for communication
    let (kill_tx, mut kill_rx) = mpsc::channel::<()>(1);
    let (write_tx, mut write_rx) = mpsc::channel::<Vec<u8>>(256);

    // Store handle immediately
    {
        let mut sessions = get_sessions().lock();
        sessions.insert(
            session_id.clone(),
            TerminalHandle {
                kill_tx: kill_tx.clone(),
                write_tx: write_tx.clone(),
            },
        );
    }

    let sid = session_id.clone();

    // Spawn PTY in a dedicated thread (portable-pty is not async-safe)
    std::thread::spawn(move || {
        let pty_system = native_pty_system();

        // Create PTY with default size
        let pair = match pty_system.openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        }) {
            Ok(p) => p,
            Err(e) => {
                tracing::error!("Failed to open PTY: {}", e);
                return;
            }
        };

        // Determine shell
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());

        // Build command
        let mut cmd = CommandBuilder::new(&shell);
        cmd.arg("-l"); // Login shell

        if let Some(dir) = cwd {
            cmd.cwd(dir);
        } else if let Ok(home) = std::env::var("HOME") {
            cmd.cwd(home);
        }

        // Spawn child process
        let mut child = match pair.slave.spawn_command(cmd) {
            Ok(c) => c,
            Err(e) => {
                tracing::error!("Failed to spawn shell: {}", e);
                return;
            }
        };

        // Get reader/writer
        let mut reader = match pair.master.try_clone_reader() {
            Ok(r) => r,
            Err(e) => {
                tracing::error!("Failed to clone reader: {}", e);
                return;
            }
        };

        let mut writer = match pair.master.take_writer() {
            Ok(w) => w,
            Err(e) => {
                tracing::error!("Failed to take writer: {}", e);
                return;
            }
        };

        // Spawn writer thread
        let writer_thread = std::thread::spawn(move || {
            // Use a blocking receiver pattern
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("failed to create tokio runtime for terminal writer");

            rt.block_on(async {
                loop {
                    tokio::select! {
                        _ = kill_rx.recv() => {
                            break;
                        }
                        Some(data) = write_rx.recv() => {
                            if writer.write_all(&data).is_err() {
                                break;
                            }
                            let _ = writer.flush();
                        }
                    }
                }
            });
        });

        // Read PTY output in main thread
        let mut buffer = [0u8; 4096];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break, // EOF
                Ok(n) => {
                    let data = String::from_utf8_lossy(&buffer[..n]).to_string();
                    let _ = window.emit(
                        "terminal:output",
                        TerminalOutputPayload {
                            session_id: sid.clone(),
                            data,
                        },
                    );
                }
                Err(e) => {
                    tracing::debug!("PTY read error: {}", e);
                    break;
                }
            }
        }

        // Cleanup
        let _ = child.kill();
        drop(writer_thread);

        // Remove session
        let mut sessions = get_sessions().lock();
        sessions.remove(&sid);

        tracing::info!("Terminal session {} ended", sid);
    });

    Ok(session_id)
}

/// Write data to terminal stdin
#[tauri::command]
pub async fn write_terminal(
    session_id: String,
    data: String,
    _state: State<'_, Arc<RwLock<AppState>>>,
) -> Result<(), String> {
    let write_tx = {
        let sessions = get_sessions().lock();
        sessions
            .get(&session_id)
            .map(|h| h.write_tx.clone())
            .ok_or_else(|| "Terminal session not found".to_string())?
    };

    write_tx
        .send(data.into_bytes())
        .await
        .map_err(|_| "Failed to send to terminal".to_string())?;

    Ok(())
}

/// Resize terminal
#[tauri::command]
pub async fn resize_terminal(
    session_id: String,
    cols: u16,
    rows: u16,
    _state: State<'_, Arc<RwLock<AppState>>>,
) -> Result<(), String> {
    // Note: resize requires master handle - would need refactor to support
    tracing::debug!("Resize terminal {} to {}x{}", session_id, cols, rows);
    Ok(())
}

/// Kill terminal session
#[tauri::command]
pub async fn kill_terminal(
    session_id: String,
    _state: State<'_, Arc<RwLock<AppState>>>,
) -> Result<(), String> {
    let kill_tx = {
        let sessions = get_sessions().lock();
        sessions.get(&session_id).map(|h| h.kill_tx.clone())
    };

    if let Some(tx) = kill_tx {
        let _ = tx.send(()).await;
        tracing::info!("Sent kill signal to terminal: {}", session_id);
    }

    Ok(())
}

/// List active terminal sessions
#[tauri::command]
pub async fn list_terminals(
    _state: State<'_, Arc<RwLock<AppState>>>,
) -> Result<Vec<String>, String> {
    let sessions = get_sessions().lock();
    Ok(sessions.keys().cloned().collect())
}
