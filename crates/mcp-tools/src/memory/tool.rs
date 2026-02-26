// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2025-2026 naskel.com

use std::sync::Arc;

use async_trait::async_trait;
use mcp_core::{McpResult, McpTool};
use serde_json::{json, Value};

use super::store;
use super::MemoryContext;
use crate::errors::{missing_param, validation_error};

pub struct MemoryTool {
    context: Arc<MemoryContext>,
}

impl MemoryTool {
    pub fn new(context: Arc<MemoryContext>) -> Self {
        Self { context }
    }
}

#[async_trait]
impl McpTool for MemoryTool {
    fn name(&self) -> &str {
        "devit_memory"
    }

    fn description(&self) -> &str {
        "Persistent memory store for Claude Code. Save decisions, facts, preferences, \
         and context between sessions. Stored per-workspace in .devit/memory.db with \
         FTS5 full-text search. Commands: save, search, list, delete, update, stats."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "enum": ["save", "search", "list", "delete", "update", "stats"],
                    "description": "Operation to perform"
                },
                "content": {
                    "type": "string",
                    "description": "Memory content (save, update)"
                },
                "format": {
                    "type": "string",
                    "enum": ["long", "compact"],
                    "default": "long",
                    "description": "Content format: 'long' (free text) or 'compact' (structured JSON)"
                },
                "category": {
                    "type": "string",
                    "enum": ["decision", "preference", "fact", "context", "bug", "todo", "general"],
                    "default": "general",
                    "description": "Memory category (save, update, search filter, list filter)"
                },
                "tags": {
                    "type": "string",
                    "description": "Comma-separated tags (save, update, search filter, list filter)"
                },
                "importance": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 10,
                    "default": 5,
                    "description": "Priority 1-10 (save, update)"
                },
                "query": {
                    "type": "string",
                    "description": "FTS5 search query (search)"
                },
                "id": {
                    "type": "string",
                    "description": "Memory UUID (delete, update)"
                },
                "limit": {
                    "type": "integer",
                    "default": 20,
                    "description": "Max results (search, list)"
                },
                "source": {
                    "type": "string",
                    "default": "claude_code",
                    "description": "Who created this memory (save)"
                }
            },
            "required": ["command"]
        })
    }

    async fn execute(&self, params: Value) -> McpResult<Value> {
        let command = params
            .get("command")
            .and_then(Value::as_str)
            .ok_or_else(|| missing_param("command", "string"))?;

        match command {
            "save" => self.cmd_save(&params),
            "search" => self.cmd_search(&params),
            "list" => self.cmd_list(&params),
            "delete" => self.cmd_delete(&params),
            "update" => self.cmd_update(&params),
            "stats" => self.cmd_stats(),
            other => Err(validation_error(&format!(
                "Unknown command '{other}'. Valid: save, search, list, delete, update, stats"
            ))),
        }
    }
}

impl MemoryTool {
    fn cmd_save(&self, params: &Value) -> McpResult<Value> {
        let content = params
            .get("content")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| missing_param("content", "string"))?;

        let format = params
            .get("format")
            .and_then(Value::as_str)
            .unwrap_or("long");
        let category = params
            .get("category")
            .and_then(Value::as_str)
            .unwrap_or("general");
        let tags = params
            .get("tags")
            .and_then(Value::as_str)
            .unwrap_or("");
        let importance = params
            .get("importance")
            .and_then(Value::as_i64)
            .unwrap_or(5);
        let source = params
            .get("source")
            .and_then(Value::as_str)
            .unwrap_or("claude_code");

        let id = self.context.with_conn(|conn| {
            store::save(conn, content, format, category, tags, importance, source)
        })?;

        let text = format!(
            "Memory saved.\n  id: {id}\n  category: {category}\n  tags: {tags}\n  importance: {importance}"
        );

        Ok(json!({
            "content": [{"type": "text", "text": text}],
            "structuredContent": {
                "memory": {
                    "command": "save",
                    "id": id,
                    "category": category,
                    "tags": tags,
                    "importance": importance
                }
            }
        }))
    }

    fn cmd_search(&self, params: &Value) -> McpResult<Value> {
        let query = params
            .get("query")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| missing_param("query", "string"))?;

        let category = params.get("category").and_then(Value::as_str);
        let tags = params.get("tags").and_then(Value::as_str);
        let limit = params
            .get("limit")
            .and_then(Value::as_i64)
            .unwrap_or(20);

        let results = self.context.with_conn(|conn| {
            store::search(conn, query, category, tags, limit)
        })?;

        let count = results.len();
        let text = if results.is_empty() {
            format!("No memories found for query: \"{query}\"")
        } else {
            let mut lines = vec![format!("Found {count} memor{} for \"{query}\":\n", if count == 1 { "y" } else { "ies" })];
            for m in &results {
                lines.push(format!(
                    "- [{}] ({}, imp:{}) {}{}",
                    &m["id"].as_str().unwrap_or("?")[..8],
                    m["category"].as_str().unwrap_or("?"),
                    m["importance"],
                    truncate(m["content"].as_str().unwrap_or(""), 120),
                    if m["tags"].as_str().unwrap_or("").is_empty() {
                        String::new()
                    } else {
                        format!("  [{}]", m["tags"].as_str().unwrap_or(""))
                    }
                ));
            }
            lines.join("\n")
        };

        Ok(json!({
            "content": [{"type": "text", "text": text}],
            "structuredContent": {
                "memory": {
                    "command": "search",
                    "query": query,
                    "count": count,
                    "results": results
                }
            }
        }))
    }

    fn cmd_list(&self, params: &Value) -> McpResult<Value> {
        let category = params.get("category").and_then(Value::as_str);
        let tags = params.get("tags").and_then(Value::as_str);
        let limit = params
            .get("limit")
            .and_then(Value::as_i64)
            .unwrap_or(20);

        let results = self.context.with_conn(|conn| {
            store::list(conn, category, tags, limit)
        })?;

        let count = results.len();
        let text = if results.is_empty() {
            "No memories stored yet.".to_string()
        } else {
            let mut lines = vec![format!("{count} memor{} (most recent first):\n", if count == 1 { "y" } else { "ies" })];
            for m in &results {
                lines.push(format!(
                    "- [{}] ({}, imp:{}) {}{}",
                    &m["id"].as_str().unwrap_or("?")[..8],
                    m["category"].as_str().unwrap_or("?"),
                    m["importance"],
                    truncate(m["content"].as_str().unwrap_or(""), 120),
                    if m["tags"].as_str().unwrap_or("").is_empty() {
                        String::new()
                    } else {
                        format!("  [{}]", m["tags"].as_str().unwrap_or(""))
                    }
                ));
            }
            lines.join("\n")
        };

        Ok(json!({
            "content": [{"type": "text", "text": text}],
            "structuredContent": {
                "memory": {
                    "command": "list",
                    "count": count,
                    "results": results
                }
            }
        }))
    }

    fn cmd_delete(&self, params: &Value) -> McpResult<Value> {
        let id = params
            .get("id")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| missing_param("id", "string"))?;

        let deleted = self.context.with_conn(|conn| store::delete(conn, id))?;

        let text = if deleted {
            format!("Memory {id} deleted.")
        } else {
            format!("Memory {id} not found.")
        };

        Ok(json!({
            "content": [{"type": "text", "text": text}],
            "structuredContent": {
                "memory": {
                    "command": "delete",
                    "id": id,
                    "deleted": deleted
                }
            }
        }))
    }

    fn cmd_update(&self, params: &Value) -> McpResult<Value> {
        let id = params
            .get("id")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| missing_param("id", "string"))?;

        let content = params.get("content").and_then(Value::as_str);
        let category = params.get("category").and_then(Value::as_str);
        let tags = params.get("tags").and_then(Value::as_str);
        let importance = params.get("importance").and_then(Value::as_i64);

        let updated = self.context.with_conn(|conn| {
            store::update(conn, id, content, category, tags, importance)
        })?;

        let text = if updated {
            format!("Memory {id} updated.")
        } else {
            format!("Memory {id} not found or no changes provided.")
        };

        Ok(json!({
            "content": [{"type": "text", "text": text}],
            "structuredContent": {
                "memory": {
                    "command": "update",
                    "id": id,
                    "updated": updated
                }
            }
        }))
    }

    fn cmd_stats(&self) -> McpResult<Value> {
        let db_path = self.context.db_path().clone();
        let stats = self.context.with_conn(|conn| store::stats(conn, &db_path))?;

        let total = stats["total"].as_i64().unwrap_or(0);
        let db_size = stats["db_size_bytes"].as_u64().unwrap_or(0);
        let categories = stats["categories"].as_object();

        let mut lines = vec![
            format!("Memory stats:"),
            format!("  Total: {total} memories"),
            format!("  DB size: {} KB", db_size / 1024),
            format!("  DB path: {}", db_path.display()),
        ];

        if let Some(cats) = categories {
            if !cats.is_empty() {
                lines.push("  Categories:".to_string());
                for (cat, count) in cats {
                    lines.push(format!("    {cat}: {count}"));
                }
            }
        }

        Ok(json!({
            "content": [{"type": "text", "text": lines.join("\n")}],
            "structuredContent": {
                "memory": {
                    "command": "stats",
                    "stats": stats
                }
            }
        }))
    }
}

fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        s
    } else {
        // Find a valid char boundary
        let mut end = max;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        &s[..end]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::file_read::FileSystemContext;
    use tempfile::tempdir;

    fn make_tool() -> MemoryTool {
        let dir = tempdir().unwrap();
        // Keep the tempdir so it lives long enough for the test
        let dir_path = dir.keep();
        let file_ctx = Arc::new(FileSystemContext::new(dir_path).unwrap());
        let ctx = Arc::new(MemoryContext::new(file_ctx).unwrap());
        MemoryTool::new(ctx)
    }

    #[tokio::test]
    async fn save_and_search_roundtrip() {
        let tool = make_tool();

        let save_result = tool
            .execute(json!({
                "command": "save",
                "content": "Always use parking_lot instead of std Mutex",
                "category": "decision",
                "tags": "rust,concurrency",
                "importance": 8
            }))
            .await
            .unwrap();

        assert!(save_result["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("Memory saved"));

        let search_result = tool
            .execute(json!({
                "command": "search",
                "query": "parking_lot Mutex"
            }))
            .await
            .unwrap();

        let text = search_result["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("1 memory"));
        assert_eq!(
            search_result["structuredContent"]["memory"]["count"],
            1
        );
    }

    #[tokio::test]
    async fn list_and_stats() {
        let tool = make_tool();

        tool.execute(json!({"command": "save", "content": "fact 1", "category": "fact"}))
            .await
            .unwrap();
        tool.execute(json!({"command": "save", "content": "fact 2", "category": "fact"}))
            .await
            .unwrap();

        let list_result = tool.execute(json!({"command": "list"})).await.unwrap();
        assert_eq!(list_result["structuredContent"]["memory"]["count"], 2);

        let stats_result = tool.execute(json!({"command": "stats"})).await.unwrap();
        assert_eq!(stats_result["structuredContent"]["memory"]["stats"]["total"], 2);
    }

    #[tokio::test]
    async fn delete_flow() {
        let tool = make_tool();

        let save_result = tool
            .execute(json!({"command": "save", "content": "to delete"}))
            .await
            .unwrap();
        let id = save_result["structuredContent"]["memory"]["id"]
            .as_str()
            .unwrap();

        let del_result = tool
            .execute(json!({"command": "delete", "id": id}))
            .await
            .unwrap();
        assert!(del_result["structuredContent"]["memory"]["deleted"]
            .as_bool()
            .unwrap());
    }

    #[tokio::test]
    async fn unknown_command_errors() {
        let tool = make_tool();
        let result = tool
            .execute(json!({"command": "fly"}))
            .await;
        assert!(result.is_err());
    }
}
