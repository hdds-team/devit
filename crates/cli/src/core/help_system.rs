// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2025-2026 naskel.com

//! # DevIt Help System
//!
//! This module provides intelligent help and auto-documentation for DevIt MCP tools,
//! specifically designed to assist AI assistants with usage optimization and token savings.

use crate::core::formats::{FormatUtils, OutputFormat};
use crate::core::{DevItError, DevItResult};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

/// Help documentation for a DevIt tool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolHelp {
    /// Name of the tool (e.g., "devit_file_read_ext")
    pub tool_name: String,
    /// Human-readable description of the tool's purpose
    pub description: String,
    /// Available output formats with descriptions
    pub formats: HashMap<String, String>,
    /// Usage examples with concrete use cases
    pub examples: Vec<UsageExample>,
    /// AI-specific optimization tips
    pub ai_tips: Vec<String>,
    /// Performance hints for large-scale usage
    pub performance_hints: Vec<String>,
}

/// Concrete usage example for a tool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageExample {
    /// Description of when to use this example
    pub use_case: String,
    /// MCP command parameters as JSON
    pub command: Value,
    /// Sample output (truncated for brevity)
    pub output_sample: String,
    /// Token savings compared to baseline (0.0-1.0, None if baseline)
    pub token_savings: Option<f32>,
}

/// Help system manager
pub struct HelpSystem {
    /// Cache of generated help content
    help_cache: HashMap<String, ToolHelp>,
    /// Available tools and their schemas
    tool_schemas: HashMap<String, Value>,
}

impl HelpSystem {
    /// Create a new help system
    pub fn new() -> Self {
        Self {
            help_cache: HashMap::new(),
            tool_schemas: HashMap::new(),
        }
    }

    /// Register a tool schema for help generation
    pub fn register_tool_schema(&mut self, tool_name: &str, schema: Value) {
        self.tool_schemas.insert(tool_name.to_string(), schema);
    }

    /// Get help for a specific tool
    pub fn get_tool_help(&mut self, tool_name: &str) -> DevItResult<&ToolHelp> {
        if !self.help_cache.contains_key(tool_name) {
            let help = self.generate_tool_help(tool_name)?;
            self.help_cache.insert(tool_name.to_string(), help);
        }

        Ok(self.help_cache.get(tool_name).unwrap())
    }

    /// Generate help content for a tool
    fn generate_tool_help(&self, tool_name: &str) -> DevItResult<ToolHelp> {
        match tool_name {
            // File operations
            "devit_file_read" => self.generate_file_read_help(),
            "devit_file_read_ext" => self.generate_file_read_ext_help(),
            "devit_file_write" => self.generate_file_write_help(),
            "devit_directory_list" => self.generate_directory_list_help(),
            "devit_file_list" => self.generate_file_list_help(),
            "devit_file_list_ext" => self.generate_file_list_ext_help(),
            "devit_file_search" => self.generate_file_search_help(),
            "devit_file_search_ext" => self.generate_file_search_ext_help(),
            "devit_project_structure" => self.generate_project_structure_help(),
            "devit_project_structure_ext" => self.generate_project_structure_ext_help(),
            "devit_pwd" => self.generate_pwd_help(),
            "devit_patch_apply" => self.generate_patch_apply_help(),
            // Git operations
            "devit_git_log" => self.generate_git_log_help(),
            "devit_git_blame" => self.generate_git_blame_help(),
            "devit_git_show" => self.generate_git_show_help(),
            "devit_git_diff" => self.generate_git_diff_help(),
            "devit_git_search" => self.generate_git_search_help(),
            // Process management
            "devit_exec" => self.generate_exec_help(),
            "devit_shell" => self.generate_shell_help(),
            "devit_ps" => self.generate_ps_help(),
            "devit_kill" => self.generate_kill_help(),
            // Window management
            "devit_window_list" => self.generate_window_list_help(),
            "devit_window_send_text" => self.generate_window_send_text_help(),
            "devit_window_focus" => self.generate_window_focus_help(),
            "devit_window_screenshot" => self.generate_window_screenshot_help(),
            "devit_window_get_content" => self.generate_window_get_content_help(),
            // Input control
            "devit_keyboard" => self.generate_keyboard_help(),
            "devit_mouse" => self.generate_mouse_help(),
            // System & Media
            "devit_ocr" => self.generate_ocr_help(),
            "devit_ocr_alerts" => self.generate_ocr_alerts_help(),
            "devit_screenshot" => self.generate_screenshot_help(),
            "devit_resource_monitor" => self.generate_resource_monitor_help(),
            "devit_search_web" => self.generate_search_web_help(),
            "devit_fetch_url" => self.generate_fetch_url_help(),
            // Orchestration & Utility
            "devit_snapshot" => self.generate_snapshot_help(),
            "devit_journal_append" => self.generate_journal_append_help(),
            "devit_delegate" => self.generate_delegate_help(),
            "devit_notify" => self.generate_notify_help(),
            "devit_orchestration_status" => self.generate_orchestration_status_help(),
            "devit_task_result" => self.generate_task_result_help(),
            "devit_poll_tasks" => self.generate_poll_tasks_help(),
            "devit_test_run" => self.generate_test_run_help(),
            _ => Err(DevItError::InvalidFormat {
                format: tool_name.to_string(),
                supported: vec![
                    "devit_file_read".to_string(),
                    "devit_file_read_ext".to_string(),
                    "devit_file_list".to_string(),
                    "devit_file_list_ext".to_string(),
                ],
            }),
        }
    }

    /// Generate help for devit_file_read (baseline tool)
    fn generate_file_read_help(&self) -> DevItResult<ToolHelp> {
        Ok(ToolHelp {
            tool_name: "devit_file_read".to_string(),
            description: "Read file content with line numbers and pagination support".to_string(),
            formats: {
                let mut formats = HashMap::new();
                formats.insert("json".to_string(), "Standard verbose JSON format".to_string());
                formats
            },
            examples: vec![
                UsageExample {
                    use_case: "Read entire file".to_string(),
                    command: serde_json::json!({
                        "path": "src/main.rs"
                    }),
                    output_sample: r#"{"path":"src/main.rs","content":"fn main() {\n    println!(\"Hello, world!\");\n}","size":42,"lines":["1: fn main() {","2:     println!(\"Hello, world!\");","3: }"],"encoding":"utf-8"}"#.to_string(),
                    token_savings: None, // Baseline
                },
                UsageExample {
                    use_case: "Read file with pagination".to_string(),
                    command: serde_json::json!({
                        "path": "src/lib.rs",
                        "offset": 10,
                        "limit": 20
                    }),
                    output_sample: r#"{"path":"src/lib.rs","content":"// Lines 10-30 content...","size":1024,"lines":["10: pub fn example() {","11:     // Implementation","..."],"encoding":"utf-8"}"#.to_string(),
                    token_savings: None,
                },
            ],
            ai_tips: vec![
                "Use offset and limit for large files to control token usage".to_string(),
                "Always check file size before reading to avoid token overflow".to_string(),
                "Prefer devit_file_read_ext for better token efficiency".to_string(),
            ],
            performance_hints: vec![
                "Files over 100KB should use pagination".to_string(),
                "Consider using devit_file_search for finding specific content".to_string(),
            ],
        })
    }

    /// Generate help for devit_file_read_ext (extended tool with compression)
    fn generate_file_read_ext_help(&self) -> DevItResult<ToolHelp> {
        Ok(ToolHelp {
            tool_name: "devit_file_read_ext".to_string(),
            description: "Read file content with compression and filtering options for token optimization".to_string(),
            formats: {
                let mut formats = HashMap::new();
                formats.insert("json".to_string(), "Standard verbose JSON (baseline)".to_string());
                formats.insert("compact".to_string(), "Abbreviated JSON (60% token reduction)".to_string());
                formats.insert("table".to_string(), "Pipe-delimited format (80% token reduction)".to_string());
                formats
            },
            examples: vec![
                UsageExample {
                    use_case: "Quick content check with compression".to_string(),
                    command: serde_json::json!({
                        "path": "main.rs",
                        "format": "compact",
                        "limit": 50
                    }),
                    output_sample: r#"{"f":"main.rs","s":1024,"c":"fn main() {\n    println!(\"Hello!\");\n}","e":"utf-8"}"#.to_string(),
                    token_savings: Some(0.6),
                },
                UsageExample {
                    use_case: "Minimal file overview for AI processing".to_string(),
                    command: serde_json::json!({
                        "path": "src/utils.rs",
                        "format": "table",
                        "limit": 100
                    }),
                    output_sample: "path|size|encoding|content\nsrc/utils.rs|2048|utf-8|pub fn helper() { ... }".to_string(),
                    token_savings: Some(0.8),
                },
                UsageExample {
                    use_case: "Standard detailed reading".to_string(),
                    command: serde_json::json!({
                        "path": "config.toml",
                        "format": "json"
                    }),
                    output_sample: r#"{"path":"config.toml","content":"[database]\nurl = \"localhost\"","size":156,"lines":["1: [database]","2: url = \"localhost\""],"encoding":"utf-8"}"#.to_string(),
                    token_savings: None,
                },
            ],
            ai_tips: vec![
                "Use 'compact' format for routine file reading to save 60% tokens".to_string(),
                "Use 'table' format when processing many files sequentially".to_string(),
                "Always set --limit for large files to avoid token overflow".to_string(),
                "Format 'compact' is optimal for file content analysis".to_string(),
                "Switch to 'json' only when you need full field names for debugging".to_string(),
            ],
            performance_hints: vec![
                "Compact format processes 2.5x faster for large files".to_string(),
                "Table format is ideal for batch processing of 10+ files".to_string(),
                "Use limit parameter to cap token usage for AI context management".to_string(),
            ],
        })
    }

    /// Generate help for devit_directory_list
    fn generate_directory_list_help(&self) -> DevItResult<ToolHelp> {
        Ok(ToolHelp {
            tool_name: "devit_directory_list".to_string(),
            description: "List directory contents with lightweight metadata and filtering".to_string(),
            formats: {
                let mut formats = HashMap::new();
                formats.insert("json".to_string(), "Directory listing with file/folder metadata".to_string());
                formats
            },
            examples: vec![
                UsageExample {
                    use_case: "List directory contents".to_string(),
                    command: serde_json::json!({
                        "path": "src/"
                    }),
                    output_sample: r#"[{"name":"main.rs","path":"src/main.rs","type":"file","size":1024,"modified":"2024-01-01T12:00:00Z"}]"#.to_string(),
                    token_savings: None,
                },
            ],
            ai_tips: vec![
                "Use for quick directory scanning".to_string(),
                "Combine with filtering for better performance".to_string(),
            ],
            performance_hints: vec![
                "Lightweight operation for directory exploration".to_string(),
            ],
        })
    }

    /// Generate help for devit_file_list
    fn generate_file_list_help(&self) -> DevItResult<ToolHelp> {
        Ok(ToolHelp {
            tool_name: "devit_file_list".to_string(),
            description: "List directory contents with metadata and filtering options".to_string(),
            formats: {
                let mut formats = HashMap::new();
                formats.insert("json".to_string(), "Standard verbose JSON with full metadata".to_string());
                formats
            },
            examples: vec![
                UsageExample {
                    use_case: "List current directory".to_string(),
                    command: serde_json::json!({
                        "path": "."
                    }),
                    output_sample: r#"[{"name":"main.rs","path":"./main.rs","entry_type":"File","size":1024,"modified":"2024-01-01T12:00:00Z","permissions":{"readable":true,"writable":true,"executable":false}}]"#.to_string(),
                    token_savings: None,
                },
                UsageExample {
                    use_case: "Recursive directory listing".to_string(),
                    command: serde_json::json!({
                        "path": "src",
                        "recursive": true,
                        "include_hidden": false
                    }),
                    output_sample: r#"[{"name":"lib.rs","path":"src/lib.rs","entry_type":"File","size":2048,"modified":"2024-01-01T12:00:00Z","permissions":{"readable":true,"writable":true,"executable":false}},{"name":"utils","path":"src/utils","entry_type":"Directory","size":null,"modified":"2024-01-01T11:00:00Z","permissions":{"readable":true,"writable":true,"executable":true}}]"#.to_string(),
                    token_savings: None,
                },
            ],
            ai_tips: vec![
                "Use recursive=false for large directories to control output size".to_string(),
                "Filter by file types using include_patterns for focused analysis".to_string(),
                "Consider devit_file_list_ext for token-efficient directory scanning".to_string(),
            ],
            performance_hints: vec![
                "Large directories (>100 files) should use pagination or filtering".to_string(),
                "Recursive listing can generate significant token usage".to_string(),
            ],
        })
    }

    /// Generate help for devit_file_list_ext
    fn generate_file_list_ext_help(&self) -> DevItResult<ToolHelp> {
        Ok(ToolHelp {
            tool_name: "devit_file_list_ext".to_string(),
            description: "List directory contents with compression and smart filtering for AI optimization".to_string(),
            formats: {
                let mut formats = HashMap::new();
                formats.insert("json".to_string(), "Standard verbose JSON with full metadata".to_string());
                formats.insert("compact".to_string(), "Abbreviated JSON (60% token reduction)".to_string());
                formats.insert("table".to_string(), "Pipe-delimited format (80% token reduction)".to_string());
                formats
            },
            examples: vec![
                UsageExample {
                    use_case: "Quick directory overview".to_string(),
                    command: serde_json::json!({
                        "path": "src",
                        "format": "compact"
                    }),
                    output_sample: r#"[{"n":"main.rs","f":"src/main.rs","t":"File","s":1024,"p":{"r":true,"w":true,"x":false}},{"n":"lib.rs","f":"src/lib.rs","t":"File","s":2048,"p":{"r":true,"w":true,"x":false}}]"#.to_string(),
                    token_savings: Some(0.6),
                },
                UsageExample {
                    use_case: "Batch file processing list".to_string(),
                    command: serde_json::json!({
                        "path": "tests",
                        "format": "table",
                        "recursive": true
                    }),
                    output_sample: "name|path|type|size|permissions\ntest1.rs|tests/test1.rs|File|512|rwx\ntest2.rs|tests/test2.rs|File|768|rwx".to_string(),
                    token_savings: Some(0.8),
                },
            ],
            ai_tips: vec![
                "Use 'compact' format for directory analysis to save tokens".to_string(),
                "Use 'table' format when you need to process many files in sequence".to_string(),
                "Combine with include_patterns to filter relevant files only".to_string(),
                "Table format is perfect for generating file processing plans".to_string(),
            ],
            performance_hints: vec![
                "Table format reduces memory usage for large directory listings".to_string(),
                "Compact format maintains full metadata while saving 60% tokens".to_string(),
            ],
        })
    }

    /// Generate help for devit_file_search
    fn generate_file_search_help(&self) -> DevItResult<ToolHelp> {
        Ok(ToolHelp {
            tool_name: "devit_file_search".to_string(),
            description: "Search for patterns in files with context and metadata".to_string(),
            formats: {
                let mut formats = HashMap::new();
                formats.insert("json".to_string(), "Standard verbose JSON with full search results".to_string());
                formats
            },
            examples: vec![
                UsageExample {
                    use_case: "Find function definitions".to_string(),
                    command: serde_json::json!({
                        "pattern": "fn\\s+\\w+",
                        "path": "src",
                        "context_lines": 2
                    }),
                    output_sample: r#"{"pattern":"fn\\s+\\w+","path":"src","files_searched":5,"total_matches":12,"matches":[{"file":"src/main.rs","line_number":1,"line":"fn main() {","context_before":[""],"context_after":["    println!(\"Hello!\");","}"]}],"truncated":false}"#.to_string(),
                    token_savings: None,
                },
            ],
            ai_tips: vec![
                "Use specific regex patterns to find relevant code sections".to_string(),
                "Adjust context_lines based on how much surrounding code you need".to_string(),
                "Consider devit_file_search_ext for token-efficient search results".to_string(),
            ],
            performance_hints: vec![
                "Complex regex patterns may slow down search on large codebases".to_string(),
                "Use file_pattern to limit search scope".to_string(),
            ],
        })
    }

    /// Generate help for devit_file_search_ext
    fn generate_file_search_ext_help(&self) -> DevItResult<ToolHelp> {
        Ok(ToolHelp {
            tool_name: "devit_file_search_ext".to_string(),
            description: "Search for patterns with compression and AI-optimized result formatting".to_string(),
            formats: {
                let mut formats = HashMap::new();
                formats.insert("json".to_string(), "Standard verbose JSON with full search results".to_string());
                formats.insert("compact".to_string(), "Abbreviated JSON (60% token reduction)".to_string());
                formats.insert("table".to_string(), "Pipe-delimited format (80% token reduction)".to_string());
                formats
            },
            examples: vec![
                UsageExample {
                    use_case: "Quick pattern search with compression".to_string(),
                    command: serde_json::json!({
                        "pattern": "TODO|FIXME",
                        "path": "src",
                        "format": "compact",
                        "context_lines": 1
                    }),
                    output_sample: r#"{"pat":"TODO|FIXME","f":"src","fs":8,"tm":3,"mt":[{"sf":"src/main.rs","ln":45,"sl":"// TODO: implement this","cb":["fn process() {"],"ca":["    return;"]}],"tc":false}"#.to_string(),
                    token_savings: Some(0.6),
                },
                UsageExample {
                    use_case: "Tabular search results for analysis".to_string(),
                    command: serde_json::json!({
                        "pattern": "error|Error",
                        "path": ".",
                        "format": "table"
                    }),
                    output_sample: "file|line|match|context\nsrc/main.rs|23|Error handling|fn handle_error()\nsrc/lib.rs|67|error message|log::error()".to_string(),
                    token_savings: Some(0.8),
                },
            ],
            ai_tips: vec![
                "Use 'compact' format for search results to save significant tokens".to_string(),
                "Use 'table' format when analyzing many search matches".to_string(),
                "Combine search with specific file patterns to focus results".to_string(),
                "Table format is excellent for generating fix/improvement plans".to_string(),
            ],
            performance_hints: vec![
                "Compact format is ideal for large search result sets".to_string(),
                "Table format facilitates batch processing of search results".to_string(),
            ],
        })
    }

    /// Generate help for devit_project_structure
    fn generate_project_structure_help(&self) -> DevItResult<ToolHelp> {
        Ok(ToolHelp {
            tool_name: "devit_project_structure".to_string(),
            description: "Generate comprehensive project structure with auto-detection and tree view".to_string(),
            formats: {
                let mut formats = HashMap::new();
                formats.insert("json".to_string(), "Standard verbose JSON with full project metadata".to_string());
                formats
            },
            examples: vec![
                UsageExample {
                    use_case: "Analyze project structure".to_string(),
                    command: serde_json::json!({
                        "path": "."
                    }),
                    output_sample: r#"{"root":".","project_type":"rust","tree":{"name":"my-project","node_type":"Directory","children":[{"name":"src","node_type":"Directory","children":[{"name":"main.rs","node_type":"File","children":[]}]}]},"total_files":15,"total_dirs":4}"#.to_string(),
                    token_savings: None,
                },
            ],
            ai_tips: vec![
                "Use for understanding overall project architecture".to_string(),
                "Helpful for generating project documentation".to_string(),
                "Consider devit_project_structure_ext for token efficiency".to_string(),
            ],
            performance_hints: vec![
                "Large projects may generate substantial output".to_string(),
                "Use max_depth to limit tree traversal".to_string(),
            ],
        })
    }

    /// Generate help for devit_project_structure_ext
    fn generate_project_structure_ext_help(&self) -> DevItResult<ToolHelp> {
        Ok(ToolHelp {
            tool_name: "devit_project_structure_ext".to_string(),
            description: "Generate project structure with compression and AI-focused formatting".to_string(),
            formats: {
                let mut formats = HashMap::new();
                formats.insert("json".to_string(), "Standard verbose JSON with full project metadata".to_string());
                formats.insert("compact".to_string(), "Abbreviated JSON (60% token reduction)".to_string());
                formats.insert("table".to_string(), "Pipe-delimited tree format (80% token reduction)".to_string());
                formats
            },
            examples: vec![
                UsageExample {
                    use_case: "Compressed project overview".to_string(),
                    command: serde_json::json!({
                        "path": ".",
                        "format": "compact"
                    }),
                    output_sample: r#"{"rt":".","pt":"rust","tr":{"n":"project","nt":"Directory","ch":[{"n":"src","nt":"Directory","ch":[{"n":"main.rs","nt":"File"}]}]},"tf":15,"td":4}"#.to_string(),
                    token_savings: Some(0.6),
                },
                UsageExample {
                    use_case: "Tabular project tree".to_string(),
                    command: serde_json::json!({
                        "path": ".",
                        "format": "table",
                        "max_depth": 3
                    }),
                    output_sample: "name|type|path|level\nproject|Directory|.|0\nsrc|Directory|src|1\nmain.rs|File|src/main.rs|2".to_string(),
                    token_savings: Some(0.8),
                },
            ],
            ai_tips: vec![
                "Use 'compact' format for project analysis to save tokens".to_string(),
                "Use 'table' format for generating navigation or build plans".to_string(),
                "Limit max_depth for large projects to control output size".to_string(),
                "Table format excellent for creating project maps".to_string(),
            ],
            performance_hints: vec![
                "Compact format maintains full structure info while saving tokens".to_string(),
                "Table format ideal for hierarchical processing".to_string(),
            ],
        })
    }

    /// Generate help for devit_pwd
    fn generate_pwd_help(&self) -> DevItResult<ToolHelp> {
        Ok(ToolHelp {
            tool_name: "devit_pwd".to_string(),
            description: "Get current working directory with path resolution and validation".to_string(),
            formats: {
                let mut formats = HashMap::new();
                formats.insert("json".to_string(), "Standard JSON with directory information".to_string());
                formats
            },
            examples: vec![
                UsageExample {
                    use_case: "Get current directory".to_string(),
                    command: serde_json::json!({}),
                    output_sample: r#"{"current_directory":"/home/user/project","absolute_path":"/home/user/project","exists":true,"readable":true,"writable":true}"#.to_string(),
                    token_savings: None,
                },
            ],
            ai_tips: vec![
                "Use to establish context before other file operations".to_string(),
                "Helpful for understanding relative path references".to_string(),
            ],
            performance_hints: vec![
                "Lightweight operation, suitable for frequent use".to_string(),
            ],
        })
    }

    /// Generate help for devit_file_write
    fn generate_file_write_help(&self) -> DevItResult<ToolHelp> {
        Ok(ToolHelp {
            tool_name: "devit_file_write".to_string(),
            description: "Write content to files with security validation and mode options".to_string(),
            formats: {
                let mut formats = HashMap::new();
                formats.insert("json".to_string(), "Standard JSON with write operation result".to_string());
                formats
            },
            examples: vec![
                UsageExample {
                    use_case: "Create new file".to_string(),
                    command: serde_json::json!({
                        "path": "src/new_module.rs",
                        "content": "pub fn new_function() {\n    // Implementation\n}",
                        "mode": "create_new"
                    }),
                    output_sample: r#"{"path":"src/new_module.rs","bytes_written":45,"mode":"create_new","success":true,"timestamp":"2024-01-01T12:00:00Z"}"#.to_string(),
                    token_savings: None,
                },
            ],
            ai_tips: vec![
                "Always use 'create_new' mode to avoid accidental overwrites".to_string(),
                "Validate file content before writing to prevent corruption".to_string(),
            ],
            performance_hints: vec![
                "Write operations include security checks and validation".to_string(),
            ],
        })
    }

    /// Generate help for devit_patch_apply
    fn generate_patch_apply_help(&self) -> DevItResult<ToolHelp> {
        Ok(ToolHelp {
            tool_name: "devit_patch_apply".to_string(),
            description: "Apply unified diff patches with validation and dry-run support".to_string(),
            formats: {
                let mut formats = HashMap::new();
                formats.insert("json".to_string(), "Standard JSON with patch application result".to_string());
                formats
            },
            examples: vec![
                UsageExample {
                    use_case: "Apply patch with dry-run preview".to_string(),
                    command: serde_json::json!({
                        "diff": "--- a/file.rs\n+++ b/file.rs\n@@ -1,3 +1,4 @@\n fn main() {\n+    println!(\"patched\");\n     println!(\"original\");\n }",
                        "dry_run": true
                    }),
                    output_sample: r#"{"success":true,"dry_run":true,"files_affected":1,"changes_applied":0,"hunks":1,"preview":"Lines 1-4 would be modified"}"#.to_string(),
                    token_savings: None,
                },
            ],
            ai_tips: vec![
                "Always do dry_run=true before actual patch application".to_string(),
                "Verify patch content before applying to production code".to_string(),
            ],
            performance_hints: vec![
                "Dry-run mode validates patches without modifying files".to_string(),
            ],
        })
    }

    /// Generate help for devit_git_log
    fn generate_git_log_help(&self) -> DevItResult<ToolHelp> {
        Ok(ToolHelp {
            tool_name: "devit_git_log".to_string(),
            description: "View git commit history with filtering and formatting options".to_string(),
            formats: {
                let mut formats = HashMap::new();
                formats.insert("json".to_string(), "Structured commit history with full details".to_string());
                formats
            },
            examples: vec![
                UsageExample {
                    use_case: "Get last 10 commits".to_string(),
                    command: serde_json::json!({
                        "max_count": 10
                    }),
                    output_sample: r#"[{"hash":"abc123","author":"John Doe","date":"2024-01-01T12:00:00Z","message":"Fix bug in parser"}]"#.to_string(),
                    token_savings: None,
                },
            ],
            ai_tips: vec![
                "Use max_count to limit output for token efficiency".to_string(),
                "Filter by path to analyze changes to specific files".to_string(),
            ],
            performance_hints: vec![
                "Large repositories benefit from max_count limiting".to_string(),
            ],
        })
    }

    /// Generate help for devit_git_blame
    fn generate_git_blame_help(&self) -> DevItResult<ToolHelp> {
        Ok(ToolHelp {
            tool_name: "devit_git_blame".to_string(),
            description: "Show git blame information with author and commit details per line".to_string(),
            formats: {
                let mut formats = HashMap::new();
                formats.insert("json".to_string(), "Line-by-line blame information with commit details".to_string());
                formats
            },
            examples: vec![
                UsageExample {
                    use_case: "Get blame for file range".to_string(),
                    command: serde_json::json!({
                        "path": "src/main.rs",
                        "line_start": 1,
                        "line_end": 50
                    }),
                    output_sample: r#"[{"line":1,"author":"Alice","commit":"abc123","date":"2024-01-01","content":"fn main() {"}]"#.to_string(),
                    token_savings: None,
                },
            ],
            ai_tips: vec![
                "Specify line ranges to avoid processing entire large files".to_string(),
                "Use to understand code evolution and author context".to_string(),
            ],
            performance_hints: vec![
                "Line range limiting significantly reduces output for large files".to_string(),
            ],
        })
    }

    /// Generate help for devit_git_show
    fn generate_git_show_help(&self) -> DevItResult<ToolHelp> {
        Ok(ToolHelp {
            tool_name: "devit_git_show".to_string(),
            description: "Show detailed commit information including diff content".to_string(),
            formats: {
                let mut formats = HashMap::new();
                formats.insert("json".to_string(), "Commit details with full diff information".to_string());
                formats
            },
            examples: vec![
                UsageExample {
                    use_case: "Show commit details".to_string(),
                    command: serde_json::json!({
                        "commit": "abc123"
                    }),
                    output_sample: r#"{"hash":"abc123","author":"John Doe","message":"Fix parser issue","diff":"--- a/src/parser.rs\n+++ b/src/parser.rs\n@@ -10,3 +10,4 @@\n  fn parse() {\n+    validate();\n  }"}"#.to_string(),
                    token_savings: None,
                },
            ],
            ai_tips: vec![
                "Use to review changes made in specific commits".to_string(),
            ],
            performance_hints: vec![
                "Large commits can generate substantial output".to_string(),
            ],
        })
    }

    /// Generate help for devit_git_diff
    fn generate_git_diff_help(&self) -> DevItResult<ToolHelp> {
        Ok(ToolHelp {
            tool_name: "devit_git_diff".to_string(),
            description: "Show git diff between commits, branches, or working tree".to_string(),
            formats: {
                let mut formats = HashMap::new();
                formats.insert("json".to_string(), "Unified diff format as JSON".to_string());
                formats
            },
            examples: vec![
                UsageExample {
                    use_case: "Diff between commits".to_string(),
                    command: serde_json::json!({
                        "range": "HEAD~5..HEAD"
                    }),
                    output_sample: r#"{"range":"HEAD~5..HEAD","files_changed":3,"insertions":42,"deletions":8,"diff":"--- a/file.rs\n+++ b/file.rs\n..."}"#.to_string(),
                    token_savings: None,
                },
            ],
            ai_tips: vec![
                "Use range parameter to compare commits efficiently".to_string(),
            ],
            performance_hints: vec![
                "Large diffs should be limited to specific files if possible".to_string(),
            ],
        })
    }

    /// Generate help for devit_git_search
    fn generate_git_search_help(&self) -> DevItResult<ToolHelp> {
        Ok(ToolHelp {
            tool_name: "devit_git_search".to_string(),
            description: "Search git history for pattern matches or code changes".to_string(),
            formats: {
                let mut formats = HashMap::new();
                formats.insert(
                    "json".to_string(),
                    "Search results with matching commits and files".to_string(),
                );
                formats
            },
            examples: vec![UsageExample {
                use_case: "Search for pattern in git history".to_string(),
                command: serde_json::json!({
                    "pattern": "TODO|FIXME",
                    "type": "log"
                }),
                output_sample:
                    r#"[{"commit":"abc123","author":"John","message":"Add TODO item","matches":1}]"#
                        .to_string(),
                token_savings: None,
            }],
            ai_tips: vec!["Use to find when features were introduced or removed".to_string()],
            performance_hints: vec!["Git search can be slow on large repositories".to_string()],
        })
    }

    /// Generate help for devit_exec
    fn generate_exec_help(&self) -> DevItResult<ToolHelp> {
        Ok(ToolHelp {
            tool_name: "devit_exec".to_string(),
            description: "Execute system commands with optional timeout and stdio redirection"
                .to_string(),
            formats: {
                let mut formats = HashMap::new();
                formats.insert(
                    "json".to_string(),
                    "Command output with exit code and timing".to_string(),
                );
                formats
            },
            examples: vec![UsageExample {
                use_case: "Run command with timeout".to_string(),
                command: serde_json::json!({
                    "binary": "cargo",
                    "args": ["build", "--release"],
                    "timeout_secs": 300
                }),
                output_sample:
                    r#"{"stdout":"Compiling...","stderr":"","exit_code":0,"duration_ms":5432}"#
                        .to_string(),
                token_savings: None,
            }],
            ai_tips: vec![
                "Always set timeout to prevent hanging processes".to_string(),
                "Check exit codes to verify command success".to_string(),
            ],
            performance_hints: vec![
                "Large stdout/stderr output should be redirected to files".to_string()
            ],
        })
    }

    /// Generate help for devit_shell
    fn generate_shell_help(&self) -> DevItResult<ToolHelp> {
        Ok(ToolHelp {
            tool_name: "devit_shell".to_string(),
            description: "Execute shell commands with bash -c wrapper. Simplified interface for running commands with pipes, redirections, and shell features.".to_string(),
            formats: {
                let mut formats = HashMap::new();
                formats.insert("json".to_string(), "Command output with exit code, stdout/stderr, and timing".to_string());
                formats
            },
            examples: vec![
                UsageExample {
                    use_case: "Simple command".to_string(),
                    command: serde_json::json!({
                        "command": "ls -la"
                    }),
                    output_sample: r#"{"stdout":"total 42\ndrwxr-xr-x ...","stderr":"","exit_code":0,"duration_ms":12}"#.to_string(),
                    token_savings: None,
                },
                UsageExample {
                    use_case: "Command with pipes and redirections".to_string(),
                    command: serde_json::json!({
                        "command": "cat file.txt | grep pattern | wc -l"
                    }),
                    output_sample: r#"{"stdout":"42","stderr":"","exit_code":0,"duration_ms":25}"#.to_string(),
                    token_savings: None,
                },
                UsageExample {
                    use_case: "Build command with custom timeout".to_string(),
                    command: serde_json::json!({
                        "command": "cargo build --release 2>&1",
                        "working_dir": "my_project",
                        "timeout_secs": 300
                    }),
                    output_sample: r#"{"stdout":"Compiling...","stderr":"","exit_code":0,"duration_ms":45000}"#.to_string(),
                    token_savings: None,
                },
                UsageExample {
                    use_case: "Background process".to_string(),
                    command: serde_json::json!({
                        "command": "python server.py",
                        "background": true
                    }),
                    output_sample: r#"{"pid":12345,"pgid":12345,"started_at":"2024-01-01T12:00:00Z"}"#.to_string(),
                    token_savings: None,
                },
            ],
            ai_tips: vec![
                "Use devit_shell instead of devit_exec for most commands - simpler syntax".to_string(),
                "Pipes (|), redirections (>, >>), and shell operators (&&, ||) work directly".to_string(),
                "Set timeout_secs for long-running commands to prevent hanging".to_string(),
                "Use background: true for servers or long processes, then monitor with devit_ps".to_string(),
                "Combine multiple commands with && for sequential execution".to_string(),
            ],
            performance_hints: vec![
                "Default timeout is 120 seconds, max is 600 seconds".to_string(),
                "Large output should be redirected to files to avoid memory issues".to_string(),
                "Background processes are registered and can be killed with devit_kill".to_string(),
            ],
        })
    }

    /// Generate help for devit_ps
    fn generate_ps_help(&self) -> DevItResult<ToolHelp> {
        Ok(ToolHelp {
            tool_name: "devit_ps".to_string(),
            description: "Query running processes with optional filtering by PID".to_string(),
            formats: {
                let mut formats = HashMap::new();
                formats.insert("json".to_string(), "Process list with metadata".to_string());
                formats
            },
            examples: vec![
                UsageExample {
                    use_case: "List all processes".to_string(),
                    command: serde_json::json!({}),
                    output_sample: r#"[{"pid":1234,"name":"process_name","status":"running","cpu":2.5,"memory":"45.2MB"}]"#.to_string(),
                    token_savings: None,
                },
            ],
            ai_tips: vec![
                "Use PID filtering for specific process monitoring".to_string(),
            ],
            performance_hints: vec![
                "Process listing is lightweight".to_string(),
            ],
        })
    }

    /// Generate help for devit_kill
    fn generate_kill_help(&self) -> DevItResult<ToolHelp> {
        Ok(ToolHelp {
            tool_name: "devit_kill".to_string(),
            description: "Terminate processes by PID with signal selection".to_string(),
            formats: {
                let mut formats = HashMap::new();
                formats.insert("json".to_string(), "Kill operation result".to_string());
                formats
            },
            examples: vec![UsageExample {
                use_case: "Kill process gracefully".to_string(),
                command: serde_json::json!({
                    "pid": 1234,
                    "signal": "TERM"
                }),
                output_sample:
                    r#"{"pid":1234,"signal":"TERM","success":true,"message":"Process terminated"}"#
                        .to_string(),
                token_savings: None,
            }],
            ai_tips: vec!["Use TERM signal first, then KILL if needed".to_string()],
            performance_hints: vec!["Careful with kill operations on system processes".to_string()],
        })
    }

    /// Generate help for devit_window_list
    fn generate_window_list_help(&self) -> DevItResult<ToolHelp> {
        Ok(ToolHelp {
            tool_name: "devit_window_list".to_string(),
            description: "List all open windows with ID, title, and geometry".to_string(),
            formats: {
                let mut formats = HashMap::new();
                formats.insert("json".to_string(), "Window list with metadata".to_string());
                formats
            },
            examples: vec![
                UsageExample {
                    use_case: "List all windows".to_string(),
                    command: serde_json::json!({}),
                    output_sample: r#"[{"window_id":"0x4400001","title":"Terminal","class":"XTerm","geometry":{"x":0,"y":0,"width":800,"height":600}}]"#.to_string(),
                    token_savings: None,
                },
            ],
            ai_tips: vec![
                "Use for automating window operations".to_string(),
            ],
            performance_hints: vec![
                "Window listing is lightweight".to_string(),
            ],
        })
    }

    /// Generate help for devit_window_send_text
    fn generate_window_send_text_help(&self) -> DevItResult<ToolHelp> {
        Ok(ToolHelp {
            tool_name: "devit_window_send_text".to_string(),
            description: "Send text input to a specific window by ID".to_string(),
            formats: {
                let mut formats = HashMap::new();
                formats.insert(
                    "json".to_string(),
                    "Send text result with confirmation".to_string(),
                );
                formats
            },
            examples: vec![UsageExample {
                use_case: "Send text to window".to_string(),
                command: serde_json::json!({
                    "window_id": "0x4400001",
                    "text": "Hello, World!"
                }),
                output_sample: r#"{"success":true,"message":"Text sent to window 0x4400001"}"#
                    .to_string(),
                token_savings: None,
            }],
            ai_tips: vec![
                "Use after window_focus to ensure correct target".to_string(),
                "Include delays if sending multiple messages".to_string(),
                "Get window_id from devit_window_list first".to_string(),
            ],
            performance_hints: vec![
                "Window must be focused before text is sent".to_string(),
                "Includes 100ms delay for window focus".to_string(),
            ],
        })
    }

    /// Generate help for devit_window_focus
    fn generate_window_focus_help(&self) -> DevItResult<ToolHelp> {
        Ok(ToolHelp {
            tool_name: "devit_window_focus".to_string(),
            description: "Focus a window by its ID".to_string(),
            formats: {
                let mut formats = HashMap::new();
                formats.insert("json".to_string(), "Focus operation result".to_string());
                formats
            },
            examples: vec![UsageExample {
                use_case: "Focus specific window".to_string(),
                command: serde_json::json!({
                    "window_id": "0x4400001"
                }),
                output_sample:
                    r#"{"window_id":"0x4400001","focused":true,"timestamp":"2024-01-01T12:00:00Z"}"#
                        .to_string(),
                token_savings: None,
            }],
            ai_tips: vec!["Use window_list first to get available window IDs".to_string()],
            performance_hints: vec!["Window focus is instant".to_string()],
        })
    }

    /// Generate help for devit_window_screenshot
    fn generate_window_screenshot_help(&self) -> DevItResult<ToolHelp> {
        Ok(ToolHelp {
            tool_name: "devit_window_screenshot".to_string(),
            description: "Capture a screenshot of a specific window".to_string(),
            formats: {
                let mut formats = HashMap::new();
                formats.insert("json".to_string(), "Screenshot metadata with base64 image data".to_string());
                formats
            },
            examples: vec![
                UsageExample {
                    use_case: "Capture window screenshot".to_string(),
                    command: serde_json::json!({
                        "window_id": "0x4400001"
                    }),
                    output_sample: r#"{"window_id":"0x4400001","path":"/tmp/screenshot.png","size":"800x600","encoding":"png","data":"iVBORw0KGgoAAAANSUhEUgAA..."}"#.to_string(),
                    token_savings: None,
                },
            ],
            ai_tips: vec![
                "Screenshots can be large; store path instead of inline data when possible".to_string(),
            ],
            performance_hints: vec![
                "Screenshot capture is relatively fast".to_string(),
            ],
        })
    }

    /// Generate help for devit_window_get_content
    fn generate_window_get_content_help(&self) -> DevItResult<ToolHelp> {
        Ok(ToolHelp {
            tool_name: "devit_window_get_content".to_string(),
            description: "Extract visible text content from a window using OCR".to_string(),
            formats: {
                let mut formats = HashMap::new();
                formats.insert("json".to_string(), "Extracted text with OCR confidence scores".to_string());
                formats
            },
            examples: vec![
                UsageExample {
                    use_case: "Extract text from window".to_string(),
                    command: serde_json::json!({
                        "window_id": "0x4400001"
                    }),
                    output_sample: r#"{"window_id":"0x4400001","text":"Extracted window content...","confidence":0.95,"lang":"eng"}"#.to_string(),
                    token_savings: None,
                },
            ],
            ai_tips: vec![
                "Check confidence scores for text accuracy".to_string(),
            ],
            performance_hints: vec![
                "OCR processing is slower than screenshot capture".to_string(),
            ],
        })
    }

    /// Generate help for devit_keyboard
    fn generate_keyboard_help(&self) -> DevItResult<ToolHelp> {
        Ok(ToolHelp {
            tool_name: "devit_keyboard".to_string(),
            description: "Send text input and key combinations to the system".to_string(),
            formats: {
                let mut formats = HashMap::new();
                formats.insert("json".to_string(), "Keyboard input result".to_string());
                formats
            },
            examples: vec![UsageExample {
                use_case: "Send keyboard input".to_string(),
                command: serde_json::json!({
                    "actions": [
                        {"type": "text", "text": "Hello World"},
                        {"type": "key", "keys": ["Return"]}
                    ]
                }),
                output_sample: r#"{"actions_executed":2,"success":true,"delay_ms":35}"#.to_string(),
                token_savings: None,
            }],
            ai_tips: vec![
                "Use for automating user interactions".to_string(),
                "Include delays between actions for reliability".to_string(),
            ],
            performance_hints: vec!["Keyboard input is synchronous".to_string()],
        })
    }

    /// Generate help for devit_mouse
    fn generate_mouse_help(&self) -> DevItResult<ToolHelp> {
        Ok(ToolHelp {
            tool_name: "devit_mouse".to_string(),
            description: "Control mouse movement, clicks, and scrolling".to_string(),
            formats: {
                let mut formats = HashMap::new();
                formats.insert("json".to_string(), "Mouse action result".to_string());
                formats
            },
            examples: vec![UsageExample {
                use_case: "Move mouse and click".to_string(),
                command: serde_json::json!({
                    "actions": [
                        {"type": "move", "x": 100, "y": 200},
                        {"type": "click", "button": 1}
                    ]
                }),
                output_sample:
                    r#"{"actions_executed":2,"success":true,"final_position":{"x":100,"y":200}}"#
                        .to_string(),
                token_savings: None,
            }],
            ai_tips: vec!["Get window coordinates first using window_list".to_string()],
            performance_hints: vec!["Mouse operations are real-time and immediate".to_string()],
        })
    }

    /// Generate help for devit_ocr
    fn generate_ocr_help(&self) -> DevItResult<ToolHelp> {
        Ok(ToolHelp {
            tool_name: "devit_ocr".to_string(),
            description: "Extract text from images using Tesseract OCR with preprocessing"
                .to_string(),
            formats: {
                let mut formats = HashMap::new();
                formats.insert(
                    "text".to_string(),
                    "Plain text extraction (inline)".to_string(),
                );
                formats.insert(
                    "tsv".to_string(),
                    "Tab-separated values with coordinates".to_string(),
                );
                formats.insert("hocr".to_string(), "HTML OCR format".to_string());
                formats
            },
            examples: vec![UsageExample {
                use_case: "Extract text from image".to_string(),
                command: serde_json::json!({
                    "path": "screenshot.png",
                    "format": "text",
                    "preprocess": {"grayscale": true, "threshold": 128}
                }),
                output_sample:
                    r#"{"text":"Extracted text from image...","confidence":0.92,"language":"eng"}"#
                        .to_string(),
                token_savings: None,
            }],
            ai_tips: vec![
                "Preprocess images for better OCR accuracy".to_string(),
                "Use TSV format for precise text location data".to_string(),
            ],
            performance_hints: vec!["OCR processing is computationally intensive".to_string()],
        })
    }

    /// Generate help for devit_ocr_alerts
    fn generate_ocr_alerts_help(&self) -> DevItResult<ToolHelp> {
        Ok(ToolHelp {
            tool_name: "devit_ocr_alerts".to_string(),
            description: "Set up regex-based alerts on image content using OCR".to_string(),
            formats: {
                let mut formats = HashMap::new();
                formats.insert("json".to_string(), "Alert results with matches".to_string());
                formats
            },
            examples: vec![
                UsageExample {
                    use_case: "Monitor for error patterns in screenshots".to_string(),
                    command: serde_json::json!({
                        "rules": [
                            {"name": "error_detected", "pattern": "Error|ERROR|error", "severity": "high"}
                        ]
                    }),
                    output_sample: r#"[{"rule":"error_detected","matched":true,"text":"Error found","severity":"high"}]"#.to_string(),
                    token_savings: None,
                },
            ],
            ai_tips: vec![
                "Use for automated screenshot monitoring".to_string(),
            ],
            performance_hints: vec![
                "OCR alerts combine screenshot + OCR + regex".to_string(),
            ],
        })
    }

    /// Generate help for devit_screenshot
    fn generate_screenshot_help(&self) -> DevItResult<ToolHelp> {
        Ok(ToolHelp {
            tool_name: "devit_screenshot".to_string(),
            description: "Capture a full desktop screenshot with compression and inline options".to_string(),
            formats: {
                let mut formats = HashMap::new();
                formats.insert("json".to_string(), "Screenshot metadata with base64-encoded image".to_string());
                formats
            },
            examples: vec![
                UsageExample {
                    use_case: "Capture desktop screenshot".to_string(),
                    command: serde_json::json!({
                        "inline": true,
                        "max_inline_kb": 512,
                        "thumb_width": 480
                    }),
                    output_sample: r#"{"path":"/tmp/screenshot.png","size":"1920x1080","data":"iVBORw0KGgoAAAANSUhEUgAA...","encoded_size_kb":450}"#.to_string(),
                    token_savings: None,
                },
            ],
            ai_tips: vec![
                "Use inline=false for large screenshots to save tokens".to_string(),
            ],
            performance_hints: vec![
                "Screenshots can be large; compress with thumb_width if needed".to_string(),
            ],
        })
    }

    /// Generate help for devit_resource_monitor
    fn generate_resource_monitor_help(&self) -> DevItResult<ToolHelp> {
        Ok(ToolHelp {
            tool_name: "devit_resource_monitor".to_string(),
            description: "Monitor system resources: CPU, RAM, disk, network, GPU".to_string(),
            formats: {
                let mut formats = HashMap::new();
                formats.insert("json".to_string(), "System metrics with current values".to_string());
                formats
            },
            examples: vec![
                UsageExample {
                    use_case: "Monitor all system resources".to_string(),
                    command: serde_json::json!({}),
                    output_sample: r#"{"cpu":{"usage":45.2,"cores":8,"temperature":65},"memory":{"used_gb":8.5,"total_gb":16,"percent":53},"disk":{"root":{"used_gb":250,"total_gb":500,"percent":50}},"gpu":{"nvidia":{"usage":12,"memory":"2GB/8GB"}}}"#.to_string(),
                    token_savings: None,
                },
            ],
            ai_tips: vec![
                "Use to check system capacity before large operations".to_string(),
            ],
            performance_hints: vec![
                "Resource monitoring is lightweight".to_string(),
            ],
        })
    }

    /// Generate help for devit_search_web
    fn generate_search_web_help(&self) -> DevItResult<ToolHelp> {
        Ok(ToolHelp {
            tool_name: "devit_search_web".to_string(),
            description: "Search the web using DuckDuckGo with safety filtering".to_string(),
            formats: {
                let mut formats = HashMap::new();
                formats.insert("json".to_string(), "Search results with URLs and summaries".to_string());
                formats
            },
            examples: vec![
                UsageExample {
                    use_case: "Search for information".to_string(),
                    command: serde_json::json!({
                        "query": "Rust programming language",
                        "max_results": 10,
                        "safe_mode": "moderate"
                    }),
                    output_sample: r#"{"query":"Rust...","results":[{"title":"The Rust Programming Language","url":"https://www.rust-lang.org/","snippet":"Rust is a..."}]}"#.to_string(),
                    token_savings: None,
                },
            ],
            ai_tips: vec![
                "Use safe_mode to filter inappropriate content".to_string(),
            ],
            performance_hints: vec![
                "Web search requires internet connectivity".to_string(),
            ],
        })
    }

    /// Generate help for devit_fetch_url
    fn generate_fetch_url_help(&self) -> DevItResult<ToolHelp> {
        Ok(ToolHelp {
            tool_name: "devit_fetch_url".to_string(),
            description: "Fetch HTML/text content from URLs with timeout and size limits".to_string(),
            formats: {
                let mut formats = HashMap::new();
                formats.insert("json".to_string(), "URL content with metadata".to_string());
                formats
            },
            examples: vec![
                UsageExample {
                    use_case: "Fetch webpage content".to_string(),
                    command: serde_json::json!({
                        "url": "https://example.com",
                        "timeout_ms": 5000,
                        "max_bytes": 100000
                    }),
                    output_sample: r#"{"url":"https://example.com","status":200,"content":"<html>...","size_bytes":45230,"encoding":"utf-8"}"#.to_string(),
                    token_savings: None,
                },
            ],
            ai_tips: vec![
                "Always set max_bytes to prevent memory issues".to_string(),
                "Use reasonable timeouts".to_string(),
            ],
            performance_hints: vec![
                "URL fetching depends on network latency".to_string(),
            ],
        })
    }

    /// Generate help for devit_snapshot
    fn generate_snapshot_help(&self) -> DevItResult<ToolHelp> {
        Ok(ToolHelp {
            tool_name: "devit_snapshot".to_string(),
            description: "Create filesystem snapshots of specified paths".to_string(),
            formats: {
                let mut formats = HashMap::new();
                formats.insert("json".to_string(), "Snapshot metadata".to_string());
                formats
            },
            examples: vec![
                UsageExample {
                    use_case: "Create snapshot".to_string(),
                    command: serde_json::json!({
                        "paths": ["src/", "Cargo.toml"]
                    }),
                    output_sample: r#"{"snapshot_id":"snap_20240101_120000","paths":3,"total_size_kb":2048,"timestamp":"2024-01-01T12:00:00Z"}"#.to_string(),
                    token_savings: None,
                },
            ],
            ai_tips: vec![
                "Use before risky operations".to_string(),
            ],
            performance_hints: vec![
                "Snapshot creation depends on filesystem size".to_string(),
            ],
        })
    }

    /// Generate help for devit_journal_append
    fn generate_journal_append_help(&self) -> DevItResult<ToolHelp> {
        Ok(ToolHelp {
            tool_name: "devit_journal_append".to_string(),
            description: "Append audit entries to DevIt journal".to_string(),
            formats: {
                let mut formats = HashMap::new();
                formats.insert("json".to_string(), "Journal append confirmation".to_string());
                formats
            },
            examples: vec![
                UsageExample {
                    use_case: "Record operation in journal".to_string(),
                    command: serde_json::json!({
                        "operation": "file_patch",
                        "details": {"file": "src/main.rs", "hunks": 3}
                    }),
                    output_sample: r#"{"entry_id":"ent_12345","operation":"file_patch","timestamp":"2024-01-01T12:00:00Z","success":true}"#.to_string(),
                    token_savings: None,
                },
            ],
            ai_tips: vec![
                "Use to maintain audit trails".to_string(),
            ],
            performance_hints: vec![
                "Journal append is fast and atomic".to_string(),
            ],
        })
    }

    /// Generate help for devit_delegate
    fn generate_delegate_help(&self) -> DevItResult<ToolHelp> {
        Ok(ToolHelp {
            tool_name: "devit_delegate".to_string(),
            description: "Delegate tasks to external AI workers with monitoring".to_string(),
            formats: {
                let mut formats = HashMap::new();
                formats.insert("json".to_string(), "Task delegation result".to_string());
                formats
            },
            examples: vec![
                UsageExample {
                    use_case: "Delegate complex task".to_string(),
                    command: serde_json::json!({
                        "goal": "Analyze codebase and generate report",
                        "delegated_to": "claude_code",
                        "timeout": 300
                    }),
                    output_sample: r#"{"task_id":"task_abc123","status":"queued","delegated_to":"claude_code","timeout_secs":300}"#.to_string(),
                    token_savings: None,
                },
            ],
            ai_tips: vec![
                "Use for long-running or complex analyses".to_string(),
            ],
            performance_hints: vec![
                "Delegation happens asynchronously".to_string(),
            ],
        })
    }

    /// Generate help for devit_notify
    fn generate_notify_help(&self) -> DevItResult<ToolHelp> {
        Ok(ToolHelp {
            tool_name: "devit_notify".to_string(),
            description: "Notify orchestrator of task progress or completion".to_string(),
            formats: {
                let mut formats = HashMap::new();
                formats.insert("json".to_string(), "Notification confirmation".to_string());
                formats
            },
            examples: vec![UsageExample {
                use_case: "Report task progress".to_string(),
                command: serde_json::json!({
                    "task_id": "task_abc123",
                    "status": "progress",
                    "summary": "Processing file 3 of 10"
                }),
                output_sample:
                    r#"{"task_id":"task_abc123","status":"progress","acknowledged":true}"#
                        .to_string(),
                token_savings: None,
            }],
            ai_tips: vec!["Send regular progress updates for long tasks".to_string()],
            performance_hints: vec!["Notifications are sent asynchronously".to_string()],
        })
    }

    /// Generate help for devit_orchestration_status
    fn generate_orchestration_status_help(&self) -> DevItResult<ToolHelp> {
        Ok(ToolHelp {
            tool_name: "devit_orchestration_status".to_string(),
            description: "Query status of delegated tasks in orchestration system".to_string(),
            formats: {
                let mut formats = HashMap::new();
                formats.insert("json".to_string(), "Task status overview".to_string());
                formats.insert("compact".to_string(), "Abbreviated status (60% token savings)".to_string());
                formats.insert("table".to_string(), "Tabular format (80% token savings)".to_string());
                formats
            },
            examples: vec![
                UsageExample {
                    use_case: "Check all active tasks".to_string(),
                    command: serde_json::json!({
                        "filter": "active"
                    }),
                    output_sample: r#"[{"task_id":"task_123","status":"in_progress","progress":45,"delegated_to":"claude_code"}]"#.to_string(),
                    token_savings: None,
                },
            ],
            ai_tips: vec![
                "Use 'compact' format to save tokens".to_string(),
            ],
            performance_hints: vec![
                "Status queries are lightweight".to_string(),
            ],
        })
    }

    /// Generate help for devit_task_result
    fn generate_task_result_help(&self) -> DevItResult<ToolHelp> {
        Ok(ToolHelp {
            tool_name: "devit_task_result".to_string(),
            description: "Retrieve detailed result of a delegated task".to_string(),
            formats: {
                let mut formats = HashMap::new();
                formats.insert("json".to_string(), "Complete task result with output".to_string());
                formats
            },
            examples: vec![
                UsageExample {
                    use_case: "Get task result".to_string(),
                    command: serde_json::json!({
                        "task_id": "task_abc123"
                    }),
                    output_sample: r#"{"task_id":"task_abc123","status":"completed","output":"Analysis complete...","duration_ms":54321}"#.to_string(),
                    token_savings: None,
                },
            ],
            ai_tips: vec![
                "Poll this after task completion notification".to_string(),
            ],
            performance_hints: vec![
                "Results are cached temporarily".to_string(),
            ],
        })
    }

    /// Generate help for devit_test_run
    fn generate_test_run_help(&self) -> DevItResult<ToolHelp> {
        Ok(ToolHelp {
            tool_name: "devit_test_run".to_string(),
            description: "Run project tests with framework detection and result formatting".to_string(),
            formats: {
                let mut formats = HashMap::new();
                formats.insert("json".to_string(), "Test results with pass/fail summary".to_string());
                formats
            },
            examples: vec![
                UsageExample {
                    use_case: "Run all tests".to_string(),
                    command: serde_json::json!({
                        "framework": "cargo",
                        "timeout_secs": 300
                    }),
                    output_sample: r#"{"framework":"cargo","tests_run":42,"passed":40,"failed":2,"duration_ms":12345,"failures":[{"name":"test_parser","error":"assertion failed"}]}"#.to_string(),
                    token_savings: None,
                },
            ],
            ai_tips: vec![
                "Always run tests after making code changes".to_string(),
            ],
            performance_hints: vec![
                "Test execution depends on project size and complexity".to_string(),
            ],
        })
    }

    /// Generate help for devit_poll_tasks
    fn generate_poll_tasks_help(&self) -> DevItResult<ToolHelp> {
        Ok(ToolHelp {
            tool_name: "devit_poll_tasks".to_string(),
            description: "Poll for task results from worker queue".to_string(),
            formats: {
                let mut formats = HashMap::new();
                formats.insert("json".to_string(), "Task results with status and output".to_string());
                formats
            },
            examples: vec![
                UsageExample {
                    use_case: "Poll for completed tasks".to_string(),
                    command: serde_json::json!({}),
                    output_sample: r#"{"tasks":[{"task_id":"task_123","status":"completed","result":"..."}],"count":1}"#.to_string(),
                    token_savings: None,
                },
            ],
            ai_tips: vec![
                "Use after delegating tasks to check for completion".to_string(),
                "Poll periodically to retrieve results".to_string(),
            ],
            performance_hints: vec![
                "Lightweight polling operation".to_string(),
            ],
        })
    }

    /// Get overview help for all tools
    pub fn get_all_tools_help(&mut self) -> DevItResult<ToolHelp> {
        Ok(ToolHelp {
            tool_name: "devit_help_all".to_string(),
            description: "Overview of all DevIt MCP tools with optimization guidance".to_string(),
            formats: {
                let mut formats = HashMap::new();
                formats.insert("json".to_string(), "Complete tool listing with descriptions and categories".to_string());
                formats
            },
            examples: vec![
                UsageExample {
                    use_case: "Get all available tools".to_string(),
                    command: serde_json::json!({}),
                    output_sample: r#"{"tools":["devit_file_read","devit_file_read_ext","devit_file_write","devit_directory_list","devit_file_list","devit_file_list_ext","devit_file_search","devit_file_search_ext","devit_project_structure","devit_project_structure_ext","devit_pwd","devit_patch_apply","devit_git_log","devit_git_blame","devit_git_show","devit_git_diff","devit_git_search","devit_exec","devit_shell","devit_ps","devit_kill","devit_window_list","devit_window_send_text","devit_window_focus","devit_window_screenshot","devit_window_get_content","devit_keyboard","devit_mouse","devit_ocr","devit_ocr_alerts","devit_screenshot","devit_resource_monitor","devit_search_web","devit_fetch_url","devit_snapshot","devit_journal_append","devit_delegate","devit_notify","devit_orchestration_status","devit_task_result","devit_poll_tasks","devit_test_run"],"categories":{"file_operations":["devit_file_read","devit_file_read_ext","devit_file_write"],"directory_operations":["devit_directory_list","devit_file_list","devit_file_list_ext"],"search_operations":["devit_file_search","devit_file_search_ext"],"project_analysis":["devit_project_structure","devit_project_structure_ext"],"patching":["devit_patch_apply"],"git_operations":["devit_git_log","devit_git_blame","devit_git_show","devit_git_diff","devit_git_search"],"process_management":["devit_exec","devit_shell","devit_ps","devit_kill"],"window_management":["devit_window_list","devit_window_send_text","devit_window_focus","devit_window_screenshot","devit_window_get_content"],"input_control":["devit_keyboard","devit_mouse"],"media_tools":["devit_ocr","devit_ocr_alerts","devit_screenshot","devit_resource_monitor"],"web_tools":["devit_search_web","devit_fetch_url"],"system_utils":["devit_pwd","devit_snapshot","devit_journal_append","devit_test_run"],"orchestration":["devit_delegate","devit_notify","devit_orchestration_status","devit_task_result","devit_poll_tasks"]}}"#.to_string(),
                    token_savings: None,
                },
            ],
            ai_tips: vec![
                "Always prefer _ext versions for token efficiency (60-80% savings)".to_string(),
                "File operations: use 'compact' or 'table' format unless debugging".to_string(),
                "Git operations: set max_count limits for large repositories".to_string(),
                "Process control: always set timeout to prevent hangs".to_string(),
                "Window/media: check screen size before operations".to_string(),
                "Web tools: handle network timeouts gracefully".to_string(),
                "Orchestration: monitor task progress with devit_notify".to_string(),
                "Combine tools for comprehensive codebase analysis".to_string(),
                "Set appropriate limits to control token usage and memory".to_string(),
            ],
            performance_hints: vec![
                "Extended _ext tools offer 60-80% token savings".to_string(),
                "Batch operations are more efficient with table format".to_string(),
                "Use pagination for large files and directories".to_string(),
                "OCR and screenshot operations are computationally intensive".to_string(),
                "Web operations depend on network latency".to_string(),
                "Window operations should include small delays between actions".to_string(),
                "Git operations can be slow on large repositories".to_string(),
            ],
        })
    }

    /// Calculate token savings for a format compared to JSON baseline
    pub fn calculate_token_savings(&self, json_output: &str, format: &OutputFormat) -> f32 {
        match format {
            OutputFormat::Json => 0.0, // Baseline
            OutputFormat::Compact => {
                let estimated_tokens_json = FormatUtils::estimate_token_count(json_output);
                let estimated_tokens_compact = (estimated_tokens_json as f32 * 0.4) as usize;
                1.0 - (estimated_tokens_compact as f32 / estimated_tokens_json as f32)
            }
            OutputFormat::Table => {
                let estimated_tokens_json = FormatUtils::estimate_token_count(json_output);
                let estimated_tokens_table = (estimated_tokens_json as f32 * 0.2) as usize;
                1.0 - (estimated_tokens_table as f32 / estimated_tokens_json as f32)
            }
            OutputFormat::MessagePack => 0.85, // Future format
        }
    }
}

impl Default for HelpSystem {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_help_system_creation() {
        let help_system = HelpSystem::new();
        assert_eq!(help_system.help_cache.len(), 0);
        assert_eq!(help_system.tool_schemas.len(), 0);
    }

    #[test]
    fn test_file_read_help_generation() {
        let mut help_system = HelpSystem::new();
        let help = help_system.get_tool_help("devit_file_read").unwrap();

        assert_eq!(help.tool_name, "devit_file_read");
        assert!(!help.description.is_empty());
        assert!(help.examples.len() > 0);
        assert!(help.ai_tips.len() > 0);
        assert!(help.formats.contains_key("json"));
    }

    #[test]
    fn test_file_read_ext_help_generation() {
        let mut help_system = HelpSystem::new();
        let help = help_system.get_tool_help("devit_file_read_ext").unwrap();

        assert_eq!(help.tool_name, "devit_file_read_ext");
        assert!(help.formats.contains_key("compact"));
        assert!(help.formats.contains_key("table"));
        assert!(help.examples.iter().any(|ex| ex.token_savings.is_some()));
    }

    #[test]
    fn test_token_savings_calculation() {
        let help_system = HelpSystem::new();
        let json_output =
            r#"{"path": "/test/file.rs", "size": 1024, "content": "example content"}"#;

        let compact_savings =
            help_system.calculate_token_savings(json_output, &OutputFormat::Compact);
        let table_savings = help_system.calculate_token_savings(json_output, &OutputFormat::Table);

        assert!(compact_savings > 0.0 && compact_savings < 1.0);
        assert!(table_savings > compact_savings);
    }

    #[test]
    fn test_help_caching() {
        let mut help_system = HelpSystem::new();

        // First call should generate and cache
        let _help1 = help_system.get_tool_help("devit_file_read").unwrap();
        assert_eq!(help_system.help_cache.len(), 1);

        // Second call should use cache
        let _help2 = help_system.get_tool_help("devit_file_read").unwrap();
        assert_eq!(help_system.help_cache.len(), 1);
    }

    #[test]
    fn test_invalid_tool_help() {
        let mut help_system = HelpSystem::new();
        let result = help_system.get_tool_help("invalid_tool");
        assert!(result.is_err());
    }

    #[test]
    fn test_all_tools_help() {
        let mut help_system = HelpSystem::new();
        let help = help_system.get_all_tools_help().unwrap();

        assert_eq!(help.tool_name, "devit_help_all");
        assert!(!help.description.is_empty());
        assert!(help.ai_tips.len() > 0);
    }
}
