// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2025-2026 naskel.com

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use mcp_core::{McpError, McpResult, McpTool};
use serde_json::{json, Value};

#[derive(Debug)]
pub struct TestRunContext {
    _root: PathBuf,
}

impl TestRunContext {
    pub fn new(root: PathBuf) -> McpResult<Self> {
        Ok(Self { _root: root })
    }
}

#[derive(Debug)]
pub struct TestRunTool {
    _ctx: Arc<TestRunContext>,
}

impl TestRunTool {
    pub fn new(ctx: Arc<TestRunContext>) -> Self {
        Self { _ctx: ctx }
    }
}

#[async_trait]
impl McpTool for TestRunTool {
    fn name(&self) -> &str {
        "devit_test_run"
    }

    fn description(&self) -> &str {
        "Run project tests (placeholder – disabled in this build)"
    }

    async fn execute(&self, _params: Value) -> McpResult<Value> {
        Err(McpError::ExecutionFailed(
            "test runner not available in this build".to_string(),
        ))
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "framework": {"type": "string"},
                "timeout_secs": {"type": "integer", "minimum": 1}
            },
            "additionalProperties": true
        })
    }
}
