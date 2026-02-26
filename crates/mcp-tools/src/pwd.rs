// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2025-2026 naskel.com

use std::sync::Arc;

use async_trait::async_trait;
use mcp_core::{McpResult, McpTool};
use serde_json::{json, Value};

use crate::errors::internal_error;
use crate::file_read::FileSystemContext;

pub struct PwdTool {
    context: Arc<FileSystemContext>,
}

impl PwdTool {
    pub fn new(context: Arc<FileSystemContext>) -> Self {
        Self { context }
    }
}

#[async_trait]
impl McpTool for PwdTool {
    fn name(&self) -> &str {
        "devit_pwd"
    }

    fn description(&self) -> &str {
        "Return the canonical working root used for path resolution"
    }

    async fn execute(&self, _params: Value) -> McpResult<Value> {
        let root = self.context.root();
        let canonical = root.canonicalize().map_err(|err| {
            internal_error(format!("Cannot canonicalize working directory: {err}"))
        })?;

        Ok(json!({
            "content": [{
                "type": "text",
                "text": format!(
                    "📁 Effective working root: {}\n\n🔍 Path resolution enforced via FileSystemContext",
                    canonical.display()
                )
            }],
            "metadata": {
                "working_directory": canonical.to_string_lossy(),
                "configured_root": root.to_string_lossy()
            }
        }))
    }

    fn input_schema(&self) -> Value {
        json!({"type": "object", "properties": {}, "additionalProperties": false})
    }
}
