// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2025-2026 naskel.com

//! devit_read_pdf — PDF reader for LLM agents
//!
//! Modes: text (pdftotext), image (pdftoppm → base64), info (pdfinfo).
//! Requires poppler-utils on the system.

use std::sync::Arc;

use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use mcp_core::{McpError, McpResult, McpTool};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::process::Command;
use tracing::info;

use crate::file_read::FileSystemContext;

const MAX_TEXT_BYTES: usize = 256 * 1024; // 256 KB text output max

#[derive(Debug, Deserialize)]
struct ReadPdfParams {
    /// PDF file path (relative to project root)
    path: String,
    /// Mode: text (default), image, info
    #[serde(default = "default_mode")]
    mode: String,
    /// Page number for image mode (1-indexed, default: 1)
    #[serde(default = "default_page")]
    page: u32,
    /// Max width for image mode (default: 1024)
    #[serde(default)]
    max_width: Option<u32>,
    /// Page range for text mode: "1-5", "3", etc. (default: all)
    #[serde(default)]
    pages: Option<String>,
}

fn default_mode() -> String {
    "text".into()
}
fn default_page() -> u32 {
    1
}

pub struct ReadPdfTool {
    context: Arc<FileSystemContext>,
}

impl ReadPdfTool {
    pub fn new(context: Arc<FileSystemContext>) -> Self {
        Self { context }
    }

    fn resolve(&self, path: &str) -> std::path::PathBuf {
        self.context.root().join(path)
    }

    /// Extract text via pdftotext
    async fn extract_text(&self, path: &str, pages: &Option<String>) -> Result<String, McpError> {
        let full_path = self.resolve(path);
        if !full_path.exists() {
            return Err(McpError::InvalidRequest(format!(
                "File not found: {}",
                full_path.display()
            )));
        }

        let mut args: Vec<String> = vec![];

        // Page range: -f first -l last
        if let Some(ref range) = pages {
            if let Some((first, last)) = parse_page_range(range) {
                args.extend([
                    "-f".into(),
                    first.to_string(),
                    "-l".into(),
                    last.to_string(),
                ]);
            }
        }

        args.push(full_path.display().to_string());
        args.push("-".into()); // stdout

        let arg_refs: Vec<&str> = args.iter().map(|s| &**s).collect();

        let output = Command::new("pdftotext")
            .args(&arg_refs)
            .output()
            .await
            .map_err(|e| {
                McpError::ExecutionFailed(format!(
                    "pdftotext not found. Install poppler-utils: {e}"
                ))
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(McpError::ExecutionFailed(format!(
                "pdftotext failed: {}",
                stderr.trim()
            )));
        }

        let text = String::from_utf8_lossy(&output.stdout);
        if text.len() > MAX_TEXT_BYTES {
            Ok(format!(
                "{}\n\n[... truncated at {} KB, use 'pages' param to select range]",
                &text[..MAX_TEXT_BYTES],
                MAX_TEXT_BYTES / 1024
            ))
        } else {
            Ok(text.to_string())
        }
    }

    /// Render page as image via pdftoppm
    async fn render_page(
        &self,
        path: &str,
        page: u32,
        max_width: u32,
    ) -> Result<(Vec<u8>, u32, u32), McpError> {
        let full_path = self.resolve(path);
        if !full_path.exists() {
            return Err(McpError::InvalidRequest(format!(
                "File not found: {}",
                full_path.display()
            )));
        }

        let tmp_prefix = format!("/tmp/devit_pdf_{}", std::process::id());

        let mut args = vec![
            "-png".to_string(),
            "-f".to_string(),
            page.to_string(),
            "-l".to_string(),
            page.to_string(),
            "-singlefile".to_string(),
        ];

        if max_width > 0 {
            args.extend([
                "-scale-to-x".into(),
                max_width.to_string(),
                "-scale-to-y".into(),
                "-1".into(),
            ]);
        }

        args.push(full_path.display().to_string());
        args.push(tmp_prefix.clone());

        let arg_refs: Vec<&str> = args.iter().map(|s| &**s).collect();

        let output = Command::new("pdftoppm")
            .args(&arg_refs)
            .output()
            .await
            .map_err(|e| {
                McpError::ExecutionFailed(format!("pdftoppm not found. Install poppler-utils: {e}"))
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(McpError::ExecutionFailed(format!(
                "pdftoppm failed: {}",
                stderr.trim()
            )));
        }

        let png_path = format!("{}.png", tmp_prefix);
        let bytes = tokio::fs::read(&png_path)
            .await
            .map_err(|e| McpError::ExecutionFailed(format!("Failed to read rendered page: {e}")))?;

        // Get dimensions
        let (w, h) = match image::load_from_memory(&bytes) {
            Ok(img) => {
                let dims = img.dimensions();
                dims
            }
            Err(_) => (0, 0),
        };

        // Cleanup temp file
        let _ = tokio::fs::remove_file(&png_path).await;

        Ok((bytes, w, h))
    }

    /// Get PDF metadata via pdfinfo
    async fn get_info(&self, path: &str) -> Result<String, McpError> {
        let full_path = self.resolve(path);
        if !full_path.exists() {
            return Err(McpError::InvalidRequest(format!(
                "File not found: {}",
                full_path.display()
            )));
        }

        let output = Command::new("pdfinfo")
            .arg(full_path.display().to_string())
            .output()
            .await
            .map_err(|e| {
                McpError::ExecutionFailed(format!("pdfinfo not found. Install poppler-utils: {e}"))
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(McpError::ExecutionFailed(format!(
                "pdfinfo failed: {}",
                stderr.trim()
            )));
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }
}

use image::GenericImageView;

fn parse_page_range(range: &str) -> Option<(u32, u32)> {
    if let Some((a, b)) = range.split_once('-') {
        let first = a.trim().parse::<u32>().ok()?;
        let last = b.trim().parse::<u32>().ok()?;
        Some((first, last))
    } else {
        let page = range.trim().parse::<u32>().ok()?;
        Some((page, page))
    }
}

#[async_trait]
impl McpTool for ReadPdfTool {
    fn name(&self) -> &str {
        "devit_read_pdf"
    }

    fn description(&self) -> &str {
        "Read PDF files. Modes: 'text' extracts text content, 'image' renders a page as PNG \
         for LLM vision, 'info' shows metadata (page count, author, etc.). Requires poppler-utils."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "PDF file path (relative to project root)"
                },
                "mode": {
                    "type": "string",
                    "enum": ["text", "image", "info"],
                    "description": "Extraction mode (default: text)",
                    "default": "text"
                },
                "page": {
                    "type": "integer",
                    "description": "Page number for image mode (1-indexed, default: 1)",
                    "default": 1
                },
                "pages": {
                    "type": "string",
                    "description": "Page range for text mode: '1-5', '3', etc. (default: all)"
                },
                "max_width": {
                    "type": "integer",
                    "description": "Max width in pixels for image mode (default: 1024)",
                    "default": 1024
                }
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, params: Value) -> McpResult<Value> {
        let params: ReadPdfParams = serde_json::from_value(params)
            .map_err(|e| McpError::InvalidRequest(format!("Invalid params: {e}")))?;

        info!(
            target: "devit_mcp_tools",
            "devit_read_pdf | mode={} | path={}",
            params.mode, params.path
        );

        match params.mode.as_str() {
            "text" => {
                let text = self.extract_text(&params.path, &params.pages).await?;
                let line_count = text.lines().count();
                Ok(json!({
                    "content": [{"type": "text", "text": text}],
                    "structuredContent": {
                        "pdf": {
                            "mode": "text",
                            "path": params.path,
                            "lines": line_count,
                            "bytes": text.len()
                        }
                    }
                }))
            }
            "image" => {
                let max_width = params.max_width.unwrap_or(1024);
                let (bytes, w, h) = self
                    .render_page(&params.path, params.page, max_width)
                    .await?;
                let b64 = BASE64.encode(&bytes);
                let size_kb = bytes.len() as f64 / 1024.0;

                let mut content = vec![json!({
                    "type": "text",
                    "text": format!(
                        "PDF page {} — {}x{}px | {:.1} KB",
                        params.page, w, h, size_kb
                    )
                })];
                content.push(json!({
                    "type": "image",
                    "data": b64,
                    "mimeType": "image/png"
                }));

                Ok(json!({
                    "content": content,
                    "structuredContent": {
                        "pdf": {
                            "mode": "image",
                            "path": params.path,
                            "page": params.page,
                            "width": w,
                            "height": h,
                            "bytes": bytes.len()
                        }
                    }
                }))
            }
            "info" => {
                let info = self.get_info(&params.path).await?;
                Ok(json!({
                    "content": [{"type": "text", "text": info}],
                    "structuredContent": {
                        "pdf": {
                            "mode": "info",
                            "path": params.path
                        }
                    }
                }))
            }
            other => Err(McpError::InvalidRequest(format!(
                "Unknown mode '{}'. Valid: text, image, info",
                other
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_page_range_single() {
        assert_eq!(parse_page_range("3"), Some((3, 3)));
    }

    #[test]
    fn test_parse_page_range_range() {
        assert_eq!(parse_page_range("1-5"), Some((1, 5)));
    }

    #[test]
    fn test_parse_page_range_invalid() {
        assert_eq!(parse_page_range("abc"), None);
    }

    #[test]
    fn test_params_defaults() {
        let v = json!({"path": "doc.pdf"});
        let p: ReadPdfParams = serde_json::from_value(v).unwrap();
        assert_eq!(p.path, "doc.pdf");
        assert_eq!(p.mode, "text");
        assert_eq!(p.page, 1);
        assert!(p.pages.is_none());
    }

    #[test]
    fn test_params_image_mode() {
        let v = json!({"path": "spec.pdf", "mode": "image", "page": 3, "max_width": 800});
        let p: ReadPdfParams = serde_json::from_value(v).unwrap();
        assert_eq!(p.mode, "image");
        assert_eq!(p.page, 3);
        assert_eq!(p.max_width.unwrap(), 800);
    }
}
