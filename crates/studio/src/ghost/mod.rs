// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2025-2026 naskel.com

//! Ghost cursor - AI-powered editing with visual streaming
//!
//! The ghost cursor appears as a secondary cursor (different color)
//! that types in real-time as the LLM generates code.

mod cursor;
mod stream;

pub use cursor::{GhostSession, GhostState};
pub use stream::GhostStreamHandler;
