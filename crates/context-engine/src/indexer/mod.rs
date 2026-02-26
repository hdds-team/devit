// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2025-2026 naskel.com

//! File indexing and incremental updates

use crate::{
    chunker::Chunker, embedder::Embedder, store::VectorStore, ContextError, EmbeddedChunk,
    IndexProgress, IndexStats, Language, Result,
};
use globset::{Glob, GlobSet, GlobSetBuilder};
use std::path::{Path, PathBuf};
use std::time::Instant;
use tokio_util::sync::CancellationToken;
use walkdir::WalkDir;

/// Indexer for traversing and indexing files
pub struct Indexer {
    workspace_root: PathBuf,
    include_globs: GlobSet,
    exclude_globs: GlobSet,
}

impl Indexer {
    /// Create a new indexer
    pub fn new(
        workspace_root: PathBuf,
        include_patterns: Vec<String>,
        exclude_patterns: Vec<String>,
    ) -> Self {
        tracing::info!("Creating indexer for workspace: {:?}", workspace_root);
        tracing::debug!("Include patterns: {:?}", include_patterns);
        tracing::debug!("Exclude patterns: {:?}", exclude_patterns);

        let include_globs = build_globset(&include_patterns);
        let exclude_globs = build_globset(&exclude_patterns);

        tracing::debug!(
            "Built {} include globs, {} exclude globs",
            include_globs.len(),
            exclude_globs.len()
        );

        Self {
            workspace_root,
            include_globs,
            exclude_globs,
        }
    }

    /// Index all files in the workspace
    ///
    /// Pass a `CancellationToken` to allow aborting the operation.
    /// When cancelled, returns `ContextError::Cancelled`.
    pub async fn index_all(
        &self,
        chunker: &dyn Chunker,
        embedder: &dyn Embedder,
        store: &dyn VectorStore,
        progress: impl Fn(IndexProgress),
        cancel_token: CancellationToken,
    ) -> Result<IndexStats> {
        let start = Instant::now();

        // Collect all files to index
        let files: Vec<PathBuf> = self.collect_files();
        let total_files = files.len();

        tracing::info!("Found {} files to index", total_files);

        let mut stats = IndexStats::default();

        for (idx, file_path) in files.iter().enumerate() {
            // Check for cancellation before processing each file
            if cancel_token.is_cancelled() {
                tracing::info!("Indexing cancelled after {} files", idx);
                return Err(ContextError::Cancelled);
            }

            let rel_path = file_path
                .strip_prefix(&self.workspace_root)
                .unwrap_or(file_path);

            progress(IndexProgress {
                current_file: rel_path.to_string_lossy().to_string(),
                files_done: idx,
                files_total: total_files,
                chunks_created: stats.chunks_created,
            });

            match self
                .index_file_internal(file_path, chunker, embedder, store)
                .await
            {
                Ok((chunks, tokens)) => {
                    stats.chunks_created += chunks;
                    stats.total_tokens += tokens;
                    stats.files_indexed += 1;
                }
                Err(e) => {
                    tracing::warn!("Failed to index {}: {}", file_path.display(), e);
                }
            }
        }

        stats.duration_ms = start.elapsed().as_millis() as u64;

        progress(IndexProgress {
            current_file: "Done".to_string(),
            files_done: total_files,
            files_total: total_files,
            chunks_created: stats.chunks_created,
        });

        tracing::info!(
            "Indexing complete: {} files, {} chunks, {} tokens in {}ms",
            stats.files_indexed,
            stats.chunks_created,
            stats.total_tokens,
            stats.duration_ms
        );

        Ok(stats)
    }

    /// Index a single file
    pub async fn index_file(
        &self,
        path: &Path,
        chunker: &dyn Chunker,
        embedder: &dyn Embedder,
        store: &dyn VectorStore,
    ) -> Result<()> {
        // First, remove old chunks for this file
        store.delete_by_file(path).await?;

        // Then index the file
        self.index_file_internal(path, chunker, embedder, store)
            .await?;

        Ok(())
    }

    /// Internal file indexing logic
    async fn index_file_internal(
        &self,
        path: &Path,
        chunker: &dyn Chunker,
        embedder: &dyn Embedder,
        store: &dyn VectorStore,
    ) -> Result<(usize, usize)> {
        // Read file content
        let content = tokio::fs::read_to_string(path).await.map_err(|e| {
            ContextError::Index(format!("Failed to read {}: {}", path.display(), e))
        })?;

        // Detect language
        let language = path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(Language::from_extension)
            .unwrap_or(Language::Unknown);

        // Chunk the file
        let chunks = chunker.chunk_file(path, &content, language).await?;

        if chunks.is_empty() {
            return Ok((0, 0));
        }

        // Calculate total tokens
        let total_tokens: usize = chunks.iter().map(|c| c.token_count).sum();

        // Generate embeddings for all chunks
        let texts: Vec<String> = chunks.iter().map(|c| c.content.clone()).collect();
        let embeddings = embedder.embed_batch(&texts).await?;

        // Combine chunks with embeddings
        let embedded_chunks: Vec<EmbeddedChunk> = chunks
            .into_iter()
            .zip(embeddings.into_iter())
            .map(|(chunk, embedding)| EmbeddedChunk { chunk, embedding })
            .collect();

        let count = embedded_chunks.len();

        // Store in vector DB
        store.upsert(embedded_chunks).await?;

        Ok((count, total_tokens))
    }

    /// Collect all files matching the include/exclude patterns
    fn collect_files(&self) -> Vec<PathBuf> {
        let mut files = Vec::new();
        let mut total_scanned = 0;
        let mut excluded_count = 0;
        let mut not_matched_count = 0;

        tracing::debug!("Scanning workspace: {:?}", self.workspace_root);
        tracing::debug!("Include patterns count: {}", self.include_globs.len());
        tracing::debug!("Exclude patterns count: {}", self.exclude_globs.len());

        for entry in WalkDir::new(&self.workspace_root)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();

            // Skip directories
            if !path.is_file() {
                continue;
            }

            total_scanned += 1;

            let rel_path = path.strip_prefix(&self.workspace_root).unwrap_or(path);

            // Check exclude patterns first
            if self.exclude_globs.is_match(rel_path) {
                excluded_count += 1;
                continue;
            }

            // Check include patterns
            if self.include_globs.is_match(rel_path) {
                files.push(path.to_path_buf());
            } else {
                not_matched_count += 1;
                // Log first few non-matching files for debugging
                if not_matched_count <= 5 {
                    tracing::debug!("File not matching include patterns: {:?}", rel_path);
                }
            }
        }

        tracing::info!(
            "File collection: {} total scanned, {} excluded, {} not matched, {} to index",
            total_scanned,
            excluded_count,
            not_matched_count,
            files.len()
        );

        files
    }
}

/// Build a GlobSet from patterns
fn build_globset(patterns: &[String]) -> GlobSet {
    let mut builder = GlobSetBuilder::new();
    let mut valid_count = 0;

    for pattern in patterns {
        match Glob::new(pattern) {
            Ok(glob) => {
                builder.add(glob);
                valid_count += 1;
            }
            Err(e) => {
                tracing::warn!("Invalid glob pattern '{}': {}", pattern, e);
            }
        }
    }

    match builder.build() {
        Ok(globset) => {
            tracing::debug!("Built globset with {} patterns", valid_count);
            globset
        }
        Err(e) => {
            tracing::error!("Failed to build globset: {}", e);
            GlobSet::empty()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_globset_building() {
        let patterns = vec!["**/*.rs".to_string(), "**/*.py".to_string()];
        let globset = build_globset(&patterns);

        assert!(globset.is_match("src/main.rs"));
        assert!(globset.is_match("lib/utils.py"));
        assert!(!globset.is_match("readme.md"));
    }

    #[test]
    fn test_exclude_patterns() {
        let exclude = vec!["**/target/**".to_string(), "**/node_modules/**".to_string()];
        let globset = build_globset(&exclude);

        assert!(globset.is_match("target/debug/main.rs"));
        assert!(globset.is_match("node_modules/lodash/index.js"));
        assert!(!globset.is_match("src/main.rs"));
    }
}
