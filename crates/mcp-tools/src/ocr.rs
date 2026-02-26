// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2025-2026 naskel.com

use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use image::{imageops::FilterType, DynamicImage, GenericImageView};
use mcp_core::{McpResult, McpTool};
use serde_json::{json, Value};
use tokio::process::Command;

use crate::file_read::FileSystemContext;
use crate::{internal_error, validation_error};

pub struct OcrTool {
    fs: Arc<FileSystemContext>,
}

impl OcrTool {
    pub fn new(fs: Arc<FileSystemContext>) -> Self {
        Self { fs }
    }

    fn default_screenshots_dir(&self) -> PathBuf {
        self.fs.root().join(".devit").join("screenshots")
    }

    /// Apply optional image preprocessing (crop, zone, grayscale, threshold, resize).
    /// Returns the path to use for tesseract (original or a temp preprocessed file).
    fn preprocess_image(
        &self,
        img_path: &Path,
        preprocess_cfg: Option<&Value>,
        zone: Option<&str>,
    ) -> (PathBuf, Option<PathBuf>) {
        let pp = preprocess_cfg.and_then(|v| v.as_object());
        let grayscale = pp
            .and_then(|m| m.get("grayscale").and_then(Value::as_bool))
            .unwrap_or(true);
        let threshold = pp
            .and_then(|m| m.get("threshold").and_then(Value::as_u64))
            .map(|v| v.min(255) as u8);
        let resize_width = pp
            .and_then(|m| m.get("resize_width").and_then(Value::as_u64))
            .and_then(|v| u32::try_from(v).ok());
        let crop = pp.and_then(|m| m.get("crop").and_then(Value::as_object));
        let mut crop_params = crop.map(|m| {
            (
                u32::try_from(m.get("x").and_then(Value::as_u64).unwrap_or(0)).unwrap_or(0),
                u32::try_from(m.get("y").and_then(Value::as_u64).unwrap_or(0)).unwrap_or(0),
                m.get("width").and_then(Value::as_u64),
                m.get("height").and_then(Value::as_u64),
            )
        });

        let mut img = match image::open(img_path) {
            Ok(i) => i,
            Err(e) => {
                tracing::warn!("Preprocess load failed: {} (fallback to original)", e);
                return (img_path.to_path_buf(), None);
            }
        };

        // Derive crop from zone template if no explicit crop
        if crop_params.is_none() {
            if let Some(z) = zone {
                let (iw, ih) = img.dimensions();
                crop_params = match z {
                    "terminal_bottom" => {
                        let y = ((ih as f32) * 0.65).round() as u32;
                        Some((0, y, Some(iw as u64), Some((ih - y) as u64)))
                    }
                    "error_zone" => {
                        let w = ((iw as f32) * 0.5).round() as u32;
                        let h = ((ih as f32) * 0.4).round() as u32;
                        let x = (iw - w) / 2;
                        let y = u32::try_from(((ih as i64) * 2 / 10).max(0)).unwrap_or(0);
                        Some((x, y, Some(w as u64), Some(h as u64)))
                    }
                    _ => None,
                };
            }
        }

        // Crop
        if let Some((x, y, w_opt, h_opt)) = crop_params {
            let (iw, ih) = img.dimensions();
            let w = w_opt
                .and_then(|v| u32::try_from(v).ok())
                .unwrap_or(iw.saturating_sub(x));
            let h = h_opt
                .and_then(|v| u32::try_from(v).ok())
                .unwrap_or(ih.saturating_sub(y));
            let cx = x.min(iw);
            let cy = y.min(ih);
            let cw = w.min(iw.saturating_sub(cx));
            let ch = h.min(ih.saturating_sub(cy));
            let sub = image::imageops::crop_imm(&img, cx, cy, cw, ch).to_image();
            img = DynamicImage::ImageRgba8(sub);
        }

        if grayscale {
            img = DynamicImage::ImageLuma8(img.to_luma8());
        }

        if let Some(th) = threshold {
            let mut gray = img.to_luma8();
            for p in gray.pixels_mut() {
                p[0] = if p[0] >= th { 255 } else { 0 };
            }
            img = DynamicImage::ImageLuma8(gray);
        }

        if let Some(tw) = resize_width {
            let (w, h) = img.dimensions();
            let nw = tw.min(w.max(1));
            let nh = ((h as f32) * (nw as f32 / w.max(1) as f32))
                .round()
                .max(1.0)
                .min(u32::MAX as f32) as u32;
            img = img.resize(nw, nh, FilterType::CatmullRom);
        }

        // Save preprocessed image to temp file
        let ts = Utc::now().format("%Y%m%dT%H%M%S");
        let abs = self
            .fs
            .root()
            .join(".devit")
            .join("ocr")
            .join(format!("preproc-{}.png", ts));
        if let Some(parent) = abs.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match img.save(&abs) {
            Ok(()) => (abs.clone(), Some(abs)),
            Err(e) => {
                tracing::warn!(
                    "Failed to save preprocessed image: {} (fallback to original)",
                    e
                );
                (img_path.to_path_buf(), None)
            }
        }
    }

    /// Save OCR text to disk (explicit path or auto-generated).
    fn save_output(
        &self,
        text: &str,
        output_path: Option<&str>,
        img_path: &Path,
        format: &str,
    ) -> Result<Option<PathBuf>, String> {
        let ext = match format {
            "tsv" => "tsv",
            "hocr" => "html",
            _ => "txt",
        };

        if let Some(path_str) = output_path {
            let pb = Path::new(path_str);
            let abs = if pb.is_absolute() {
                pb.to_path_buf()
            } else {
                self.fs.root().join(pb)
            };
            if !abs.starts_with(self.fs.root()) {
                return Err("'output_path' must be inside the workspace".to_string());
            }
            if let Some(parent) = abs.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            std::fs::write(&abs, text)
                .map_err(|e| format!("Failed to write output_path: {}", e))?;
            Ok(Some(abs))
        } else {
            // Auto-generate path under .devit/ocr/
            let base = img_path
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "ocr".into());
            let ts = Utc::now().format("%Y%m%dT%H%M%S");
            let abs = self
                .fs
                .root()
                .join(".devit")
                .join("ocr")
                .join(format!("{}-{}.{}", base, ts, ext));
            if let Some(parent) = abs.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            std::fs::write(&abs, text)
                .map_err(|e| format!("Failed to write output_path: {}", e))?;
            Ok(Some(abs))
        }
    }

    fn resolve_image_path(&self, input: Option<&str>) -> Result<PathBuf, String> {
        match input {
            Some(p) if !p.trim().is_empty() => {
                let p = Path::new(p);
                let abs = if p.is_absolute() {
                    PathBuf::from(p)
                } else {
                    self.fs.root().join(p)
                };
                if abs.exists() {
                    Ok(abs)
                } else {
                    Err(format!("Image not found: {}", abs.display()))
                }
            }
            _ => {
                // Pick most recent file from .devit/screenshots
                let dir = self.default_screenshots_dir();
                let rd = std::fs::read_dir(&dir).map_err(|e| format!("{}", e))?;
                let mut latest: Option<(std::time::SystemTime, PathBuf)> = None;
                for entry in rd.flatten() {
                    let path = entry.path();
                    if path.is_file() {
                        if let Ok(meta) = std::fs::metadata(&path) {
                            if let Ok(mtime) = meta.modified() {
                                if latest.as_ref().map(|(t, _)| *t < mtime).unwrap_or(true) {
                                    latest = Some((mtime, path));
                                }
                            }
                        }
                    }
                }
                match latest {
                    Some((_t, p)) => Ok(p),
                    None => Err(format!(
                        "No screenshots found in {}. Provide a path.",
                        dir.display()
                    )),
                }
            }
        }
    }
}

#[async_trait]
impl McpTool for OcrTool {
    fn name(&self) -> &str {
        "devit_ocr"
    }

    fn description(&self) -> &str {
        "Extract text from an image (OCR via tesseract). By default, reads the last screenshot."
    }

    async fn execute(&self, params: Value) -> McpResult<Value> {
        if !params.is_null() && !params.is_object() {
            return Err(validation_error(
                "Parameters must be a JSON object (or omitted).",
            ));
        }

        let path_param = params.get("path").and_then(Value::as_str);
        let lang = params.get("lang").and_then(Value::as_str).unwrap_or("eng");
        let psm = params.get("psm").and_then(Value::as_u64);
        let oem = params.get("oem").and_then(Value::as_u64);
        let max_chars = params
            .get("max_chars")
            .and_then(Value::as_u64)
            .unwrap_or(2000) as usize;
        let format = params
            .get("format")
            .and_then(Value::as_str)
            .unwrap_or("text");
        let output_path = params.get("output_path").and_then(Value::as_str);
        let explicit_inline = params.get("inline").and_then(Value::as_bool);
        let silent = params
            .get("silent")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let inline = explicit_inline.unwrap_or(true) && !silent;

        if !matches!(format, "text" | "tsv" | "hocr") {
            return Err(validation_error(
                "Invalid 'format' parameter (expected: text|tsv|hocr)",
            ));
        }

        let img_path = self
            .resolve_image_path(path_param)
            .map_err(|e| validation_error(&e))?;

        // Optional preprocessing
        let preprocess_cfg = params.get("preprocess");
        let zone = params.get("zone").and_then(Value::as_str);
        let do_preprocess = matches!(
            preprocess_cfg,
            Some(Value::Bool(true)) | Some(Value::Object(_))
        ) || zone.is_some();

        let (tesseract_input_path, temp_path) = if do_preprocess {
            self.preprocess_image(&img_path, preprocess_cfg, zone)
        } else {
            (img_path.clone(), None)
        };

        // Build and run tesseract
        let mut cmd = Command::new("tesseract");
        cmd.arg(&tesseract_input_path)
            .arg("stdout")
            .arg("-l")
            .arg(lang);
        if let Some(v) = psm {
            cmd.arg("--psm").arg(v.to_string());
        }
        if let Some(v) = oem {
            cmd.arg("--oem").arg(v.to_string());
        }
        if format != "text" {
            cmd.arg(format);
        }

        let output = cmd
            .output()
            .await
            .map_err(|e| internal_error(e.to_string()))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(internal_error(format!(
                "tesseract failed (code {:?}): {}",
                output.status.code(),
                stderr
            )));
        }

        let mut text = String::from_utf8_lossy(&output.stdout).to_string();

        // Save output if needed
        let saved_to = if output_path.is_some() || !inline {
            self.save_output(&text, output_path, &img_path, format)
                .map_err(|e| {
                    if output_path.is_some() && e.contains("must be inside") {
                        validation_error(&e)
                    } else {
                        internal_error(e)
                    }
                })?
        } else {
            None
        };

        let full_len = text.len();
        let truncated = text.len() > max_chars;
        if truncated {
            text.truncate(max_chars);
        }

        let img_rel = img_path
            .strip_prefix(self.fs.root())
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| img_path.display().to_string());
        let saved_rel = saved_to.as_ref().map(|p| {
            p.strip_prefix(self.fs.root())
                .map(|q| q.display().to_string())
                .unwrap_or_else(|_| p.display().to_string())
        });

        let trunc_label = if truncated { " (truncated)" } else { "" };
        let summary = match &saved_rel {
            Some(s) => format!(
                "[OCR] Extracted {} chars -- {} (lang: {}, format: {}) -> saved: {}{}",
                full_len, img_rel, lang, format, s, trunc_label
            ),
            None => format!(
                "[OCR] Extracted {} chars -- {} (lang: {}, format: {}){}",
                full_len, img_rel, lang, format, trunc_label
            ),
        };

        let mut content = vec![json!({"type":"text","text": summary})];
        if inline {
            content.push(json!({"type":"text","text": text}));
        }

        let result = json!({
            "content": content,
            "structuredContent": {
                "ocr": {
                    "path": img_rel,
                    "engine": "tesseract",
                    "lang": lang,
                    "psm": psm,
                    "oem": oem,
                    "format": format,
                    "chars": full_len,
                    "truncated": truncated,
                    "inline": inline,
                    "saved_to": saved_rel
                }
            }
        });

        if let Some(p) = temp_path {
            let _ = std::fs::remove_file(p);
        }

        Ok(result)
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Image path (default: last screenshot)"},
                "lang": {"type": "string", "description": "Tesseract language (e.g., eng, fra)", "default": "eng"},
                "psm": {"type": "integer", "description": "Page segmentation mode (tesseract --psm)"},
                "oem": {"type": "integer", "description": "OCR Engine mode (tesseract --oem)"},
                "max_chars": {"type": "integer", "description": "Max text size returned in response (if inline=true)", "default": 2000},
                "format": {"type": "string", "enum": ["text", "tsv", "hocr"], "default": "text"},
                "inline": {"type": "boolean", "description": "Include text excerpt in response", "default": true},
                "silent": {"type": "boolean", "description": "Alias for inline=false (forces silent mode)", "default": false},
                "output_path": {"type": "string", "description": "Path to save full output (txt/tsv/html)"},
                "zone": {"type": "string", "description": "Zone template: terminal_bottom | error_zone", "enum": ["terminal_bottom", "error_zone"]},
                "preprocess": {
                    "type": ["boolean", "object"],
                    "description": "Enable preprocessing (grayscale/threshold/resize/crop)",
                    "properties": {
                        "grayscale": {"type": "boolean", "default": true},
                        "threshold": {"type": "integer", "minimum": 0, "maximum": 255},
                        "resize_width": {"type": "integer", "minimum": 1},
                        "crop": {
                            "type": "object",
                            "properties": {
                                "x": {"type": "integer", "minimum": 0},
                                "y": {"type": "integer", "minimum": 0},
                                "width": {"type": "integer", "minimum": 1},
                                "height": {"type": "integer", "minimum": 1}
                            }
                        }
                    }
                }
            },
            "additionalProperties": false
        })
    }
}
