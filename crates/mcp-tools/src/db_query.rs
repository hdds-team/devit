// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2025-2026 naskel.com

//! devit_db_query — Database query tool (SQLite + PostgreSQL)
//!
//! Execute SQL queries via CLI tools (sqlite3, psql).
//! Returns structured results in JSON mode.

use std::sync::Arc;

use async_trait::async_trait;
use mcp_core::{McpError, McpResult, McpTool};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::process::Command;
use tracing::info;

use crate::file_read::FileSystemContext;

const MAX_ROWS: u32 = 500;
const QUERY_TIMEOUT_SECS: u64 = 30;

#[derive(Debug, Deserialize)]
struct DbQueryParams {
    /// SQL query to execute
    query: String,
    /// Database type: sqlite (default), postgres
    #[serde(default = "default_db_type")]
    db_type: String,
    /// SQLite: file path (relative). Postgres: connection string.
    db: String,
    /// Max rows to return (default: 500)
    #[serde(default)]
    max_rows: Option<u32>,
}

fn default_db_type() -> String {
    "sqlite".into()
}

pub struct DbQueryTool {
    context: Arc<FileSystemContext>,
}

impl DbQueryTool {
    pub fn new(context: Arc<FileSystemContext>) -> Self {
        Self { context }
    }

    async fn query_sqlite(
        &self,
        db_path: &str,
        query: &str,
        max_rows: u32,
    ) -> Result<(String, usize), McpError> {
        let full_path = self.context.root().join(db_path);
        if !full_path.exists() {
            return Err(McpError::InvalidRequest(format!(
                "SQLite database not found: {}",
                full_path.display()
            )));
        }

        // Use JSON mode for structured output
        let limited_query = if query.to_lowercase().contains("limit") {
            query.to_string()
        } else {
            format!("{} LIMIT {}", query.trim_end_matches(';'), max_rows)
        };

        let output = tokio::time::timeout(
            std::time::Duration::from_secs(QUERY_TIMEOUT_SECS),
            Command::new("sqlite3")
                .args(["-json", &full_path.display().to_string(), &limited_query])
                .output(),
        )
        .await
        .map_err(|_| McpError::ExecutionFailed("Query timeout".into()))?
        .map_err(|e| {
            McpError::ExecutionFailed(format!("sqlite3 not found. Install sqlite3: {e}"))
        })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(McpError::ExecutionFailed(format!(
                "SQLite error: {}",
                stderr.trim()
            )));
        }

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let row_count = if stdout.trim().is_empty() {
            0
        } else {
            // Count JSON array elements
            serde_json::from_str::<Vec<Value>>(&stdout)
                .map(|v| v.len())
                .unwrap_or(0)
        };

        Ok((stdout, row_count))
    }

    async fn query_postgres(
        &self,
        connstr: &str,
        query: &str,
        max_rows: u32,
    ) -> Result<(String, usize), McpError> {
        let limited_query = if query.to_lowercase().contains("limit") {
            query.to_string()
        } else {
            format!("{} LIMIT {}", query.trim_end_matches(';'), max_rows)
        };

        let output = tokio::time::timeout(
            std::time::Duration::from_secs(QUERY_TIMEOUT_SECS),
            Command::new("psql")
                .args([
                    connstr,
                    "-c",
                    &limited_query,
                    "--no-align",
                    "--tuples-only",
                    "--csv",
                ])
                .env("PGCONNECT_TIMEOUT", "5")
                .output(),
        )
        .await
        .map_err(|_| McpError::ExecutionFailed("Query timeout".into()))?
        .map_err(|e| {
            McpError::ExecutionFailed(format!("psql not found. Install postgresql-client: {e}"))
        })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(McpError::ExecutionFailed(format!(
                "PostgreSQL error: {}",
                stderr.trim()
            )));
        }

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let row_count = stdout.lines().count();

        Ok((stdout, row_count))
    }
}

#[async_trait]
impl McpTool for DbQueryTool {
    fn name(&self) -> &str {
        "devit_db_query"
    }

    fn description(&self) -> &str {
        "Execute SQL queries on SQLite or PostgreSQL databases. \
         SQLite uses file path, Postgres uses connection string. \
         Results in JSON (SQLite) or CSV (Postgres). Read-only recommended."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "SQL query to execute"
                },
                "db_type": {
                    "type": "string",
                    "enum": ["sqlite", "postgres"],
                    "description": "Database type (default: sqlite)",
                    "default": "sqlite"
                },
                "db": {
                    "type": "string",
                    "description": "SQLite: file path (relative). Postgres: connection string (e.g. 'postgresql://user:pass@localhost/db')"
                },
                "max_rows": {
                    "type": "integer",
                    "description": "Max rows to return (default: 500)",
                    "default": 500
                }
            },
            "required": ["query", "db"]
        })
    }

    async fn execute(&self, params: Value) -> McpResult<Value> {
        let params: DbQueryParams = serde_json::from_value(params)
            .map_err(|e| McpError::InvalidRequest(format!("Invalid params: {e}")))?;

        let max_rows = params.max_rows.unwrap_or(MAX_ROWS).min(MAX_ROWS);

        info!(
            target: "devit_mcp_tools",
            "devit_db_query | db_type={} | db={} | query_len={}",
            params.db_type, params.db, params.query.len()
        );

        let (result, row_count) = match params.db_type.as_str() {
            "sqlite" => {
                self.query_sqlite(&params.db, &params.query, max_rows)
                    .await?
            }
            "postgres" => {
                self.query_postgres(&params.db, &params.query, max_rows)
                    .await?
            }
            other => {
                return Err(McpError::InvalidRequest(format!(
                    "Unknown db_type '{}'. Valid: sqlite, postgres",
                    other
                )));
            }
        };

        let display = if result.trim().is_empty() {
            "(no results)".to_string()
        } else if result.len() > 32 * 1024 {
            format!(
                "{}\n[... truncated, {} rows]",
                &result[..32 * 1024],
                row_count
            )
        } else {
            result.clone()
        };

        Ok(json!({
            "content": [{"type": "text", "text": display}],
            "structuredContent": {
                "db": {
                    "db_type": params.db_type,
                    "rows": row_count,
                    "query": params.query,
                    "result_bytes": result.len()
                }
            }
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_params_sqlite() {
        let v = json!({"query": "SELECT * FROM users", "db": "data/app.db"});
        let p: DbQueryParams = serde_json::from_value(v).unwrap();
        assert_eq!(p.db_type, "sqlite");
        assert_eq!(p.db, "data/app.db");
        assert!(p.max_rows.is_none());
    }

    #[test]
    fn test_params_postgres() {
        let v = json!({
            "query": "SELECT 1",
            "db_type": "postgres",
            "db": "postgresql://localhost/test",
            "max_rows": 100
        });
        let p: DbQueryParams = serde_json::from_value(v).unwrap();
        assert_eq!(p.db_type, "postgres");
        assert_eq!(p.max_rows.unwrap(), 100);
    }
}
