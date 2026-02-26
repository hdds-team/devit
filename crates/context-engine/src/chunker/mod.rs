// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2025-2026 naskel.com

//! Code chunking with tree-sitter AST awareness

mod tree_sitter_chunker;

pub use tree_sitter_chunker::TreeSitterChunker;

use crate::{CodeChunk, Language, Result};
use async_trait::async_trait;
use std::path::Path;

/// Trait for chunking source code
#[async_trait]
pub trait Chunker: Send + Sync {
    /// Chunk a source file into semantic units
    async fn chunk_file(
        &self,
        path: &Path,
        content: &str,
        language: Language,
    ) -> Result<Vec<CodeChunk>>;

    /// Estimate token count for a piece of text
    fn estimate_tokens(&self, text: &str) -> usize;
}
