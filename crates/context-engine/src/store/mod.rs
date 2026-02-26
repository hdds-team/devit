// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2025-2026 naskel.com

//! Vector store for code chunk embeddings

mod sqlite;

pub use sqlite::SqliteStore;

use crate::{ContextChunk, EmbeddedChunk, Result};
use async_trait::async_trait;
use std::path::Path;

/// Trait for vector storage and retrieval
#[async_trait]
pub trait VectorStore: Send + Sync {
    /// Insert or update chunks in the store
    async fn upsert(&self, chunks: Vec<EmbeddedChunk>) -> Result<usize>;

    /// Search for similar chunks by embedding
    async fn search(
        &self,
        embedding: &[f32],
        top_k: usize,
        threshold: f32,
    ) -> Result<Vec<ContextChunk>>;

    /// Get all chunks for specific files
    async fn get_by_files(&self, paths: &[std::path::PathBuf]) -> Result<Vec<ContextChunk>>;

    /// Delete all chunks for a file
    async fn delete_by_file(&self, path: &Path) -> Result<()>;

    /// Get total number of chunks in store
    async fn count(&self) -> Result<usize>;

    /// Clear all data from the store
    async fn clear(&self) -> Result<()>;
}
