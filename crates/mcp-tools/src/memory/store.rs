// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2025-2026 naskel.com

use std::path::Path;

use rusqlite::{params, Connection};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::errors::internal_error;
use mcp_core::McpResult;

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS memories (
    id          TEXT PRIMARY KEY,
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL,
    category    TEXT NOT NULL DEFAULT 'general',
    format      TEXT NOT NULL DEFAULT 'long',
    content     TEXT NOT NULL,
    tags        TEXT NOT NULL DEFAULT '',
    source      TEXT NOT NULL DEFAULT '',
    importance  INTEGER NOT NULL DEFAULT 5
);

CREATE VIRTUAL TABLE IF NOT EXISTS memories_fts USING fts5(
    content, tags, category,
    content='memories', content_rowid='rowid'
);

CREATE TRIGGER IF NOT EXISTS memories_ai AFTER INSERT ON memories BEGIN
    INSERT INTO memories_fts(rowid, content, tags, category)
    VALUES (new.rowid, new.content, new.tags, new.category);
END;

CREATE TRIGGER IF NOT EXISTS memories_ad AFTER DELETE ON memories BEGIN
    INSERT INTO memories_fts(memories_fts, rowid, content, tags, category)
    VALUES ('delete', old.rowid, old.content, old.tags, old.category);
END;

CREATE TRIGGER IF NOT EXISTS memories_au AFTER UPDATE ON memories BEGIN
    INSERT INTO memories_fts(memories_fts, rowid, content, tags, category)
    VALUES ('delete', old.rowid, old.content, old.tags, old.category);
    INSERT INTO memories_fts(rowid, content, tags, category)
    VALUES (new.rowid, new.content, new.tags, new.category);
END;
"#;

pub fn init_db(conn: &Connection) -> McpResult<()> {
    conn.execute_batch("PRAGMA journal_mode=WAL;")
        .map_err(|e| internal_error(format!("Failed to set WAL mode: {e}")))?;
    conn.execute_batch(SCHEMA)
        .map_err(|e| internal_error(format!("Failed to init memory schema: {e}")))?;
    Ok(())
}

fn now_rfc3339() -> String {
    chrono::Utc::now()
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

pub fn save(
    conn: &Connection,
    content: &str,
    format: &str,
    category: &str,
    tags: &str,
    importance: i64,
    source: &str,
) -> McpResult<String> {
    let id = Uuid::new_v4().to_string();
    let now = now_rfc3339();
    conn.execute(
        "INSERT INTO memories (id, created_at, updated_at, category, format, content, tags, source, importance)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![id, now, now, category, format, content, tags, source, importance],
    )
    .map_err(|e| internal_error(format!("Failed to save memory: {e}")))?;
    Ok(id)
}

pub fn search(
    conn: &Connection,
    query: &str,
    category: Option<&str>,
    tags: Option<&str>,
    limit: i64,
) -> McpResult<Vec<Value>> {
    // Build FTS5 query with optional filters
    let mut sql = String::from(
        "SELECT m.id, m.created_at, m.updated_at, m.category, m.format, m.content, m.tags, m.source, m.importance
         FROM memories_fts f
         JOIN memories m ON m.rowid = f.rowid
         WHERE memories_fts MATCH ?1",
    );
    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = vec![Box::new(query.to_string())];
    let mut idx = 2;

    if let Some(cat) = category {
        sql.push_str(&format!(" AND m.category = ?{idx}"));
        param_values.push(Box::new(cat.to_string()));
        idx += 1;
    }
    if let Some(t) = tags {
        // Filter: any of the comma-separated tags should appear in m.tags
        for tag in t.split(',') {
            let tag = tag.trim();
            if !tag.is_empty() {
                sql.push_str(&format!(" AND m.tags LIKE ?{idx}"));
                param_values.push(Box::new(format!("%{tag}%")));
                idx += 1;
            }
        }
    }
    let _ = idx; // silence unused warning

    sql.push_str(" ORDER BY rank LIMIT ?");
    param_values.push(Box::new(limit));

    let param_refs: Vec<&dyn rusqlite::types::ToSql> = param_values.iter().map(|p| p.as_ref()).collect();

    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| internal_error(format!("Failed to prepare search query: {e}")))?;

    let rows = stmt
        .query_map(param_refs.as_slice(), |row| {
            Ok(json!({
                "id": row.get::<_, String>(0)?,
                "created_at": row.get::<_, String>(1)?,
                "updated_at": row.get::<_, String>(2)?,
                "category": row.get::<_, String>(3)?,
                "format": row.get::<_, String>(4)?,
                "content": row.get::<_, String>(5)?,
                "tags": row.get::<_, String>(6)?,
                "source": row.get::<_, String>(7)?,
                "importance": row.get::<_, i64>(8)?,
            }))
        })
        .map_err(|e| internal_error(format!("Failed to execute search: {e}")))?;

    let mut results = Vec::new();
    for row in rows {
        results.push(row.map_err(|e| internal_error(format!("Row read error: {e}")))?);
    }
    Ok(results)
}

pub fn list(
    conn: &Connection,
    category: Option<&str>,
    tags: Option<&str>,
    limit: i64,
) -> McpResult<Vec<Value>> {
    let mut sql = String::from(
        "SELECT id, created_at, updated_at, category, format, content, tags, source, importance
         FROM memories WHERE 1=1",
    );
    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    let mut idx = 1;

    if let Some(cat) = category {
        sql.push_str(&format!(" AND category = ?{idx}"));
        param_values.push(Box::new(cat.to_string()));
        idx += 1;
    }
    if let Some(t) = tags {
        for tag in t.split(',') {
            let tag = tag.trim();
            if !tag.is_empty() {
                sql.push_str(&format!(" AND tags LIKE ?{idx}"));
                param_values.push(Box::new(format!("%{tag}%")));
                idx += 1;
            }
        }
    }
    let _ = idx;

    sql.push_str(" ORDER BY updated_at DESC LIMIT ?");
    param_values.push(Box::new(limit));

    let param_refs: Vec<&dyn rusqlite::types::ToSql> = param_values.iter().map(|p| p.as_ref()).collect();

    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| internal_error(format!("Failed to prepare list query: {e}")))?;

    let rows = stmt
        .query_map(param_refs.as_slice(), |row| {
            Ok(json!({
                "id": row.get::<_, String>(0)?,
                "created_at": row.get::<_, String>(1)?,
                "updated_at": row.get::<_, String>(2)?,
                "category": row.get::<_, String>(3)?,
                "format": row.get::<_, String>(4)?,
                "content": row.get::<_, String>(5)?,
                "tags": row.get::<_, String>(6)?,
                "source": row.get::<_, String>(7)?,
                "importance": row.get::<_, i64>(8)?,
            }))
        })
        .map_err(|e| internal_error(format!("Failed to execute list: {e}")))?;

    let mut results = Vec::new();
    for row in rows {
        results.push(row.map_err(|e| internal_error(format!("Row read error: {e}")))?);
    }
    Ok(results)
}

pub fn delete(conn: &Connection, id: &str) -> McpResult<bool> {
    let affected = conn
        .execute("DELETE FROM memories WHERE id = ?1", params![id])
        .map_err(|e| internal_error(format!("Failed to delete memory: {e}")))?;
    Ok(affected > 0)
}

pub fn update(
    conn: &Connection,
    id: &str,
    content: Option<&str>,
    category: Option<&str>,
    tags: Option<&str>,
    importance: Option<i64>,
) -> McpResult<bool> {
    let mut sets = Vec::new();
    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    let mut idx = 1;

    if let Some(c) = content {
        sets.push(format!("content = ?{idx}"));
        param_values.push(Box::new(c.to_string()));
        idx += 1;
    }
    if let Some(cat) = category {
        sets.push(format!("category = ?{idx}"));
        param_values.push(Box::new(cat.to_string()));
        idx += 1;
    }
    if let Some(t) = tags {
        sets.push(format!("tags = ?{idx}"));
        param_values.push(Box::new(t.to_string()));
        idx += 1;
    }
    if let Some(imp) = importance {
        sets.push(format!("importance = ?{idx}"));
        param_values.push(Box::new(imp));
        idx += 1;
    }

    if sets.is_empty() {
        return Ok(false);
    }

    sets.push(format!("updated_at = ?{idx}"));
    param_values.push(Box::new(now_rfc3339()));
    idx += 1;

    let sql = format!(
        "UPDATE memories SET {} WHERE id = ?{idx}",
        sets.join(", ")
    );
    param_values.push(Box::new(id.to_string()));

    let param_refs: Vec<&dyn rusqlite::types::ToSql> = param_values.iter().map(|p| p.as_ref()).collect();

    let affected = conn
        .execute(&sql, param_refs.as_slice())
        .map_err(|e| internal_error(format!("Failed to update memory: {e}")))?;
    Ok(affected > 0)
}

pub fn stats(conn: &Connection, db_path: &Path) -> McpResult<Value> {
    let mut stmt = conn
        .prepare("SELECT category, COUNT(*) FROM memories GROUP BY category ORDER BY COUNT(*) DESC")
        .map_err(|e| internal_error(format!("Failed to prepare stats: {e}")))?;

    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })
        .map_err(|e| internal_error(format!("Failed to execute stats: {e}")))?;

    let mut categories = serde_json::Map::new();
    let mut total: i64 = 0;
    for row in rows {
        let (cat, count) = row.map_err(|e| internal_error(format!("Stats row error: {e}")))?;
        total += count;
        categories.insert(cat, json!(count));
    }

    let db_size = std::fs::metadata(db_path)
        .map(|m| m.len())
        .unwrap_or(0);

    Ok(json!({
        "total": total,
        "categories": categories,
        "db_size_bytes": db_size,
        "db_path": db_path.to_string_lossy(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn test_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();
        conn
    }

    #[test]
    fn save_and_search() {
        let conn = test_conn();
        let id = save(&conn, "Rust is great for CLI tools", "long", "fact", "rust,cli", 7, "test").unwrap();
        assert!(!id.is_empty());

        let results = search(&conn, "Rust CLI", None, None, 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0]["content"], "Rust is great for CLI tools");
    }

    #[test]
    fn save_and_list() {
        let conn = test_conn();
        save(&conn, "item 1", "long", "general", "", 5, "test").unwrap();
        save(&conn, "item 2", "long", "decision", "", 8, "test").unwrap();

        let all = list(&conn, None, None, 10).unwrap();
        assert_eq!(all.len(), 2);

        let decisions = list(&conn, Some("decision"), None, 10).unwrap();
        assert_eq!(decisions.len(), 1);
    }

    #[test]
    fn update_and_delete() {
        let conn = test_conn();
        let id = save(&conn, "old content", "long", "general", "", 5, "test").unwrap();

        let updated = update(&conn, &id, Some("new content"), Some("fact"), None, Some(9)).unwrap();
        assert!(updated);

        let items = list(&conn, Some("fact"), None, 10).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["content"], "new content");
        assert_eq!(items[0]["importance"], 9);

        let deleted = delete(&conn, &id).unwrap();
        assert!(deleted);

        let items = list(&conn, None, None, 10).unwrap();
        assert!(items.is_empty());
    }

    #[test]
    fn search_with_category_filter() {
        let conn = test_conn();
        save(&conn, "decision about API design", "long", "decision", "api", 8, "test").unwrap();
        save(&conn, "API performance fact", "long", "fact", "api", 5, "test").unwrap();

        let results = search(&conn, "API", Some("decision"), None, 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0]["category"], "decision");
    }

    #[test]
    fn stats_returns_counts() {
        let conn = test_conn();
        save(&conn, "a", "long", "fact", "", 5, "test").unwrap();
        save(&conn, "b", "long", "fact", "", 5, "test").unwrap();
        save(&conn, "c", "long", "decision", "", 5, "test").unwrap();

        let tmp = tempfile::NamedTempFile::new().unwrap();
        let s = stats(&conn, tmp.path()).unwrap();
        assert_eq!(s["total"], 3);
        assert_eq!(s["categories"]["fact"], 2);
        assert_eq!(s["categories"]["decision"], 1);
    }

    #[test]
    fn delete_nonexistent_returns_false() {
        let conn = test_conn();
        let deleted = delete(&conn, "nonexistent-uuid").unwrap();
        assert!(!deleted);
    }

    #[test]
    fn update_nothing_returns_false() {
        let conn = test_conn();
        let id = save(&conn, "content", "long", "general", "", 5, "test").unwrap();
        let updated = update(&conn, &id, None, None, None, None).unwrap();
        assert!(!updated);
    }
}
