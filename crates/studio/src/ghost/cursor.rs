// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2025-2026 naskel.com

//! Ghost cursor state and management

/// State of a ghost editing session
#[derive(Debug, Clone, PartialEq)]
pub enum GhostState {
    /// Waiting for LLM response
    Pending,
    /// Streaming in progress
    Streaming,
    /// Stream complete, waiting for user accept/reject
    WaitingApproval,
    /// User accepted, edit applied
    Accepted,
    /// User rejected, edit discarded
    Rejected,
    /// Cancelled (user or error)
    Cancelled,
}

/// A ghost editing session
#[derive(Debug, Clone)]
pub struct GhostSession {
    /// Unique session ID
    pub id: String,
    /// File being edited
    pub file_path: String,
    /// Starting position (line)
    pub start_line: u32,
    /// Starting position (column)
    pub start_column: u32,
    /// Current ghost cursor position (line)
    pub current_line: u32,
    /// Current ghost cursor position (column)  
    pub current_column: u32,
    /// Text pending insertion
    pub pending_text: String,
    /// Current state
    pub state: GhostState,
}

impl GhostSession {
    pub fn new(id: String, file_path: String, position: crate::ipc::ghost::Position) -> Self {
        Self {
            id,
            file_path,
            start_line: position.line,
            start_column: position.column,
            current_line: position.line,
            current_column: position.column,
            pending_text: String::new(),
            state: GhostState::Pending,
        }
    }

    /// Append streamed text
    pub fn append(&mut self, text: &str) {
        self.pending_text.push_str(text);

        // Update cursor position
        for c in text.chars() {
            if c == '\n' {
                self.current_line += 1;
                self.current_column = 0;
            } else {
                self.current_column += 1;
            }
        }

        self.state = GhostState::Streaming;
    }

    /// Mark stream as complete
    pub fn complete(&mut self) {
        self.state = GhostState::WaitingApproval;
    }

    /// Accept the edit
    pub fn accept(&mut self) -> String {
        self.state = GhostState::Accepted;
        std::mem::take(&mut self.pending_text)
    }

    /// Reject the edit
    pub fn reject(&mut self) {
        self.state = GhostState::Rejected;
        self.pending_text.clear();
    }
}
