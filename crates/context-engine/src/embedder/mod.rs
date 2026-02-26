// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2025-2026 naskel.com

//! Embedding generation for code chunks

mod ollama;

pub use ollama::OllamaEmbedder;

use crate::Result;
use async_trait::async_trait;

/// Trait for generating embeddings from text
#[async_trait]
pub trait Embedder: Send + Sync {
    /// Generate embedding for a single text
    async fn embed(&self, text: &str) -> Result<Vec<f32>>;

    /// Generate embeddings for multiple texts (batch)
    async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>>;

    /// Get the embedding dimension
    fn dimension(&self) -> usize;
}
