// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2025-2026 naskel.com

//! devit_clipboard — Clipboard read/write via xclip
//!
//! Actions: read, write, clear.
//! Requires xclip on the system.

use async_trait::async_trait;
use mcp_core::{McpError, McpResult, McpTool};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::process::Command;
use tracing::info;

const MAX_CLIPBOARD_READ: usize = 64 * 1024; // 64 KB

#[derive(Debug, Deserialize)]
struct ClipboardParams {
    /// Action: read, write, clear
    action: String,
    /// Content to write (for write action)
    #[serde(default)]
    content: Option<String>,
    /// Selection: clipboard (default), primary
    #[serde(default = "default_selection")]
    selection: String,
}

fn default_selection() -> String {
    "clipboard".into()
}

pub struct ClipboardTool;

impl ClipboardTool {
    pub fn new() -> Self {
        Self
    }

    async fn read_clipboard(&self, selection: &str) -> Result<String, McpError> {
        let output = Command::new("xclip")
            .args(["-selection", selection, "-o"])
            .output()
            .await
            .map_err(|e| {
                McpError::ExecutionFailed(format!("xclip not found. Install xclip: {e}"))
            })?;

        if !output.status.success() {
            // Empty clipboard is not an error
            let stderr = String::from_utf8_lossy(&output.stderr);
            if stderr.contains("target") || output.stdout.is_empty() {
                return Ok(String::new());
            }
            return Err(McpError::ExecutionFailed(format!(
                "xclip read failed: {}",
                stderr.trim()
            )));
        }

        let text = String::from_utf8_lossy(&output.stdout);
        if text.len() > MAX_CLIPBOARD_READ {
            Ok(format!(
                "{}\n[... clipboard truncated at {} KB]",
                &text[..MAX_CLIPBOARD_READ],
                MAX_CLIPBOARD_READ / 1024
            ))
        } else {
            Ok(text.to_string())
        }
    }

    async fn write_clipboard(&self, content: &str, selection: &str) -> Result<(), McpError> {
        use tokio::io::AsyncWriteExt;

        let mut child = Command::new("xclip")
            .args(["-selection", selection, "-i"])
            .stdin(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| {
                McpError::ExecutionFailed(format!("xclip not found. Install xclip: {e}"))
            })?;

        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(content.as_bytes())
                .await
                .map_err(|e| McpError::ExecutionFailed(format!("Failed to write to xclip: {e}")))?;
        }

        let status = child
            .wait()
            .await
            .map_err(|e| McpError::ExecutionFailed(format!("xclip wait failed: {e}")))?;

        if !status.success() {
            return Err(McpError::ExecutionFailed("xclip write failed".into()));
        }

        Ok(())
    }
}

#[async_trait]
impl McpTool for ClipboardTool {
    fn name(&self) -> &str {
        "devit_clipboard"
    }

    fn description(&self) -> &str {
        "Read/write system clipboard. Actions: 'read' gets clipboard content, \
         'write' sets it, 'clear' empties it. Requires xclip."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["read", "write", "clear"],
                    "description": "Clipboard action"
                },
                "content": {
                    "type": "string",
                    "description": "Content to write (for write action)"
                },
                "selection": {
                    "type": "string",
                    "enum": ["clipboard", "primary"],
                    "description": "X11 selection (default: clipboard)",
                    "default": "clipboard"
                }
            },
            "required": ["action"]
        })
    }

    async fn execute(&self, params: Value) -> McpResult<Value> {
        let params: ClipboardParams = serde_json::from_value(params)
            .map_err(|e| McpError::InvalidRequest(format!("Invalid params: {e}")))?;

        info!(
            target: "devit_mcp_tools",
            "devit_clipboard | action={} | selection={}",
            params.action, params.selection
        );

        match params.action.as_str() {
            "read" => {
                let text = self.read_clipboard(&params.selection).await?;
                let len = text.len();
                Ok(json!({
                    "content": [{"type": "text", "text": if text.is_empty() { "(clipboard empty)".to_string() } else { text }}],
                    "structuredContent": {"clipboard": {"action": "read", "bytes": len, "empty": len == 0}}
                }))
            }
            "write" => {
                let content = params.content.as_deref().ok_or_else(|| {
                    McpError::InvalidRequest("'content' required for write action".into())
                })?;
                self.write_clipboard(content, &params.selection).await?;
                Ok(json!({
                    "content": [{"type": "text", "text": format!("Clipboard set ({} bytes)", content.len())}],
                    "structuredContent": {"clipboard": {"action": "write", "bytes": content.len()}}
                }))
            }
            "clear" => {
                self.write_clipboard("", &params.selection).await?;
                Ok(json!({
                    "content": [{"type": "text", "text": "Clipboard cleared"}],
                    "structuredContent": {"clipboard": {"action": "clear"}}
                }))
            }
            other => Err(McpError::InvalidRequest(format!(
                "Unknown action '{}'. Valid: read, write, clear",
                other
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_params_read() {
        let v = json!({"action": "read"});
        let p: ClipboardParams = serde_json::from_value(v).unwrap();
        assert_eq!(p.action, "read");
        assert_eq!(p.selection, "clipboard");
    }

    #[test]
    fn test_params_write() {
        let v = json!({"action": "write", "content": "hello", "selection": "primary"});
        let p: ClipboardParams = serde_json::from_value(v).unwrap();
        assert_eq!(p.action, "write");
        assert_eq!(p.content.unwrap(), "hello");
        assert_eq!(p.selection, "primary");
    }
}
