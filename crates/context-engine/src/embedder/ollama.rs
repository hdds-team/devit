// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2025-2026 naskel.com

//! Ollama-based embedder implementation

use super::Embedder;
use crate::{ContextError, Result};
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};

/// Embedder using Ollama's embedding API
pub struct OllamaEmbedder {
    client: Client,
    base_url: String,
    model: String,
    dimension: usize,
}

#[derive(Serialize)]
struct EmbedRequest {
    model: String,
    input: Vec<String>,
}

#[derive(Deserialize)]
struct EmbedResponse {
    embeddings: Vec<Vec<f32>>,
}

impl OllamaEmbedder {
    /// Create a new Ollama embedder
    ///
    /// # Arguments
    /// * `base_url` - Ollama API URL (e.g., "http://localhost:11434")
    /// * `model` - Embedding model name (e.g., "nomic-embed-text", "mxbai-embed-large")
    pub fn new(base_url: &str, model: &str) -> Self {
        // Default dimensions for common models
        let dimension = match model {
            "nomic-embed-text" => 768,
            "mxbai-embed-large" => 1024,
            "all-minilm" => 384,
            "snowflake-arctic-embed" => 1024,
            _ => 768, // Default fallback
        };

        Self {
            client: Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
            model: model.to_string(),
            dimension,
        }
    }

    /// Create embedder with explicit dimension
    pub fn with_dimension(base_url: &str, model: &str, dimension: usize) -> Self {
        Self {
            client: Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
            model: model.to_string(),
            dimension,
        }
    }
}

#[async_trait]
impl Embedder for OllamaEmbedder {
    async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let embeddings = self.embed_batch(&[text.to_string()]).await?;
        embeddings
            .into_iter()
            .next()
            .ok_or_else(|| ContextError::Embedder("Empty embedding response".to_string()))
    }

    async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(vec![]);
        }

        let url = format!("{}/api/embed", self.base_url);

        let request = EmbedRequest {
            model: self.model.clone(),
            input: texts.to_vec(),
        };

        let response = self
            .client
            .post(&url)
            .json(&request)
            .send()
            .await
            .map_err(|e| ContextError::Embedder(format!("Request failed: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(ContextError::Embedder(format!(
                "Ollama API error ({}): {}",
                status, body
            )));
        }

        let embed_response: EmbedResponse = response
            .json()
            .await
            .map_err(|e| ContextError::Embedder(format!("Failed to parse response: {}", e)))?;

        Ok(embed_response.embeddings)
    }

    fn dimension(&self) -> usize {
        self.dimension
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_embedder_creation() {
        let embedder = OllamaEmbedder::new("http://localhost:11434", "nomic-embed-text");
        assert_eq!(embedder.dimension(), 768);
    }

    #[test]
    fn test_dimension_detection() {
        assert_eq!(
            OllamaEmbedder::new("http://localhost:11434", "nomic-embed-text").dimension(),
            768
        );
        assert_eq!(
            OllamaEmbedder::new("http://localhost:11434", "mxbai-embed-large").dimension(),
            1024
        );
        assert_eq!(
            OllamaEmbedder::new("http://localhost:11434", "all-minilm").dimension(),
            384
        );
    }
}
