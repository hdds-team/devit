// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2025-2026 naskel.com

mod soul;
pub mod store;
mod tool;

pub use soul::SoulTool;
pub use tool::MemoryTool;

use std::path::PathBuf;
use std::sync::Arc;

use mcp_core::McpResult;
use parking_lot::Mutex;
use rusqlite::Connection;

use crate::errors::{internal_error, io_error};
use crate::file_read::FileSystemContext;

pub struct MemoryContext {
    db_path: PathBuf,
    conn: Mutex<Connection>,
    #[allow(dead_code)]
    workspace_root: PathBuf,
}

impl MemoryContext {
    pub fn new(file_context: Arc<FileSystemContext>) -> McpResult<Self> {
        let workspace_root = file_context.root().to_path_buf();
        let devit_dir = workspace_root.join(".devit");
        std::fs::create_dir_all(&devit_dir).map_err(|e| {
            io_error("create .devit directory", Some(&devit_dir), e.to_string())
        })?;

        let db_path = devit_dir.join("memory.db");
        let conn = Connection::open(&db_path).map_err(|e| {
            internal_error(format!("Failed to open memory database: {e}"))
        })?;

        store::init_db(&conn)?;

        Ok(Self {
            db_path,
            conn: Mutex::new(conn),
            workspace_root,
        })
    }

    pub fn db_path(&self) -> &PathBuf {
        &self.db_path
    }

    pub fn with_conn<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&Connection) -> R,
    {
        let conn = self.conn.lock();
        f(&conn)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn context_creates_db() {
        let dir = tempdir().unwrap();
        let file_ctx = Arc::new(FileSystemContext::new(dir.path().to_path_buf()).unwrap());
        let ctx = MemoryContext::new(file_ctx).unwrap();
        assert!(ctx.db_path().exists());
    }
}
