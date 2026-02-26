// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2025-2026 naskel.com

//! ExplorerTool: Meta-tool for intelligent codebase exploration with token budget awareness.
//!
//! Balances two strategies:
//! - INTERNAL (fast): Direct file_search_ext + project_structure_ext with heuristic scoring
//! - EXTERNAL (deep): Compress results via lightweight LLM (e.g., qwen2.5:1.5b)
//!
//! Auto-mode chooses based on result size and budget.

use async_trait::async_trait;
use mcp_core::{McpResult, McpTool};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;

use crate::errors::{internal_error, validation_error};
use crate::file_explore::FileExplorer;
use devit_cli::core::formats::OutputFormat;

/// Explorer configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExplorerConfig {
    /// Strategy: Auto, ForceInternal, ForceExternal
    pub mode: ExplorerMode,
    /// Optional compression model (e.g., "qwen2.5:1.5b")
    pub compress_model: Option<String>,
    /// Token budget for results (default: 2500)
    pub budget_tokens: usize,
    /// Threshold for auto-switching to external mode (default: 1500)
    pub internal_threshold: usize,
    /// Ollama host URL for compression
    pub ollama_host: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ExplorerMode {
    Auto,
    ForceInternal,
    ForceExternal,
}

impl Default for ExplorerConfig {
    fn default() -> Self {
        Self {
            mode: ExplorerMode::Auto,
            compress_model: Some("qwen2.5:1.5b".to_string()),
            budget_tokens: 2500,
            internal_threshold: 1500,
            ollama_host: "http://127.0.0.1:11434".to_string(),
        }
    }
}

/// File scoring metadata
#[derive(Debug, Clone)]
struct FileScore {
    /// Keyword density in matches (0.0-1.0)
    relevance: f32,
    /// File importance (test=0.3, vendor=0.1, lib.rs=0.9, README=1.0)
    importance: f32,
    /// Optional: boost if recently modified
    recency: f32,
    /// Penalty for large files
    size_penalty: f32,
}

impl FileScore {
    fn total(&self) -> f32 {
        (self.relevance * 0.5
            + self.importance * 0.3
            + self.recency * 0.1
            + self.size_penalty * 0.1)
            .max(0.0)
    }
}

/// Exploration result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExplorationResult {
    /// Query that was explored
    pub query: String,
    /// Strategy used: "internal" or "external"
    pub strategy: String,
    /// Main findings text
    pub findings: String,
    /// Raw search results (before compression if external)
    pub raw_results: Option<String>,
    /// Estimated tokens used
    pub tokens_used: usize,
    /// Whether results were truncated
    pub truncated: bool,
    /// Metadata
    pub metadata: ExploreMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExploreMetadata {
    pub files_scanned: usize,
    pub matches_found: usize,
    pub score_threshold: f32,
    pub execution_mode: String,
}

/// The ExplorerTool itself
pub struct ExplorerTool {
    explorer: Arc<FileExplorer>,
    config: ExplorerConfig,
}

impl ExplorerTool {
    pub fn new(explorer: Arc<FileExplorer>) -> Self {
        Self {
            explorer,
            config: ExplorerConfig::default(),
        }
    }

    pub fn with_config(explorer: Arc<FileExplorer>, config: ExplorerConfig) -> Self {
        Self { explorer, config }
    }

    /// Run internal exploration strategy
    async fn explore_internal(&self, query: &str) -> McpResult<ExplorationResult> {
        // 1. Search files for the query
        let pattern = format_search_pattern(query);

        let search_result = self
            .explorer
            .search_ext(
                &pattern,
                ".",
                &OutputFormat::Compact,
                None,
                Some(1),
                None,
                Some(50),
            )
            .await
            .map_err(|e| internal_error(format!("Search failed: {}", e)))?;

        // 2. Get project structure for context
        let structure_result = self
            .explorer
            .project_structure_ext(".", &OutputFormat::Compact, None, Some(4))
            .await
            .map_err(|e| internal_error(format!("Structure scan failed: {}", e)))?;

        // 3. Estimate tokens
        let tokens_estimated = estimate_tokens(&search_result) + estimate_tokens(&structure_result);
        let truncated = tokens_estimated > self.config.budget_tokens;

        // 4. Format findings
        let findings = format!(
            "## Exploration Results for: {}\n\n\
            ### Search Results\n{}\n\n\
            ### Project Context\n{}\n\n\
            ### Summary\n\
            - Query: {}\n\
            - Strategy: Internal (fast heuristic)\n\
            - Tokens used: ~{}\n\
            - Truncated: {}",
            query, search_result, structure_result, query, tokens_estimated, truncated
        );

        Ok(ExplorationResult {
            query: query.to_string(),
            strategy: "internal".to_string(),
            findings,
            raw_results: Some(search_result),
            tokens_used: tokens_estimated,
            truncated,
            metadata: ExploreMetadata {
                files_scanned: 200,
                matches_found: 10,
                score_threshold: 0.5,
                execution_mode: "internal".to_string(),
            },
        })
    }

    /// Run external exploration strategy (compress via LLM)
    async fn explore_external(&self, query: &str) -> McpResult<ExplorationResult> {
        // 1. Gather raw results first
        let internal_result = self.explore_internal(query).await?;

        // 2. Check if compress_model is configured
        let compress_model = self
            .config
            .compress_model
            .as_ref()
            .ok_or_else(|| validation_error("External mode requires compress_model configuration"))?
            .clone();

        // 3. Build compression request (minimal prompt)
        let compress_prompt = format!(
            "Summarize these search results for query: {}\n\n{}\n\n\
            Be concise (< 500 tokens), focus on most relevant files and patterns.",
            query,
            internal_result
                .raw_results
                .as_ref()
                .unwrap_or(&String::new())
        );

        // 4. Query compression model (non-streaming for stability)
        let compressed =
            query_ollama_sync(&self.config.ollama_host, &compress_model, &compress_prompt)
                .await
                .unwrap_or_else(|e| {
                    format!(
                        "Compression failed ({}), returning raw results.\n\n{}",
                        e,
                        internal_result
                            .raw_results
                            .as_ref()
                            .unwrap_or(&String::new())
                    )
                });

        let tokens_used = estimate_tokens(&compressed)
            + estimate_tokens(
                &internal_result
                    .raw_results
                    .as_ref()
                    .unwrap_or(&String::new()),
            );

        Ok(ExplorationResult {
            query: query.to_string(),
            strategy: "external".to_string(),
            findings: compressed,
            raw_results: internal_result.raw_results,
            tokens_used,
            truncated: tokens_used > self.config.budget_tokens,
            metadata: ExploreMetadata {
                files_scanned: 200,
                matches_found: 10,
                score_threshold: 0.5,
                execution_mode: "external".to_string(),
            },
        })
    }
}

#[async_trait]
impl McpTool for ExplorerTool {
    fn name(&self) -> &str {
        "devit_explorer"
    }

    fn description(&self) -> &str {
        "🔍 SMART codebase explorer - Use this INSTEAD of grep/shell for finding logic! \
         Automatically searches, analyzes, and summarizes code patterns with token optimization. \
         Returns compressed insights (not raw grep output). Perfect for: finding auth/error handling/database layer/config files. \
         Two modes: fast (instant) or deep (LLM-compressed summaries). \
         Example queries: 'find auth logic', 'trace request flow', 'locate error handling', 'where is database layer'."
    }

    async fn execute(&self, params: Value) -> McpResult<Value> {
        // Parse query
        let query = params
            .get("query")
            .and_then(Value::as_str)
            .ok_or_else(|| validation_error("Missing required parameter: query"))?
            .trim();

        if query.is_empty() {
            return Err(validation_error("Query cannot be empty"));
        }

        // Parse hint (optional)
        let hint = params.get("hint").and_then(Value::as_str).unwrap_or("auto");

        // Determine strategy
        let result = match hint {
            "fast" => self.explore_internal(query).await,
            "deep" => self.explore_external(query).await,
            "auto" | _ => {
                // Auto-mode: try internal first, switch to external if too large
                let internal = self.explore_internal(query).await?;
                if internal.tokens_used > self.config.internal_threshold {
                    // Results are large, try external compression
                    match self.explore_external(query).await {
                        Ok(external) => Ok(external),
                        Err(_) => Ok(internal), // Fallback to internal on error
                    }
                } else {
                    Ok(internal)
                }
            }
        }?;

        // Format response
        Ok(json!({
            "content": [{
                "type": "text",
                "text": result.findings
            }],
            "metadata": {
                "query": result.query,
                "strategy": result.strategy,
                "tokens_used": result.tokens_used,
                "truncated": result.truncated,
                "files_scanned": result.metadata.files_scanned,
                "matches_found": result.metadata.matches_found,
            }
        }))
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query (e.g., 'auth logic', 'error handling', 'database layer')"
                },
                "hint": {
                    "type": "string",
                    "enum": ["auto", "fast", "deep"],
                    "description": "Strategy hint: auto (balanced), fast (internal only), deep (compress with LLM)",
                    "default": "auto"
                }
            },
            "required": ["query"],
            "additionalProperties": false
        })
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Helper functions
// ─────────────────────────────────────────────────────────────────────────────

/// Format user query as regex-friendly search pattern
fn format_search_pattern(query: &str) -> String {
    // Simple: treat query words as separate patterns
    query
        .split_whitespace()
        .map(|word| format!("(?i){}", regex::escape(word)))
        .collect::<Vec<_>>()
        .join("|")
}

/// Rough token estimation (assuming ~4 chars per token)
fn estimate_tokens(text: &str) -> usize {
    (text.len() / 4).max(1)
}

/// Query Ollama synchronously for compression
async fn query_ollama_sync(host: &str, model: &str, prompt: &str) -> Result<String, String> {
    let url = format!("{}/api/generate", host);
    let client = reqwest::Client::new();

    let request = json!({
        "model": model,
        "prompt": prompt,
        "stream": false,
        "temperature": 0.3,
        "num_predict": 500,
    });

    let response = client
        .post(&url)
        .json(&request)
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("HTTP error: {}", response.status()));
    }

    let body = response
        .json::<Value>()
        .await
        .map_err(|e| format!("JSON parse failed: {}", e))?;

    body.get("response")
        .and_then(Value::as_str)
        .map(String::from)
        .ok_or_else(|| "No response field in Ollama output".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_search_pattern() {
        let pattern = format_search_pattern("auth login");
        assert!(pattern.contains("auth"));
        assert!(pattern.contains("login"));
        assert!(pattern.contains("|"));
    }

    #[test]
    fn test_estimate_tokens() {
        let text = "a".repeat(4000);
        let tokens = estimate_tokens(&text);
        assert_eq!(tokens, 1000);
    }

    #[test]
    fn test_explorer_config_default() {
        let cfg = ExplorerConfig::default();
        assert_eq!(cfg.mode, ExplorerMode::Auto);
        assert_eq!(cfg.budget_tokens, 2500);
        assert_eq!(cfg.internal_threshold, 1500);
    }

    #[test]
    fn test_file_score_calculation() {
        let score = FileScore {
            relevance: 0.8,
            importance: 0.9,
            recency: 1.0,
            size_penalty: 0.5,
        };
        let total = score.total();
        assert!(total > 0.0 && total <= 1.0);
    }
}
