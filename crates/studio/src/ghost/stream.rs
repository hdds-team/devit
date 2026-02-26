// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2025-2026 naskel.com

//! Ghost stream handler - bridges LLM streaming to ghost cursor

use super::GhostSession;
use tokio::sync::mpsc;

/// Handles streaming from LLM to ghost cursor
pub struct GhostStreamHandler {
    /// Channel to send updates
    tx: mpsc::Sender<GhostStreamEvent>,
}

#[derive(Debug, Clone)]
pub enum GhostStreamEvent {
    /// New text chunk
    Chunk(String),
    /// Stream completed
    Done,
    /// Error occurred
    Error(String),
}

impl GhostStreamHandler {
    pub fn new(tx: mpsc::Sender<GhostStreamEvent>) -> Self {
        Self { tx }
    }

    /// Send a text chunk
    pub async fn send_chunk(&self, text: String) -> Result<(), String> {
        self.tx
            .send(GhostStreamEvent::Chunk(text))
            .await
            .map_err(|e| e.to_string())
    }

    /// Signal completion
    pub async fn done(&self) -> Result<(), String> {
        self.tx
            .send(GhostStreamEvent::Done)
            .await
            .map_err(|e| e.to_string())
    }

    /// Signal error
    pub async fn error(&self, msg: String) -> Result<(), String> {
        self.tx
            .send(GhostStreamEvent::Error(msg))
            .await
            .map_err(|e| e.to_string())
    }
}

/// Create a ghost stream processor
pub fn create_ghost_stream(
    session: &mut GhostSession,
) -> (GhostStreamHandler, mpsc::Receiver<GhostStreamEvent>) {
    let (tx, rx) = mpsc::channel(100);
    (GhostStreamHandler::new(tx), rx)
}
