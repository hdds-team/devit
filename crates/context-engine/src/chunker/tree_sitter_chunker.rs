// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2025-2026 naskel.com

//! Tree-sitter based chunker for AST-aware code splitting

use super::Chunker;
use crate::{ChunkType, CodeChunk, ContextError, Language, Result};
use async_trait::async_trait;
use std::path::Path;
use tree_sitter::{Parser, Query, QueryCursor};
use uuid::Uuid;

/// Chunker implementation using tree-sitter for AST-aware splitting
pub struct TreeSitterChunker {
    max_chunk_tokens: usize,
    chunk_overlap: usize,
}

impl TreeSitterChunker {
    pub fn new(max_chunk_tokens: usize, chunk_overlap: usize) -> Self {
        Self {
            max_chunk_tokens,
            chunk_overlap,
        }
    }

    /// Get tree-sitter language for a given Language enum
    fn get_ts_language(&self, lang: Language) -> Option<tree_sitter::Language> {
        match lang {
            Language::Rust => Some(tree_sitter_rust::language()),
            Language::Python => Some(tree_sitter_python::language()),
            Language::C => Some(tree_sitter_c::language()),
            Language::Cpp => Some(tree_sitter_cpp::language()),
            Language::JavaScript => Some(tree_sitter_javascript::language()),
            Language::TypeScript => Some(tree_sitter_typescript::language_typescript()),
            Language::Unknown => None,
        }
    }

    /// Get query patterns for semantic chunking based on language
    fn get_chunk_query(&self, lang: Language) -> Option<&'static str> {
        match lang {
            Language::Rust => Some(RUST_CHUNK_QUERY),
            Language::Python => Some(PYTHON_CHUNK_QUERY),
            Language::C => Some(C_CHUNK_QUERY),
            Language::Cpp => Some(CPP_CHUNK_QUERY),
            Language::JavaScript | Language::TypeScript => Some(JS_TS_CHUNK_QUERY),
            Language::Unknown => None,
        }
    }

    /// Parse file and extract semantic chunks using tree-sitter queries
    fn extract_semantic_chunks(
        &self,
        content: &str,
        path: &Path,
        language: Language,
        ts_lang: tree_sitter::Language,
        query_str: &str,
    ) -> Result<Vec<CodeChunk>> {
        let mut parser = Parser::new();
        parser
            .set_language(&ts_lang)
            .map_err(|e| ContextError::Chunker(format!("Failed to set language: {}", e)))?;

        let tree = parser
            .parse(content, None)
            .ok_or_else(|| ContextError::Chunker("Failed to parse file".to_string()))?;

        let query = Query::new(&ts_lang, query_str)
            .map_err(|e| ContextError::Chunker(format!("Failed to create query: {}", e)))?;

        let mut cursor = QueryCursor::new();
        let matches = cursor.matches(&query, tree.root_node(), content.as_bytes());

        let mut chunks = Vec::new();
        let file_mtime = std::fs::metadata(path)
            .map(|m| m.modified().ok())
            .ok()
            .flatten()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);

        for m in matches {
            for capture in m.captures {
                let node = capture.node;
                let capture_name = query.capture_names()[capture.index as usize];

                let start_line = node.start_position().row + 1;
                let end_line = node.end_position().row + 1;
                let chunk_content = node
                    .utf8_text(content.as_bytes())
                    .map_err(|e| ContextError::Chunker(format!("UTF-8 error: {}", e)))?
                    .to_string();

                let token_count = self.estimate_tokens(&chunk_content);

                // Skip empty or too small chunks
                if chunk_content.trim().is_empty() || token_count < 10 {
                    continue;
                }

                let chunk_type = match capture_name {
                    "function" | "method" => ChunkType::Function,
                    "class" | "struct" | "impl" => ChunkType::Class,
                    "module" => ChunkType::Module,
                    "import" | "use" => ChunkType::Imports,
                    "const" | "static" => ChunkType::Constants,
                    "type" | "typedef" => ChunkType::TypeDef,
                    _ => ChunkType::Block,
                };

                // Try to extract symbol name
                let symbol_name = self.extract_symbol_name(&node, content);

                let chunk = CodeChunk {
                    id: Uuid::new_v4().to_string(),
                    file_path: path.to_path_buf(),
                    start_line,
                    end_line,
                    content: chunk_content,
                    language: language.as_str().to_string(),
                    chunk_type,
                    symbol_name,
                    token_count,
                    file_mtime,
                };

                // If chunk is too large, split it further
                if token_count > self.max_chunk_tokens {
                    chunks.extend(self.split_large_chunk(chunk)?);
                } else {
                    chunks.push(chunk);
                }
            }
        }

        // If no semantic chunks found, fall back to line-based chunking
        if chunks.is_empty() {
            chunks = self.fallback_line_chunking(content, path, language, file_mtime)?;
        }

        Ok(chunks)
    }

    /// Extract symbol name from AST node (function name, class name, etc.)
    fn extract_symbol_name(&self, node: &tree_sitter::Node, content: &str) -> Option<String> {
        // Look for identifier child node
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "identifier" || child.kind() == "name" {
                return child
                    .utf8_text(content.as_bytes())
                    .ok()
                    .map(|s| s.to_string());
            }
            // For function definitions, look for function_item > identifier
            if child.kind() == "function_item" || child.kind() == "function_definition" {
                let mut inner_cursor = child.walk();
                for inner_child in child.children(&mut inner_cursor) {
                    if inner_child.kind() == "identifier" || inner_child.kind() == "name" {
                        return inner_child
                            .utf8_text(content.as_bytes())
                            .ok()
                            .map(|s| s.to_string());
                    }
                }
            }
        }
        None
    }

    /// Split a large chunk into smaller pieces with overlap
    fn split_large_chunk(&self, chunk: CodeChunk) -> Result<Vec<CodeChunk>> {
        let lines: Vec<&str> = chunk.content.lines().collect();
        let mut chunks = Vec::new();
        let mut start_idx = 0;

        while start_idx < lines.len() {
            let mut end_idx = start_idx;
            let mut current_tokens = 0;

            // Accumulate lines until we hit the token limit
            while end_idx < lines.len() && current_tokens < self.max_chunk_tokens {
                current_tokens += self.estimate_tokens(lines[end_idx]);
                end_idx += 1;
            }

            if end_idx == start_idx {
                end_idx = start_idx + 1; // Ensure progress
            }

            let sub_content = lines[start_idx..end_idx].join("\n");
            let sub_chunk = CodeChunk {
                id: Uuid::new_v4().to_string(),
                file_path: chunk.file_path.clone(),
                start_line: chunk.start_line + start_idx,
                end_line: chunk.start_line + end_idx - 1,
                content: sub_content.clone(),
                language: chunk.language.clone(),
                chunk_type: chunk.chunk_type,
                symbol_name: if start_idx == 0 {
                    chunk.symbol_name.clone()
                } else {
                    None
                },
                token_count: self.estimate_tokens(&sub_content),
                file_mtime: chunk.file_mtime,
            };

            chunks.push(sub_chunk);

            // Move start with overlap
            let overlap_lines = self.chunk_overlap / 4; // Approximate lines for overlap
            start_idx = end_idx.saturating_sub(overlap_lines);
            if start_idx <= end_idx.saturating_sub(end_idx - start_idx) {
                start_idx = end_idx; // Prevent infinite loop
            }
        }

        Ok(chunks)
    }

    /// Fallback to simple line-based chunking when AST parsing fails
    fn fallback_line_chunking(
        &self,
        content: &str,
        path: &Path,
        language: Language,
        file_mtime: u64,
    ) -> Result<Vec<CodeChunk>> {
        let lines: Vec<&str> = content.lines().collect();
        let mut chunks = Vec::new();
        let mut start_idx = 0;

        while start_idx < lines.len() {
            let mut end_idx = start_idx;
            let mut current_tokens = 0;

            while end_idx < lines.len() && current_tokens < self.max_chunk_tokens {
                current_tokens += self.estimate_tokens(lines[end_idx]);
                end_idx += 1;
            }

            if end_idx == start_idx {
                end_idx = start_idx + 1;
            }

            let chunk_content = lines[start_idx..end_idx].join("\n");
            let chunk = CodeChunk {
                id: Uuid::new_v4().to_string(),
                file_path: path.to_path_buf(),
                start_line: start_idx + 1,
                end_line: end_idx,
                content: chunk_content.clone(),
                language: language.as_str().to_string(),
                chunk_type: ChunkType::Block,
                symbol_name: None,
                token_count: self.estimate_tokens(&chunk_content),
                file_mtime,
            };

            chunks.push(chunk);

            let overlap_lines = self.chunk_overlap / 4;
            start_idx = end_idx.saturating_sub(overlap_lines);
            if start_idx <= end_idx.saturating_sub(end_idx - start_idx) {
                start_idx = end_idx;
            }
        }

        Ok(chunks)
    }
}

#[async_trait]
impl Chunker for TreeSitterChunker {
    async fn chunk_file(
        &self,
        path: &Path,
        content: &str,
        language: Language,
    ) -> Result<Vec<CodeChunk>> {
        // Get tree-sitter language
        let ts_lang = match self.get_ts_language(language) {
            Some(l) => l,
            None => {
                // Unknown language, use fallback chunking
                let file_mtime = std::fs::metadata(path)
                    .map(|m| m.modified().ok())
                    .ok()
                    .flatten()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                return self.fallback_line_chunking(content, path, language, file_mtime);
            }
        };

        // Get query for this language
        let query_str = match self.get_chunk_query(language) {
            Some(q) => q,
            None => {
                let file_mtime = std::fs::metadata(path)
                    .map(|m| m.modified().ok())
                    .ok()
                    .flatten()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                return self.fallback_line_chunking(content, path, language, file_mtime);
            }
        };

        self.extract_semantic_chunks(content, path, language, ts_lang, query_str)
    }

    fn estimate_tokens(&self, text: &str) -> usize {
        // Simple heuristic: ~4 chars per token for code
        // This is a rough approximation; could use tiktoken for accuracy
        (text.len() + 3) / 4
    }
}

// Tree-sitter queries for different languages
// These capture semantic units like functions, classes, etc.

const RUST_CHUNK_QUERY: &str = r#"
(function_item) @function
(impl_item) @impl
(struct_item) @struct
(enum_item) @class
(trait_item) @class
(mod_item) @module
(use_declaration) @use
(const_item) @const
(static_item) @static
(type_item) @type
"#;

const PYTHON_CHUNK_QUERY: &str = r#"
(function_definition) @function
(class_definition) @class
(import_statement) @import
(import_from_statement) @import
"#;

// C-only query (no classes, no namespaces)
const C_CHUNK_QUERY: &str = r#"
(function_definition) @function
(struct_specifier) @struct
(enum_specifier) @class
(preproc_include) @import
(type_definition) @typedef
"#;

// C++ query (includes classes and namespaces)
const CPP_CHUNK_QUERY: &str = r#"
(function_definition) @function
(struct_specifier) @struct
(class_specifier) @class
(enum_specifier) @class
(namespace_definition) @module
(preproc_include) @import
(type_definition) @typedef
"#;

const JS_TS_CHUNK_QUERY: &str = r#"
(function_declaration) @function
(arrow_function) @function
(method_definition) @method
(class_declaration) @class
(import_statement) @import
(export_statement) @module
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_rust_chunking() {
        let chunker = TreeSitterChunker::new(512, 64);
        let content = r#"
fn hello() {
    println!("Hello, world!");
}

struct Point {
    x: i32,
    y: i32,
}

impl Point {
    fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}
"#;
        let path = Path::new("test.rs");
        let chunks = chunker
            .chunk_file(path, content, Language::Rust)
            .await
            .unwrap();

        assert!(!chunks.is_empty());
        // Should have function, struct, and impl chunks
        let types: Vec<_> = chunks.iter().map(|c| c.chunk_type).collect();
        assert!(types.contains(&ChunkType::Function));
    }

    #[test]
    fn test_token_estimation() {
        let chunker = TreeSitterChunker::new(512, 64);
        let text = "fn main() { println!(\"hello\"); }";
        let tokens = chunker.estimate_tokens(text);
        // ~33 chars / 4 = ~8 tokens
        assert!(tokens > 5 && tokens < 15);
    }
}
