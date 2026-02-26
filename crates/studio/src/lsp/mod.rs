// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2025-2026 naskel.com

//! LSP client management
//!
//! Manages multiple LSP servers for different languages.

mod client;
mod registry;

pub use client::get_server_config;
pub use registry::LspRegistry;
