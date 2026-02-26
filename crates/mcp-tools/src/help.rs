// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2025-2026 naskel.com

use std::sync::Arc;

use async_trait::async_trait;
use devit_cli::core::help_system::{HelpSystem, ToolHelp};
use mcp_core::{McpResult, McpTool};
use serde_json::{json, Value};

use crate::errors::{internal_error, validation_error};
use crate::file_read::FileSystemContext;

/// Tool exposing DevIt CLI help topics to MCP clients.
pub struct HelpTool;

/// Tool providing a static overview of all DevIt MCP tools for AI assistants.
pub struct HelpAllTool;

impl HelpTool {
    pub fn new(_fs_context: Arc<FileSystemContext>) -> Self {
        Self
    }

    fn generate_static_help(&self, topic: &str) -> McpResult<String> {
        let mut help_system = HelpSystem::new();
        if topic == "all" {
            let overview = help_system
                .get_all_tools_help()
                .map_err(|err| internal_error(err.to_string()))?;
            return Ok(render_tool_help(&overview));
        }

        match help_system.get_tool_help(topic) {
            Ok(help) => Ok(render_tool_help(help)),
            Err(_) => Err(validation_error(&format!(
                "Unknown help topic: '{}'. Available topics: {}",
                topic,
                FALLBACK_TOPICS.join(", ")
            ))),
        }
    }
}

#[async_trait]
impl McpTool for HelpTool {
    fn name(&self) -> &str {
        "devit_help"
    }

    fn description(&self) -> &str {
        "Show DevIt CLI help for a specific command or list all commands"
    }

    async fn execute(&self, params: Value) -> McpResult<Value> {
        let topic_label = params
            .get("topic")
            .and_then(Value::as_str)
            .unwrap_or("all")
            .trim()
            .to_string();

        let mut metadata = serde_json::Map::new();
        metadata.insert("topic".into(), Value::String(topic_label.clone()));

        metadata.insert("source".into(), Value::String("static".into()));
        let body = self.generate_static_help(&topic_label)?;

        Ok(json!({
            "content": [{
                "type": "text",
                "text": body
            }],
            "metadata": Value::Object(metadata)
        }))
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "topic": {
                    "type": "string",
                    "description": "Command to show help for (use 'all' to list everything)",
                    "default": "all"
                }
            },
            "additionalProperties": false
        })
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// HelpAllTool - Static overview for AI assistants
// ─────────────────────────────────────────────────────────────────────────────

impl HelpAllTool {
    pub fn new() -> Self {
        Self
    }

    fn generate_all_tools_help(&self) -> String {
        let mut output = String::new();
        output.push_str("# DevIt MCP Tools - Complete Reference\n\n");
        output
            .push_str("This document provides a comprehensive overview of all DevIt MCP tools.\n");
        output.push_str("Use `devit_help` with a specific tool name for detailed help.\n\n");

        Self::append_tool_tables(&mut output);
        Self::append_examples(&mut output);
        Self::append_tips(&mut output);

        output
    }

    fn append_tool_tables(out: &mut String) {
        let sections: &[(&str, &[(&str, &str, &str)])] = &[
            (
                "File Operations",
                &[
                    (
                        "devit_file_read",
                        "Read file content with line numbers",
                        "`path`, `offset`, `limit`",
                    ),
                    (
                        "devit_file_read_ext",
                        "Read with compression (60-80% token savings)",
                        "`path`, `format`: json/compact/table",
                    ),
                    (
                        "devit_file_write",
                        "Write content to files",
                        "`path`, `content`, `mode`: overwrite/append/create_new",
                    ),
                    (
                        "devit_directory_list",
                        "List directory contents",
                        "`path`, `max_depth`, `include_files`",
                    ),
                    (
                        "devit_file_list",
                        "List files with metadata",
                        "`path`, `recursive`",
                    ),
                    (
                        "devit_file_list_ext",
                        "List with compression",
                        "`path`, `format`, `include_patterns`",
                    ),
                    (
                        "devit_file_search",
                        "Regex search in files",
                        "`pattern`, `path`, `context_lines`",
                    ),
                    (
                        "devit_file_search_ext",
                        "Search with compression",
                        "`pattern`, `format`, `max_results`",
                    ),
                    ("devit_project_structure", "Generate project tree", "`path`"),
                    (
                        "devit_project_structure_ext",
                        "Project tree with compression",
                        "`path`, `format`, `max_depth`",
                    ),
                    ("devit_pwd", "Get working directory", "(none)"),
                    (
                        "devit_patch_apply",
                        "Apply unified diff patches",
                        "`diff`, `dry_run`",
                    ),
                ],
            ),
            (
                "Git Operations",
                &[
                    (
                        "devit_git_log",
                        "View commit history",
                        "`max_count`, `path`",
                    ),
                    (
                        "devit_git_blame",
                        "Show line-by-line authorship",
                        "`path`, `line_start`, `line_end`",
                    ),
                    (
                        "devit_git_show",
                        "Show commit details with diff",
                        "`commit`, `path`",
                    ),
                    (
                        "devit_git_diff",
                        "Show diff between refs",
                        "`range`, `path`",
                    ),
                    (
                        "devit_git_search",
                        "Search git history",
                        "`pattern`, `type`: grep/log",
                    ),
                ],
            ),
            (
                "Process Management",
                &[
                    (
                        "devit_exec",
                        "Execute binary with args",
                        "`binary`, `args`, `timeout_secs`, `mode`: foreground/background",
                    ),
                    (
                        "devit_shell",
                        "Execute shell command (bash -c)",
                        "`command`, `timeout_secs`, `background`, `working_dir`",
                    ),
                    ("devit_ps", "List running processes", "`pid` (optional)"),
                    (
                        "devit_kill",
                        "Terminate process",
                        "`pid`, `signal`: TERM/KILL/INT",
                    ),
                ],
            ),
            (
                "Window Management (X11)",
                &[
                    ("devit_window_list", "List all open windows", "(none)"),
                    ("devit_window_focus", "Focus a window", "`window_id`"),
                    (
                        "devit_window_send_text",
                        "Send text to window",
                        "`window_id`, `text`",
                    ),
                    (
                        "devit_window_screenshot",
                        "Capture window screenshot",
                        "`window_id`",
                    ),
                    (
                        "devit_window_get_content",
                        "Extract text via OCR",
                        "`window_id`",
                    ),
                ],
            ),
            (
                "Input Control",
                &[
                    (
                        "devit_keyboard",
                        "Send keyboard input",
                        "`actions`: [{type: text/key, text/keys, delay_ms}]",
                    ),
                    (
                        "devit_mouse",
                        "Control mouse",
                        "`actions`: [{type: move/click/scroll, x, y, button}]",
                    ),
                ],
            ),
            (
                "Media & System",
                &[
                    (
                        "devit_screenshot",
                        "Capture desktop screenshot",
                        "`inline`, `thumb_width`, `max_inline_kb`",
                    ),
                    (
                        "devit_read_image",
                        "Read/download image for LLM vision",
                        "`path` OR `url`, `max_width`, `max_kb`",
                    ),
                    (
                        "devit_read_pdf",
                        "Read PDF (text/image/info)",
                        "`path`, `mode`: text/image/info, `page`, `pages`",
                    ),
                    (
                        "devit_ocr",
                        "Extract text from image (Tesseract)",
                        "`path`, `lang`, `format`: text/tsv/hocr, `preprocess`",
                    ),
                    (
                        "devit_ocr_alerts",
                        "Regex alerts on OCR text",
                        "`rules`: [{name, pattern, severity}]",
                    ),
                    (
                        "devit_resource_monitor",
                        "Monitor CPU/RAM/disk/GPU",
                        "`include_cpu`, `include_memory`, `include_disk`, `include_gpu`",
                    ),
                ],
            ),
            (
                "Local Dev Tools",
                &[
                    (
                        "devit_clipboard",
                        "Read/write system clipboard",
                        "`action`: read/write/clear, `content`, `selection`",
                    ),
                    (
                        "devit_ports",
                        "Show listening ports (ss)",
                        "`proto`: tcp/udp/all, `port`",
                    ),
                    (
                        "devit_docker",
                        "Docker management",
                        "`action`: ps/logs/start/stop/restart/images/inspect, `container`",
                    ),
                    (
                        "devit_db_query",
                        "SQL queries (SQLite/Postgres)",
                        "`query`, `db`, `db_type`, `max_rows`",
                    ),
                    (
                        "devit_archive",
                        "Archive management",
                        "`action`: extract/create/list, `path`, `files`, `format`",
                    ),
                ],
            ),
            (
                "Web Tools",
                &[
                    (
                        "devit_search_web",
                        "Search via DuckDuckGo",
                        "`query`, `max_results`, `safe_mode`",
                    ),
                    (
                        "devit_fetch_url",
                        "Fetch URL content",
                        "`url`, `timeout_ms`, `max_bytes`",
                    ),
                ],
            ),
            (
                "Orchestration",
                &[
                    (
                        "devit_delegate",
                        "Delegate task to AI worker",
                        "`goal`, `delegated_to`, `timeout`, `working_dir`",
                    ),
                    (
                        "devit_notify",
                        "Report task progress",
                        "`task_id`, `status`: completed/failed/progress/blocked, `summary`",
                    ),
                    (
                        "devit_orchestration_status",
                        "Query task status",
                        "`filter`: all/active/completed/failed, `format`",
                    ),
                    ("devit_task_result", "Get task result", "`task_id`"),
                ],
            ),
            (
                "Utility",
                &[
                    (
                        "devit_snapshot",
                        "Create filesystem snapshot",
                        "`paths`: [\"src/\", \"file.rs\"]",
                    ),
                    (
                        "devit_journal_append",
                        "Add audit journal entry",
                        "`operation`, `details`",
                    ),
                    (
                        "devit_test_run",
                        "Run project tests",
                        "`framework`, `timeout_secs`",
                    ),
                    (
                        "devit_help",
                        "Get help for specific tool",
                        "`topic`: tool name or \"all\"",
                    ),
                ],
            ),
            (
                "Claude Desktop Convenience (DEVIT_CLAUDE_DESKTOP=true)",
                &[
                    (
                        "devit_cargo",
                        "Run cargo commands with structured diagnostics",
                        "`subcommand`, `args`, `package`, `timeout_secs`",
                    ),
                    (
                        "devit_git",
                        "Git write operations",
                        "`action`: commit/push/branch/stash/checkout/status",
                    ),
                    (
                        "devit_doctor",
                        "Health check pipeline (check>clippy>test>fmt)",
                        "`checks`, `fail_fast`, `package`",
                    ),
                    (
                        "devit_file_ops",
                        "File management (rename/copy/delete/mkdir)",
                        "`action`, `path`, `from`, `to`, `recursive`",
                    ),
                ],
            ),
        ];

        for (title, tools) in sections {
            out.push_str(&format!("## {}\n\n", title));
            out.push_str("| Tool | Description | Key Parameters |\n");
            out.push_str("|------|-------------|----------------|\n");
            for (name, desc, params) in *tools {
                out.push_str(&format!("| `{}` | {} | {} |\n", name, desc, params));
            }
            out.push('\n');
        }
    }

    fn append_examples(out: &mut String) {
        out.push_str("## Quick Usage Examples\n\n");
        let examples: &[(&str, &str)] = &[
            ("Read a file with token optimization",
             "{\"path\": \"src/main.rs\", \"format\": \"compact\", \"limit\": 100}"),
            ("Search for patterns in code",
             "{\"pattern\": \"TODO|FIXME\", \"path\": \"src\", \"format\": \"table\", \"context_lines\": 1}"),
            ("Run a shell command",
             "{\"command\": \"cargo build --release\", \"timeout_secs\": 300}"),
            ("Take a screenshot and OCR it",
             "// Step 1: Screenshot\n{\"inline\": true, \"thumb_width\": 800}\n// Step 2: OCR the result\n{\"path\": \"/tmp/devit_screenshots/latest.png\", \"lang\": \"eng\"}"),
            ("Read an image (local or URL)",
             "// Local file\n{\"path\": \"docs/architecture.png\", \"max_width\": 800}\n// From URL\n{\"url\": \"https://example.com/diagram.png\", \"max_kb\": 256}"),
            ("Delegate a task to another AI",
             "{\"goal\": \"Analyze src/ for security issues\", \"delegated_to\": \"claude_code\", \"timeout\": 600}"),
        ];
        for (title, json) in examples {
            out.push_str(&format!("### {}\n```json\n{}\n```\n\n", title, json));
        }
    }

    fn append_tips(out: &mut String) {
        out.push_str("## AI Optimization Tips\n\n");
        out.push_str("1. **Use `_ext` tools** - They offer 60-80% token savings with `format: compact` or `table`\n");
        out.push_str("2. **Set limits** - Always use `limit`, `max_count`, `max_results` to control output size\n");
        out.push_str(
            "3. **Use `dry_run`** - For `devit_patch_apply`, always preview before applying\n",
        );
        out.push_str("4. **Background processes** - Use `background: true` for long-running commands, then monitor with `devit_ps`\n");
        out.push_str("5. **Combine tools** - Use `devit_file_search_ext` to find, then `devit_file_read_ext` to read specific sections\n");
    }
}

impl Default for HelpAllTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl McpTool for HelpAllTool {
    fn name(&self) -> &str {
        "devit_help_all"
    }

    fn description(&self) -> &str {
        "Static overview for all DevIt MCP tools"
    }

    async fn execute(&self, params: Value) -> McpResult<Value> {
        let format = params
            .get("format")
            .and_then(Value::as_str)
            .unwrap_or("markdown");

        let body = self.generate_all_tools_help();

        Ok(json!({
            "content": [{
                "type": "text",
                "text": body
            }],
            "metadata": {
                "format": format,
                "tool_count": FALLBACK_TOPICS.len()
            }
        }))
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "format": {
                    "type": "string",
                    "description": "Output format for help content",
                    "enum": ["markdown"],
                    "default": "markdown"
                }
            },
            "additionalProperties": false
        })
    }
}

const FALLBACK_TOPICS: &[&str] = &[
    // File operations
    "devit_file_read",
    "devit_file_read_ext",
    "devit_file_write",
    "devit_directory_list",
    "devit_file_list",
    "devit_file_list_ext",
    "devit_file_search",
    "devit_file_search_ext",
    "devit_project_structure",
    "devit_project_structure_ext",
    "devit_pwd",
    "devit_patch_apply",
    // Git operations
    "devit_git_log",
    "devit_git_blame",
    "devit_git_show",
    "devit_git_diff",
    "devit_git_search",
    // Process management
    "devit_exec",
    "devit_shell",
    "devit_ps",
    "devit_kill",
    // Window management
    "devit_window_list",
    "devit_window_send_text",
    "devit_window_focus",
    "devit_window_screenshot",
    "devit_window_get_content",
    // Input control
    "devit_keyboard",
    "devit_mouse",
    // Media & System
    "devit_ocr",
    "devit_ocr_alerts",
    "devit_screenshot",
    "devit_read_image",
    "devit_read_pdf",
    "devit_resource_monitor",
    "devit_search_web",
    "devit_fetch_url",
    // Local dev tools
    "devit_clipboard",
    "devit_ports",
    "devit_docker",
    "devit_db_query",
    "devit_archive",
    // Orchestration & Utility
    "devit_snapshot",
    "devit_journal_append",
    "devit_delegate",
    "devit_notify",
    "devit_orchestration_status",
    "devit_task_result",
    "devit_test_run",
    // Claude Desktop convenience (DEVIT_CLAUDE_DESKTOP=true)
    "devit_cargo",
    "devit_git",
    "devit_doctor",
    "devit_file_ops",
];

fn render_tool_help(help: &ToolHelp) -> String {
    let mut lines = Vec::new();
    if help.tool_name == "devit_help_all" {
        lines.push("DevIt MCP Tools Overview".to_string());
        lines.push(String::from(""));
    }

    lines.push(format!("{} — {}", help.tool_name, help.description));

    if !help.formats.is_empty() {
        lines.push(String::from(""));
        lines.push(String::from("formats:"));
        for (name, desc) in &help.formats {
            lines.push(format!("  • {}: {}", name, desc));
        }
    }

    if !help.examples.is_empty() {
        lines.push(String::from(""));
        lines.push(String::from("examples:"));
        for example in &help.examples {
            let command = serde_json::to_string_pretty(&example.command)
                .unwrap_or_else(|_| String::from("{}"));
            lines.push(format!("  ▶ {}", example.use_case));
            lines.push(format!("    Commande: {}", command.replace('\n', "\n    ")));
            lines.push(format!(
                "    Sortie: {}",
                example.output_sample.replace('\n', "\n    ")
            ));
            if let Some(savings) = example.token_savings {
                lines.push(format!("    Token savings: {:.0}%", savings * 100.0));
            }
        }
    }

    if !help.ai_tips.is_empty() {
        lines.push(String::from(""));
        lines.push(String::from("AI Optimization Tips:"));
        for tip in &help.ai_tips {
            lines.push(format!("  • {}", tip));
        }
    }

    if !help.performance_hints.is_empty() {
        lines.push(String::from(""));
        lines.push(String::from("Performance Hints:"));
        for hint in &help.performance_hints {
            lines.push(format!("  • {}", hint));
        }
    }

    lines.join("\n")
}
