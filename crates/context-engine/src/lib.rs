// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2025-2026 naskel.com

//! Context Engine - Multi-file context, RAG/embeddings, and diff management
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────┐     ┌──────────────┐     ┌─────────────┐
//! │   Chunker   │────▶│   Embedder   │────▶│    Store    │
//! │ (tree-sitter)│     │   (Ollama)   │     │  (SQLite)   │
//! └─────────────┘     └──────────────┘     └─────────────┘
//!        │                                        │
//!        ▼                                        ▼
//! ┌─────────────┐                         ┌─────────────┐
//! │   Indexer   │◀───────────────────────▶│    Query    │
//! │  (watcher)  │                         │  (planner)  │
//! └─────────────┘                         └─────────────┘
//! ```

pub mod chunker;
pub mod diff;
pub mod embedder;
pub mod indexer;
pub mod query;
pub mod store;
pub mod types;

pub use chunker::Chunker;
pub use diff::{DiffApplier, DiffLine, DiffTag, EditHunk, HunkStatus};
pub use embedder::Embedder;
pub use indexer::Indexer;
pub use query::QueryPlanner;
pub use store::VectorStore;
pub use types::*;

// Re-export CancellationToken for consumers
pub use tokio_util::sync::CancellationToken;

use std::path::PathBuf;
use thiserror::Error;

/// Context Engine errors
#[derive(Error, Debug)]
pub enum ContextError {
    #[error("Chunker error: {0}")]
    Chunker(String),

    #[error("Embedder error: {0}")]
    Embedder(String),

    #[error("Store error: {0}")]
    Store(String),

    #[error("Index error: {0}")]
    Index(String),

    #[error("Query error: {0}")]
    Query(String),

    #[error("Diff error: {0}")]
    Diff(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("Operation cancelled")]
    Cancelled,
}

pub type Result<T> = std::result::Result<T, ContextError>;

/// Configuration for the Context Engine
#[derive(Debug, Clone)]
pub struct ContextEngineConfig {
    /// Workspace root path
    pub workspace_root: PathBuf,

    /// Path to store embeddings (relative to workspace)
    pub store_path: PathBuf,

    /// Ollama API URL for embeddings
    pub ollama_url: String,

    /// Embedding model name
    pub embedding_model: String,

    /// Maximum chunk size in tokens
    pub max_chunk_tokens: usize,

    /// Overlap between chunks in tokens
    pub chunk_overlap: usize,

    /// Number of results for similarity search
    pub top_k: usize,

    /// Similarity threshold (0.0 - 1.0)
    pub similarity_threshold: f32,

    /// File patterns to include
    pub include_patterns: Vec<String>,

    /// File patterns to exclude
    pub exclude_patterns: Vec<String>,
}

impl Default for ContextEngineConfig {
    fn default() -> Self {
        Self {
            workspace_root: PathBuf::from("."),
            store_path: PathBuf::from(".devit/embeddings"),
            ollama_url: "http://localhost:11434".to_string(),
            embedding_model: "nomic-embed-text".to_string(),
            max_chunk_tokens: 512,
            chunk_overlap: 64,
            top_k: 10,
            similarity_threshold: 0.7,
            include_patterns: vec![
                "**/*.rs".to_string(),
                "**/*.py".to_string(),
                "**/*.c".to_string(),
                "**/*.cpp".to_string(),
                "**/*.h".to_string(),
                "**/*.js".to_string(),
                "**/*.ts".to_string(),
            ],
            exclude_patterns: vec![
                // Rust
                "**/target/**".to_string(),
                // Node.js
                "**/node_modules/**".to_string(),
                // Git
                "**/.git/**".to_string(),
                // Build outputs
                "**/dist/**".to_string(),
                "**/build/**".to_string(),
                "**/out/**".to_string(),
                "**/bin/**".to_string(),
                "**/obj/**".to_string(),
                // Python
                "**/__pycache__/**".to_string(),
                "**/.venv/**".to_string(),
                "**/venv/**".to_string(),
                "**/*.egg-info/**".to_string(),
                // CMake
                "**/CMakeFiles/**".to_string(),
                "**/cmake-build-*/**".to_string(),
                // IDE/Editor
                "**/.idea/**".to_string(),
                "**/.vscode/**".to_string(),
                "**/.vs/**".to_string(),
                // Dependencies/externals (common names)
                "**/vendor/**".to_string(),
                "**/third_party/**".to_string(),
                "**/external/**".to_string(),
                "**/externals/**".to_string(),
                "**/deps/**".to_string(),
                // Tests data / fixtures
                "**/testdata/**".to_string(),
                "**/fixtures/**".to_string(),
                // Generated files
                "**/*.min.js".to_string(),
                "**/*.min.css".to_string(),
                "**/*.bundle.js".to_string(),
            ],
        }
    }
}

/// Main Context Engine instance
pub struct ContextEngine {
    config: ContextEngineConfig,
    chunker: Box<dyn Chunker + Send + Sync>,
    embedder: Box<dyn Embedder + Send + Sync>,
    store: Box<dyn VectorStore + Send + Sync>,
    indexer: Indexer,
}

impl ContextEngine {
    /// Create a new Context Engine with default implementations
    pub async fn new(config: ContextEngineConfig) -> Result<Self> {
        let chunker = Box::new(chunker::TreeSitterChunker::new(
            config.max_chunk_tokens,
            config.chunk_overlap,
        ));
        let embedder = Box::new(embedder::OllamaEmbedder::new(
            &config.ollama_url,
            &config.embedding_model,
        ));

        let store_path = config
            .workspace_root
            .join(&config.store_path)
            .join("context.db");
        let store = Box::new(store::SqliteStore::new(&store_path).await?);

        let indexer = Indexer::new(
            config.workspace_root.clone(),
            config.include_patterns.clone(),
            config.exclude_patterns.clone(),
        );

        Ok(Self {
            config,
            chunker,
            embedder,
            store,
            indexer,
        })
    }

    /// Index the entire workspace
    ///
    /// Pass a `CancellationToken` to allow aborting the operation gracefully.
    pub async fn index_workspace(
        &self,
        progress: impl Fn(IndexProgress),
        cancel_token: CancellationToken,
    ) -> Result<IndexStats> {
        self.indexer
            .index_all(
                &*self.chunker,
                &*self.embedder,
                &*self.store,
                progress,
                cancel_token,
            )
            .await
    }

    /// Index a single file (for incremental updates)
    pub async fn index_file(&self, path: &std::path::Path) -> Result<()> {
        self.indexer
            .index_file(path, &*self.chunker, &*self.embedder, &*self.store)
            .await
    }

    /// Invalidate a file (remove from index)
    pub async fn invalidate_file(&self, path: &std::path::Path) -> Result<()> {
        self.store.delete_by_file(path).await
    }

    /// Query for relevant context
    pub async fn query(&self, query: &str, max_tokens: usize) -> Result<Vec<ContextChunk>> {
        let planner = QueryPlanner::new(self.config.top_k, self.config.similarity_threshold);
        planner
            .query(query, max_tokens, &*self.embedder, &*self.store)
            .await
    }

    /// Get context for specific files (multi-file context)
    pub async fn get_file_context(&self, paths: &[PathBuf]) -> Result<Vec<ContextChunk>> {
        self.store.get_by_files(paths).await
    }
}

/// Progress reporting for indexing
#[derive(Debug, Clone)]
pub struct IndexProgress {
    pub current_file: String,
    pub files_done: usize,
    pub files_total: usize,
    pub chunks_created: usize,
}

/// Statistics after indexing
#[derive(Debug, Clone, Default)]
pub struct IndexStats {
    pub files_indexed: usize,
    pub chunks_created: usize,
    pub total_tokens: usize,
    pub duration_ms: u64,
}
