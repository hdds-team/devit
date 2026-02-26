// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2025-2026 naskel.com

//! Query planning and context retrieval

use crate::{embedder::Embedder, store::VectorStore, ContextChunk, Result};

/// Query planner for retrieving relevant context
pub struct QueryPlanner {
    top_k: usize,
    similarity_threshold: f32,
}

impl QueryPlanner {
    /// Create a new query planner
    pub fn new(top_k: usize, similarity_threshold: f32) -> Self {
        Self {
            top_k,
            similarity_threshold,
        }
    }

    /// Query for relevant context chunks
    ///
    /// Returns chunks sorted by relevance, limited by max_tokens
    pub async fn query(
        &self,
        query: &str,
        max_tokens: usize,
        embedder: &dyn Embedder,
        store: &dyn VectorStore,
    ) -> Result<Vec<ContextChunk>> {
        // Generate query embedding
        let query_embedding = embedder.embed(query).await?;

        // Search for similar chunks
        let mut results = store
            .search(&query_embedding, self.top_k * 2, self.similarity_threshold) // Fetch extra for token budget
            .await?;

        // Sort by score (highest first)
        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Filter to fit within token budget
        let mut selected = Vec::new();
        let mut total_tokens = 0;

        for chunk in results {
            let chunk_tokens = chunk.chunk.token_count;

            if total_tokens + chunk_tokens > max_tokens {
                // Check if we have at least some context
                if selected.is_empty() && chunk_tokens <= max_tokens {
                    // Include at least one chunk if it fits
                    selected.push(chunk);
                }
                break;
            }

            total_tokens += chunk_tokens;
            selected.push(chunk);
        }

        tracing::debug!(
            "Query returned {} chunks ({} tokens) for: {}",
            selected.len(),
            total_tokens,
            truncate_str(query, 50)
        );

        Ok(selected)
    }

    /// Query with file priority
    ///
    /// Prioritizes chunks from specific files, then fills with semantic search
    pub async fn query_with_files(
        &self,
        query: &str,
        priority_files: &[std::path::PathBuf],
        max_tokens: usize,
        embedder: &dyn Embedder,
        store: &dyn VectorStore,
    ) -> Result<Vec<ContextChunk>> {
        let mut selected = Vec::new();
        let mut total_tokens = 0;

        // First, get chunks from priority files
        let file_chunks = store.get_by_files(priority_files).await?;

        for chunk in file_chunks {
            let chunk_tokens = chunk.chunk.token_count;

            if total_tokens + chunk_tokens > max_tokens / 2 {
                // Reserve half the budget for priority files
                break;
            }

            total_tokens += chunk_tokens;
            selected.push(chunk);
        }

        // Then fill remaining budget with semantic search
        let remaining_tokens = max_tokens.saturating_sub(total_tokens);
        if remaining_tokens > 100 {
            let semantic_chunks = self.query(query, remaining_tokens, embedder, store).await?;

            // Add semantic chunks that aren't already selected
            let selected_ids: std::collections::HashSet<String> =
                selected.iter().map(|c| c.chunk.id.clone()).collect();

            for chunk in semantic_chunks {
                if !selected_ids.contains(&chunk.chunk.id) {
                    let chunk_tokens = chunk.chunk.token_count;
                    if total_tokens + chunk_tokens <= max_tokens {
                        total_tokens += chunk_tokens;
                        selected.push(chunk);
                    }
                }
            }
        }

        Ok(selected)
    }

    /// Format context chunks for LLM prompt
    pub fn format_context(chunks: &[ContextChunk]) -> String {
        if chunks.is_empty() {
            return String::new();
        }

        let mut output = String::from("## Relevant Code Context\n\n");

        // Group by file
        let mut by_file: std::collections::HashMap<&std::path::Path, Vec<&ContextChunk>> =
            std::collections::HashMap::new();

        for chunk in chunks {
            by_file
                .entry(chunk.chunk.file_path.as_path())
                .or_default()
                .push(chunk);
        }

        for (file_path, file_chunks) in by_file {
            output.push_str(&format!("### {}\n\n", file_path.display()));

            // Sort chunks by start line
            let mut sorted_chunks = file_chunks;
            sorted_chunks.sort_by_key(|c| c.chunk.start_line);

            for chunk in sorted_chunks {
                let symbol_info = chunk
                    .chunk
                    .symbol_name
                    .as_ref()
                    .map(|s| format!(" ({})", s))
                    .unwrap_or_default();

                output.push_str(&format!(
                    "Lines {}-{}{} [{}]:\n```{}\n{}\n```\n\n",
                    chunk.chunk.start_line,
                    chunk.chunk.end_line,
                    symbol_info,
                    chunk.chunk.chunk_type.as_str(),
                    chunk.chunk.language,
                    chunk.chunk.content
                ));
            }
        }

        output
    }
}

/// Truncate a string for logging
fn truncate_str(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate() {
        assert_eq!(truncate_str("hello", 10), "hello");
        assert_eq!(truncate_str("hello world", 5), "hello...");
    }
}
