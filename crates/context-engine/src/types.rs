// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2025-2026 naskel.com

//! Core types for the Context Engine

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// A chunk of code with its metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeChunk {
    /// Unique identifier for this chunk
    pub id: String,

    /// Source file path (relative to workspace)
    pub file_path: PathBuf,

    /// Start line (1-indexed)
    pub start_line: usize,

    /// End line (1-indexed)
    pub end_line: usize,

    /// The actual code content
    pub content: String,

    /// Programming language
    pub language: String,

    /// Semantic type (function, class, module, etc.)
    pub chunk_type: ChunkType,

    /// Optional symbol name (function name, class name, etc.)
    pub symbol_name: Option<String>,

    /// Estimated token count
    pub token_count: usize,

    /// File modification timestamp
    pub file_mtime: u64,
}

/// Type of code chunk
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChunkType {
    /// Function or method
    Function,
    /// Class or struct definition
    Class,
    /// Module or namespace
    Module,
    /// Import/use statements
    Imports,
    /// Constants or static definitions
    Constants,
    /// Type definitions (typedef, type alias)
    TypeDef,
    /// Generic code block (fallback)
    Block,
    /// Documentation/comments
    Documentation,
}

impl ChunkType {
    pub fn as_str(&self) -> &'static str {
        match self {
            ChunkType::Function => "function",
            ChunkType::Class => "class",
            ChunkType::Module => "module",
            ChunkType::Imports => "imports",
            ChunkType::Constants => "constants",
            ChunkType::TypeDef => "typedef",
            ChunkType::Block => "block",
            ChunkType::Documentation => "documentation",
        }
    }
}

/// A chunk with its embedding vector
#[derive(Debug, Clone)]
pub struct EmbeddedChunk {
    pub chunk: CodeChunk,
    pub embedding: Vec<f32>,
}

/// A context chunk returned from queries (chunk + relevance score)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextChunk {
    /// The code chunk
    pub chunk: CodeChunk,

    /// Similarity score (0.0 - 1.0)
    pub score: f32,
}

/// Supported programming languages
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Language {
    Rust,
    Python,
    C,
    Cpp,
    JavaScript,
    TypeScript,
    Unknown,
}

impl Language {
    pub fn from_extension(ext: &str) -> Self {
        match ext.to_lowercase().as_str() {
            "rs" => Language::Rust,
            "py" => Language::Python,
            "c" | "h" => Language::C,
            "cpp" | "cc" | "cxx" | "hpp" | "hh" | "hxx" => Language::Cpp,
            "js" | "jsx" | "mjs" => Language::JavaScript,
            "ts" | "tsx" => Language::TypeScript,
            _ => Language::Unknown,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Language::Rust => "rust",
            Language::Python => "python",
            Language::C => "c",
            Language::Cpp => "cpp",
            Language::JavaScript => "javascript",
            Language::TypeScript => "typescript",
            Language::Unknown => "unknown",
        }
    }
}

/// JSON-based edit format for LLM responses
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructuredEdit {
    /// Target file path
    pub file: String,

    /// List of edits to apply
    pub edits: Vec<EditOperation>,
}

/// A single edit operation
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum EditOperation {
    /// Replace a range of lines
    Replace {
        /// Start line (1-indexed)
        start_line: usize,
        /// End line (1-indexed, inclusive)
        end_line: usize,
        /// New content to insert
        content: String,
    },

    /// Insert new content after a line
    InsertAfter {
        /// Line number after which to insert (1-indexed)
        after_line: usize,
        /// Content to insert
        content: String,
    },

    /// Delete a range of lines
    Delete {
        /// Start line (1-indexed)
        start_line: usize,
        /// End line (1-indexed, inclusive)
        end_line: usize,
    },

    /// Replace entire file
    ReplaceFile {
        /// New file content
        content: String,
    },
}

/// Example JSON format for LLM edits:
/// ```json
/// {
///   "file": "src/main.rs",
///   "edits": [
///     {
///       "op": "replace",
///       "start_line": 10,
///       "end_line": 15,
///       "content": "fn new_function() {\n    // ...\n}"
///     },
///     {
///       "op": "insert_after",
///       "after_line": 5,
///       "content": "use std::collections::HashMap;"
///     }
///   ]
/// }
/// ```
pub const EDIT_FORMAT_EXAMPLE: &str = r#"{
  "file": "src/main.rs",
  "edits": [
    {
      "op": "replace",
      "start_line": 10,
      "end_line": 15,
      "content": "fn new_function() {\n    // implementation\n}"
    }
  ]
}"#;
