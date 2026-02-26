// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2025-2026 naskel.com

//! SQLite-based vector store with brute-force search
//!
//! For small-to-medium codebases (< 100k chunks), brute-force cosine similarity
//! is fast enough. We store embeddings as BLOBs and compute similarity in Rust.

use super::VectorStore;
use crate::{ChunkType, CodeChunk, ContextChunk, ContextError, EmbeddedChunk, Result};
use async_trait::async_trait;
use rusqlite::{params, Connection};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;

/// SQLite-based vector store
pub struct SqliteStore {
    conn: Arc<Mutex<Connection>>,
}

impl SqliteStore {
    /// Create or open a SQLite store at the given path
    pub async fn new(db_path: &Path) -> Result<Self> {
        // Ensure parent directory exists
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(db_path)
            .map_err(|e| ContextError::Store(format!("Failed to open SQLite: {}", e)))?;

        // Create tables
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS chunks (
                id TEXT PRIMARY KEY,
                file_path TEXT NOT NULL,
                start_line INTEGER NOT NULL,
                end_line INTEGER NOT NULL,
                content TEXT NOT NULL,
                language TEXT NOT NULL,
                chunk_type TEXT NOT NULL,
                symbol_name TEXT,
                token_count INTEGER NOT NULL,
                file_mtime INTEGER NOT NULL,
                embedding BLOB NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_chunks_file ON chunks(file_path);
            "#,
        )
        .map_err(|e| ContextError::Store(format!("Failed to create tables: {}", e)))?;

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// Compute cosine similarity between two vectors
    fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
        if a.len() != b.len() {
            return 0.0;
        }

        let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
        let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();

        if norm_a == 0.0 || norm_b == 0.0 {
            return 0.0;
        }

        dot / (norm_a * norm_b)
    }

    /// Serialize embedding to bytes
    fn embedding_to_bytes(embedding: &[f32]) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(embedding.len() * 4);
        for val in embedding {
            bytes.extend_from_slice(&val.to_le_bytes());
        }
        bytes
    }

    /// Deserialize bytes to embedding
    fn bytes_to_embedding(bytes: &[u8]) -> Vec<f32> {
        bytes
            .chunks(4)
            .map(|chunk| {
                let arr: [u8; 4] = chunk.try_into().unwrap_or([0; 4]);
                f32::from_le_bytes(arr)
            })
            .collect()
    }
}

#[async_trait]
impl VectorStore for SqliteStore {
    async fn upsert(&self, chunks: Vec<EmbeddedChunk>) -> Result<usize> {
        if chunks.is_empty() {
            return Ok(0);
        }

        let conn = self.conn.lock().await;
        let count = chunks.len();

        for chunk in chunks {
            let embedding_bytes = Self::embedding_to_bytes(&chunk.embedding);

            conn.execute(
                r#"
                INSERT OR REPLACE INTO chunks
                (id, file_path, start_line, end_line, content, language, chunk_type,
                 symbol_name, token_count, file_mtime, embedding)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
                "#,
                params![
                    chunk.chunk.id,
                    chunk.chunk.file_path.to_string_lossy().to_string(),
                    chunk.chunk.start_line as i64,
                    chunk.chunk.end_line as i64,
                    chunk.chunk.content,
                    chunk.chunk.language,
                    chunk.chunk.chunk_type.as_str(),
                    chunk.chunk.symbol_name,
                    chunk.chunk.token_count as i64,
                    chunk.chunk.file_mtime as i64,
                    embedding_bytes,
                ],
            )
            .map_err(|e| ContextError::Store(format!("Insert failed: {}", e)))?;
        }

        Ok(count)
    }

    async fn search(
        &self,
        embedding: &[f32],
        top_k: usize,
        threshold: f32,
    ) -> Result<Vec<ContextChunk>> {
        let conn = self.conn.lock().await;

        // Fetch all chunks with embeddings (for brute-force search)
        let mut stmt = conn
            .prepare(
                r#"
                SELECT id, file_path, start_line, end_line, content, language, chunk_type,
                       symbol_name, token_count, file_mtime, embedding
                FROM chunks
                "#,
            )
            .map_err(|e| ContextError::Store(format!("Query failed: {}", e)))?;

        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, Option<String>>(7)?,
                    row.get::<_, i64>(8)?,
                    row.get::<_, i64>(9)?,
                    row.get::<_, Vec<u8>>(10)?,
                ))
            })
            .map_err(|e| ContextError::Store(format!("Query failed: {}", e)))?;

        // Compute similarities and collect results
        let mut results: Vec<(ContextChunk, f32)> = Vec::new();

        for row in rows {
            let (
                id,
                file_path,
                start_line,
                end_line,
                content,
                language,
                chunk_type,
                symbol_name,
                token_count,
                file_mtime,
                emb_bytes,
            ) = row.map_err(|e| ContextError::Store(format!("Row error: {}", e)))?;

            let stored_embedding = Self::bytes_to_embedding(&emb_bytes);
            let score = Self::cosine_similarity(embedding, &stored_embedding);

            if score >= threshold {
                let chunk_type_enum = match chunk_type.as_str() {
                    "function" => ChunkType::Function,
                    "class" => ChunkType::Class,
                    "module" => ChunkType::Module,
                    "imports" => ChunkType::Imports,
                    "constants" => ChunkType::Constants,
                    "typedef" => ChunkType::TypeDef,
                    "documentation" => ChunkType::Documentation,
                    _ => ChunkType::Block,
                };

                let chunk = CodeChunk {
                    id,
                    file_path: PathBuf::from(file_path),
                    start_line: start_line as usize,
                    end_line: end_line as usize,
                    content,
                    language,
                    chunk_type: chunk_type_enum,
                    symbol_name,
                    token_count: token_count as usize,
                    file_mtime: file_mtime as u64,
                };

                results.push((ContextChunk { chunk, score }, score));
            }
        }

        // Sort by score descending
        results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        // Take top_k
        Ok(results.into_iter().take(top_k).map(|(c, _)| c).collect())
    }

    async fn get_by_files(&self, paths: &[PathBuf]) -> Result<Vec<ContextChunk>> {
        if paths.is_empty() {
            return Ok(vec![]);
        }

        let conn = self.conn.lock().await;

        let placeholders = paths.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let query = format!(
            r#"
            SELECT id, file_path, start_line, end_line, content, language, chunk_type,
                   symbol_name, token_count, file_mtime
            FROM chunks
            WHERE file_path IN ({})
            ORDER BY file_path, start_line
            "#,
            placeholders
        );

        let mut stmt = conn
            .prepare(&query)
            .map_err(|e| ContextError::Store(format!("Query failed: {}", e)))?;

        let path_strs: Vec<String> = paths
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect();
        let params: Vec<&dyn rusqlite::ToSql> = path_strs
            .iter()
            .map(|s| s as &dyn rusqlite::ToSql)
            .collect();

        let rows = stmt
            .query_map(params.as_slice(), |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, Option<String>>(7)?,
                    row.get::<_, i64>(8)?,
                    row.get::<_, i64>(9)?,
                ))
            })
            .map_err(|e| ContextError::Store(format!("Query failed: {}", e)))?;

        let mut results = Vec::new();
        for row in rows {
            let (
                id,
                file_path,
                start_line,
                end_line,
                content,
                language,
                chunk_type,
                symbol_name,
                token_count,
                file_mtime,
            ) = row.map_err(|e| ContextError::Store(format!("Row error: {}", e)))?;

            let chunk_type_enum = match chunk_type.as_str() {
                "function" => ChunkType::Function,
                "class" => ChunkType::Class,
                "module" => ChunkType::Module,
                "imports" => ChunkType::Imports,
                "constants" => ChunkType::Constants,
                "typedef" => ChunkType::TypeDef,
                "documentation" => ChunkType::Documentation,
                _ => ChunkType::Block,
            };

            let chunk = CodeChunk {
                id,
                file_path: PathBuf::from(file_path),
                start_line: start_line as usize,
                end_line: end_line as usize,
                content,
                language,
                chunk_type: chunk_type_enum,
                symbol_name,
                token_count: token_count as usize,
                file_mtime: file_mtime as u64,
            };

            results.push(ContextChunk { chunk, score: 1.0 });
        }

        Ok(results)
    }

    async fn delete_by_file(&self, path: &Path) -> Result<()> {
        let conn = self.conn.lock().await;
        let path_str = path.to_string_lossy().to_string();

        conn.execute("DELETE FROM chunks WHERE file_path = ?1", params![path_str])
            .map_err(|e| ContextError::Store(format!("Delete failed: {}", e)))?;

        Ok(())
    }

    async fn count(&self) -> Result<usize> {
        let conn = self.conn.lock().await;

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM chunks", [], |row| row.get(0))
            .map_err(|e| ContextError::Store(format!("Count failed: {}", e)))?;

        Ok(count as usize)
    }

    async fn clear(&self) -> Result<()> {
        let conn = self.conn.lock().await;
        conn.execute("DELETE FROM chunks", [])
            .map_err(|e| ContextError::Store(format!("Clear failed: {}", e)))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_cosine_similarity() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![1.0, 0.0, 0.0];
        let c = vec![0.0, 1.0, 0.0];

        assert!((SqliteStore::cosine_similarity(&a, &b) - 1.0).abs() < 0.001);
        assert!((SqliteStore::cosine_similarity(&a, &c) - 0.0).abs() < 0.001);
    }

    #[tokio::test]
    async fn test_embedding_serialization() {
        let embedding = vec![1.0, 2.0, 3.0, 4.0];
        let bytes = SqliteStore::embedding_to_bytes(&embedding);
        let restored = SqliteStore::bytes_to_embedding(&bytes);

        assert_eq!(embedding, restored);
    }

    #[tokio::test]
    async fn test_store_operations() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.db");

        let store = SqliteStore::new(&db_path).await.unwrap();

        // Insert a chunk
        let chunk = EmbeddedChunk {
            chunk: CodeChunk {
                id: "test-1".to_string(),
                file_path: PathBuf::from("src/main.rs"),
                start_line: 1,
                end_line: 10,
                content: "fn main() {}".to_string(),
                language: "rust".to_string(),
                chunk_type: ChunkType::Function,
                symbol_name: Some("main".to_string()),
                token_count: 5,
                file_mtime: 12345,
            },
            embedding: vec![1.0, 0.0, 0.0],
        };

        let count = store.upsert(vec![chunk]).await.unwrap();
        assert_eq!(count, 1);

        // Search
        let results = store.search(&[1.0, 0.0, 0.0], 10, 0.5).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].chunk.id, "test-1");

        // Count
        assert_eq!(store.count().await.unwrap(), 1);

        // Delete
        store
            .delete_by_file(Path::new("src/main.rs"))
            .await
            .unwrap();
        assert_eq!(store.count().await.unwrap(), 0);
    }
}
