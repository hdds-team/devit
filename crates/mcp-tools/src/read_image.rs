// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2025-2026 naskel.com

//! devit_read_image — Read/download images and return as MCP image content
//!
//! Sources: local file path OR URL download.
//! Returns base64-encoded image with optional thumbnail resize.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use image::codecs::png::PngEncoder;
use image::{imageops::FilterType, ColorType, DynamicImage, GenericImageView, ImageEncoder};
use mcp_core::{McpError, McpResult, McpTool};
use reqwest::Client;
use serde::Deserialize;
use serde_json::{json, Value};
use tracing::info;

use crate::file_read::FileSystemContext;

const DEFAULT_MAX_KB: u64 = 512;
const DEFAULT_MAX_WIDTH: u32 = 1024;
const DOWNLOAD_TIMEOUT_SECS: u64 = 30;
const MAX_DOWNLOAD_BYTES: usize = 10 * 1024 * 1024; // 10 MB

#[derive(Debug, Deserialize)]
struct ReadImageParams {
    /// Local file path (relative to project root)
    #[serde(default)]
    path: Option<String>,
    /// URL to download image from
    #[serde(default)]
    url: Option<String>,
    /// Max width for resize (default: 1024px). 0 = no resize.
    #[serde(default)]
    max_width: Option<u32>,
    /// Max base64 budget in KB (default: 512). Image is resized to fit.
    #[serde(default)]
    max_kb: Option<u64>,
}

pub struct ReadImageTool {
    context: Arc<FileSystemContext>,
}

impl ReadImageTool {
    pub fn new(context: Arc<FileSystemContext>) -> Self {
        Self { context }
    }

    /// Detect MIME type from extension
    fn mime_from_ext(ext: &str) -> Option<&'static str> {
        match ext.to_lowercase().as_str() {
            "png" => Some("image/png"),
            "jpg" | "jpeg" => Some("image/jpeg"),
            "gif" => Some("image/gif"),
            "webp" => Some("image/webp"),
            "bmp" => Some("image/bmp"),
            "svg" => Some("image/svg+xml"),
            "ico" => Some("image/x-icon"),
            "tiff" | "tif" => Some("image/tiff"),
            _ => None,
        }
    }

    /// Detect MIME from bytes magic
    fn mime_from_magic(bytes: &[u8]) -> &'static str {
        if bytes.len() >= 8 {
            if bytes.starts_with(b"\x89PNG") {
                return "image/png";
            }
            if bytes.starts_with(b"\xFF\xD8\xFF") {
                return "image/jpeg";
            }
            if bytes.starts_with(b"GIF8") {
                return "image/gif";
            }
            if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
                return "image/webp";
            }
            if bytes.starts_with(b"BM") {
                return "image/bmp";
            }
        }
        "image/png" // fallback
    }

    /// Read image bytes from local path
    async fn read_local(&self, rel_path: &str) -> Result<(Vec<u8>, String), McpError> {
        let full_path = self.context.root().join(rel_path);
        if !full_path.exists() {
            return Err(McpError::InvalidRequest(format!(
                "File not found: {}",
                full_path.display()
            )));
        }

        let bytes = tokio::fs::read(&full_path)
            .await
            .map_err(|e| McpError::ExecutionFailed(format!("Failed to read file: {e}")))?;

        // Detect mime from extension, fallback to magic bytes
        let mime = full_path
            .extension()
            .and_then(|e| e.to_str())
            .and_then(Self::mime_from_ext)
            .unwrap_or_else(|| Self::mime_from_magic(&bytes));

        Ok((bytes, mime.to_string()))
    }

    /// Download image from URL
    async fn download_url(&self, url: &str) -> Result<(Vec<u8>, String), McpError> {
        let client = Client::builder()
            .timeout(Duration::from_secs(DOWNLOAD_TIMEOUT_SECS))
            .build()
            .map_err(|e| McpError::ExecutionFailed(format!("HTTP client error: {e}")))?;

        let response = client
            .get(url)
            .header("User-Agent", "DevIt-MCP/1.0")
            .send()
            .await
            .map_err(|e| McpError::ExecutionFailed(format!("Download failed: {e}")))?;

        if !response.status().is_success() {
            return Err(McpError::ExecutionFailed(format!(
                "HTTP {}: {}",
                response.status(),
                url
            )));
        }

        // Get content-type from header
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();

        // Check content-length if available
        if let Some(len) = response.content_length() {
            if len as usize > MAX_DOWNLOAD_BYTES {
                return Err(McpError::InvalidRequest(format!(
                    "Image too large: {:.1} MB (max {} MB)",
                    len as f64 / (1024.0 * 1024.0),
                    MAX_DOWNLOAD_BYTES / (1024 * 1024)
                )));
            }
        }

        let bytes = response
            .bytes()
            .await
            .map_err(|e| McpError::ExecutionFailed(format!("Failed to read response: {e}")))?;

        if bytes.len() > MAX_DOWNLOAD_BYTES {
            return Err(McpError::InvalidRequest(format!(
                "Image too large: {:.1} MB (max {} MB)",
                bytes.len() as f64 / (1024.0 * 1024.0),
                MAX_DOWNLOAD_BYTES / (1024 * 1024)
            )));
        }

        // Determine mime: content-type header, then URL extension, then magic bytes
        let mime = if content_type.starts_with("image/") {
            // Take just the mime part (strip charset etc.)
            content_type
                .split(';')
                .next()
                .unwrap_or("image/png")
                .trim()
                .to_string()
        } else {
            // Try URL extension
            let ext_mime = url
                .rsplit('/')
                .next()
                .and_then(|filename| filename.rsplit('.').next())
                .and_then(Self::mime_from_ext);
            match ext_mime {
                Some(m) => m.to_string(),
                None => Self::mime_from_magic(&bytes).to_string(),
            }
        };

        Ok((bytes.to_vec(), mime))
    }

    /// Resize image if needed and encode as PNG base64, respecting budget
    fn process_image(
        bytes: &[u8],
        max_width: u32,
        max_kb: u64,
        source_mime: &str,
    ) -> Result<(String, String, u32, u32, usize), McpError> {
        // SVG: return as-is (base64 text, no resize)
        if source_mime == "image/svg+xml" {
            let b64 = BASE64.encode(bytes);
            let b64_size = b64.len();
            return Ok((b64, "image/svg+xml".to_string(), 0, 0, b64_size));
        }

        let img = image::load_from_memory(bytes)
            .map_err(|e| McpError::ExecutionFailed(format!("Failed to decode image: {e}")))?;

        let (orig_w, orig_h) = img.dimensions();

        // Resize if width exceeds max_width
        let resized = if max_width > 0 && orig_w > max_width {
            let new_h = ((orig_h as f32) * (max_width as f32 / orig_w as f32)).round() as u32;
            img.resize(max_width, new_h, FilterType::Triangle)
        } else {
            img
        };

        let (final_w, final_h) = resized.dimensions();

        // Encode to PNG
        let encoded = encode_png(&resized)?;
        let encoded_size = encoded.len();

        // If over budget, progressively shrink
        if max_kb > 0 && encoded_size as u64 > max_kb * 1024 {
            let target_bytes = max_kb * 1024;
            // Estimate scale factor from size ratio (sqrt because area scales quadratically)
            let ratio = (target_bytes as f64 / encoded_size as f64).sqrt();
            let shrink_w = ((final_w as f64) * ratio).max(64.0) as u32;
            let shrink_h = ((final_h as f64) * ratio).max(64.0) as u32;

            let shrunk = resized.resize(shrink_w, shrink_h, FilterType::Triangle);
            let (sw, sh) = shrunk.dimensions();
            let shrunk_encoded = encode_png(&shrunk)?;
            let b64 = BASE64.encode(&shrunk_encoded);
            return Ok((b64, "image/png".to_string(), sw, sh, shrunk_encoded.len()));
        }

        let b64 = BASE64.encode(&encoded);
        Ok((b64, "image/png".to_string(), final_w, final_h, encoded_size))
    }
}

fn encode_png(img: &DynamicImage) -> Result<Vec<u8>, McpError> {
    let rgba = img.to_rgba8();
    let (w, h) = img.dimensions();
    let mut buf = Vec::new();
    PngEncoder::new(&mut buf)
        .write_image(&rgba, w, h, ColorType::Rgba8.into())
        .map_err(|e| McpError::ExecutionFailed(format!("PNG encode failed: {e}")))?;
    Ok(buf)
}

#[async_trait]
impl McpTool for ReadImageTool {
    fn name(&self) -> &str {
        "devit_read_image"
    }

    fn description(&self) -> &str {
        "Read an image from local file or download from URL. Returns base64-encoded image \
         for LLM vision. Auto-resizes to fit budget. Use this to show images to the AI."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Local file path (relative to project root)"
                },
                "url": {
                    "type": "string",
                    "description": "URL to download image from (http/https)"
                },
                "max_width": {
                    "type": "integer",
                    "description": "Max width in pixels (default: 1024, 0 = no resize)",
                    "default": 1024
                },
                "max_kb": {
                    "type": "integer",
                    "description": "Max base64 budget in KB (default: 512). Image shrinks to fit.",
                    "default": 512
                }
            }
        })
    }

    async fn execute(&self, params: Value) -> McpResult<Value> {
        let params: ReadImageParams = serde_json::from_value(params)
            .map_err(|e| McpError::InvalidRequest(format!("Invalid params: {e}")))?;

        if params.path.is_none() && params.url.is_none() {
            return Err(McpError::InvalidRequest(
                "Either 'path' or 'url' is required".into(),
            ));
        }

        let max_width = params.max_width.unwrap_or(DEFAULT_MAX_WIDTH);
        let max_kb = params.max_kb.unwrap_or(DEFAULT_MAX_KB);

        // Fetch bytes
        let (source_label, bytes, mime) = if let Some(ref path) = params.path {
            info!(target: "devit_mcp_tools", "devit_read_image | source=local | path={}", path);
            let (bytes, mime) = self.read_local(path).await?;
            (format!("file: {path}"), bytes, mime)
        } else {
            let url = params.url.as_deref().ok_or_else(|| {
                McpError::InvalidRequest("Either 'path' or 'url' is required".into())
            })?;
            info!(target: "devit_mcp_tools", "devit_read_image | source=url | url={}", url);
            let (bytes, mime) = self.download_url(url).await?;
            (format!("url: {url}"), bytes, mime)
        };

        let original_size = bytes.len();

        // Process: decode, resize, encode as base64
        let (b64, output_mime, width, height, encoded_size) =
            Self::process_image(&bytes, max_width, max_kb, &mime)?;

        let mut content = vec![json!({
            "type": "text",
            "text": format!(
                "Image loaded — {} | {}x{}px | {:.1} KB (original {:.1} KB)",
                source_label,
                width,
                height,
                encoded_size as f64 / 1024.0,
                original_size as f64 / 1024.0,
            )
        })];

        content.push(json!({
            "type": "image",
            "data": b64,
            "mimeType": output_mime
        }));

        Ok(json!({
            "content": content,
            "structuredContent": {
                "image": {
                    "source": if params.path.is_some() { "local" } else { "url" },
                    "original_mime": mime,
                    "output_mime": output_mime,
                    "width": width,
                    "height": height,
                    "original_bytes": original_size,
                    "encoded_bytes": encoded_size,
                    "resized": width != 0 && encoded_size != original_size
                }
            }
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mime_from_ext() {
        assert_eq!(ReadImageTool::mime_from_ext("png"), Some("image/png"));
        assert_eq!(ReadImageTool::mime_from_ext("JPG"), Some("image/jpeg"));
        assert_eq!(ReadImageTool::mime_from_ext("jpeg"), Some("image/jpeg"));
        assert_eq!(ReadImageTool::mime_from_ext("gif"), Some("image/gif"));
        assert_eq!(ReadImageTool::mime_from_ext("webp"), Some("image/webp"));
        assert_eq!(ReadImageTool::mime_from_ext("svg"), Some("image/svg+xml"));
        assert_eq!(ReadImageTool::mime_from_ext("xyz"), None);
    }

    #[test]
    fn test_mime_from_magic() {
        assert_eq!(
            ReadImageTool::mime_from_magic(b"\x89PNG\r\n\x1a\nabcdef"),
            "image/png"
        );
        assert_eq!(
            ReadImageTool::mime_from_magic(b"\xFF\xD8\xFF\xE0abcdef"),
            "image/jpeg"
        );
        assert_eq!(ReadImageTool::mime_from_magic(b"GIF89aabcdef"), "image/gif");
        assert_eq!(
            ReadImageTool::mime_from_magic(b"RIFFxxxxWEBP"),
            "image/webp"
        );
        assert_eq!(ReadImageTool::mime_from_magic(b"BMxxxxxx"), "image/bmp");
        // Unknown → fallback
        assert_eq!(ReadImageTool::mime_from_magic(b"unknown_data"), "image/png");
    }

    #[test]
    fn test_process_small_image() {
        // Create a tiny 4x4 PNG in memory
        let img = DynamicImage::new_rgba8(4, 4);
        let mut buf = Vec::new();
        PngEncoder::new(&mut buf)
            .write_image(&img.to_rgba8(), 4, 4, ColorType::Rgba8.into())
            .unwrap();

        let (b64, mime, w, h, _size) =
            ReadImageTool::process_image(&buf, 1024, 512, "image/png").unwrap();
        assert!(!b64.is_empty());
        assert_eq!(mime, "image/png");
        assert_eq!(w, 4);
        assert_eq!(h, 4);
    }

    #[test]
    fn test_process_resize_needed() {
        // Create 2000x1000 image, max_width=800
        let img = DynamicImage::new_rgba8(2000, 1000);
        let mut buf = Vec::new();
        PngEncoder::new(&mut buf)
            .write_image(&img.to_rgba8(), 2000, 1000, ColorType::Rgba8.into())
            .unwrap();

        let (_b64, _mime, w, h, _size) =
            ReadImageTool::process_image(&buf, 800, 0, "image/png").unwrap();
        assert_eq!(w, 800);
        assert_eq!(h, 400); // proportional
    }

    #[test]
    fn test_params_path_only() {
        let v = json!({"path": "images/test.png"});
        let p: ReadImageParams = serde_json::from_value(v).unwrap();
        assert_eq!(p.path.unwrap(), "images/test.png");
        assert!(p.url.is_none());
        assert!(p.max_width.is_none());
    }

    #[test]
    fn test_params_url_only() {
        let v = json!({"url": "https://example.com/img.jpg", "max_kb": 256});
        let p: ReadImageParams = serde_json::from_value(v).unwrap();
        assert!(p.path.is_none());
        assert_eq!(p.url.unwrap(), "https://example.com/img.jpg");
        assert_eq!(p.max_kb.unwrap(), 256);
    }
}
