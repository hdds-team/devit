// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2025-2026 naskel.com

use async_trait::async_trait;
use mcp_core::{McpResult, McpTool};
use serde_json::{json, Value};
use std::process::Command;

use crate::validation_error;

// ============================================================================
// WINDOW LISTING TOOL
// ============================================================================

pub struct WindowListTool;

impl WindowListTool {
    pub fn new() -> Self {
        Self
    }

    fn list_windows() -> Result<Vec<Value>, String> {
        let output = Command::new("wmctrl")
            .arg("-l")
            .arg("-p")
            .output()
            .map_err(|e| format!("Failed to execute wmctrl: {}", e))?;

        if !output.status.success() {
            return Err("wmctrl command failed. Is wmctrl installed?".to_string());
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut windows = Vec::new();

        for line in stdout.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 8 {
                // Format: ID DESK PID WIDTHxHEIGHT+X+Y HOST CLASS TITLE
                let window_id = parts[0].to_string();
                let desktop = parts[1].parse::<i32>().unwrap_or(-1);
                let pid = parts[2].parse::<u32>().unwrap_or(0);
                let class = parts[7].to_string();
                let title = parts[8..].join(" ");

                // Parse geometry (WIDTHxHEIGHT+X+Y)
                let geometry = parts[4];
                let (width, height, x, y) = parse_geometry(geometry);

                windows.push(json!({
                    "id": window_id,
                    "desktop": desktop,
                    "pid": pid,
                    "title": title,
                    "class": class,
                    "geometry": {
                        "width": width,
                        "height": height,
                        "x": x,
                        "y": y,
                    }
                }));
            }
        }

        Ok(windows)
    }
}

#[async_trait]
impl McpTool for WindowListTool {
    fn name(&self) -> &str {
        "devit_window_list"
    }

    fn description(&self) -> &str {
        "List all open windows on X11 with ID, class, title, and geometry information."
    }

    async fn execute(&self, params: Value) -> McpResult<Value> {
        if !params.is_null() && !params.is_object() {
            return Err(validation_error(
                "Parameters must be a JSON object (or omitted).",
            ));
        }

        match Self::list_windows() {
            Ok(windows) => {
                let summary = json!({
                    "total_windows": windows.len(),
                    "windows": windows,
                });

                Ok(json!({
                    "content": [{
                        "type": "text",
                        "text": format!("🪟 Found {} open windows", summary["total_windows"])
                    }],
                    "structuredContent": summary
                }))
            }
            Err(e) => Err(crate::internal_error(e)),
        }
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false
        })
    }
}

// ============================================================================
// WINDOW SEND TEXT TOOL
// ============================================================================

pub struct WindowSendTextTool;

impl WindowSendTextTool {
    pub fn new() -> Self {
        Self
    }

    fn send_text_to_window(window_id: &str, text: &str) -> Result<(), String> {
        // First, focus the window
        Command::new("wmctrl")
            .args(&["-i", "-a", window_id])
            .output()
            .map_err(|e| format!("Failed to focus window: {}", e))?;

        // Small delay to ensure window is focused
        std::thread::sleep(std::time::Duration::from_millis(100));

        // Send the text using xdotool
        Command::new("xdotool")
            .args(&["type", "--clearmodifiers", text])
            .output()
            .map_err(|e| format!("Failed to send text: {}", e))?;

        Ok(())
    }
}

#[async_trait]
impl McpTool for WindowSendTextTool {
    fn name(&self) -> &str {
        "devit_window_send_text"
    }

    fn description(&self) -> &str {
        "Send text to a specific window identified by window ID, title, or class."
    }

    async fn execute(&self, params: Value) -> McpResult<Value> {
        if !params.is_object() {
            return Err(validation_error(
                "Parameters must be a JSON object with 'window_id' and 'text'.",
            ));
        }

        let text = params.get("text").and_then(Value::as_str).ok_or_else(|| {
            validation_error("'text' parameter is required and must be a string.")
        })?;

        let window_id = params
            .get("window_id")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                validation_error("'window_id' parameter is required and must be a string.")
            })?;

        match Self::send_text_to_window(window_id, text) {
            Ok(()) => Ok(json!({
                "content": [{
                    "type": "text",
                    "text": format!("✅ Text sent to window {}", window_id)
                }]
            })),
            Err(e) => Err(crate::internal_error(e)),
        }
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "window_id": {
                    "type": "string",
                    "description": "X11 window ID (e.g., '0x4400001')"
                },
                "text": {
                    "type": "string",
                    "description": "Text to send to the window"
                }
            },
            "required": ["window_id", "text"],
            "additionalProperties": false
        })
    }
}

// ============================================================================
// WINDOW FOCUS TOOL
// ============================================================================

pub struct WindowFocusTool;

impl WindowFocusTool {
    pub fn new() -> Self {
        Self
    }

    fn focus_window(window_id: &str) -> Result<(), String> {
        Command::new("wmctrl")
            .args(&["-i", "-a", window_id])
            .output()
            .map_err(|e| format!("Failed to focus window: {}", e))?;

        Ok(())
    }
}

#[async_trait]
impl McpTool for WindowFocusTool {
    fn name(&self) -> &str {
        "devit_window_focus"
    }

    fn description(&self) -> &str {
        "Activate and bring a window to focus by its window ID."
    }

    async fn execute(&self, params: Value) -> McpResult<Value> {
        if !params.is_object() {
            return Err(validation_error(
                "Parameters must be a JSON object with 'window_id'.",
            ));
        }

        let window_id = params
            .get("window_id")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                validation_error("'window_id' parameter is required and must be a string.")
            })?;

        match Self::focus_window(window_id) {
            Ok(()) => Ok(json!({
                "content": [{
                    "type": "text",
                    "text": format!("🎯 Window {} focused", window_id)
                }]
            })),
            Err(e) => Err(crate::internal_error(e)),
        }
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "window_id": {
                    "type": "string",
                    "description": "X11 window ID (e.g., '0x4400001')"
                }
            },
            "required": ["window_id"],
            "additionalProperties": false
        })
    }
}

// ============================================================================
// WINDOW SCREENSHOT TOOL
// ============================================================================

pub struct WindowScreenshotTool;

impl WindowScreenshotTool {
    pub fn new() -> Self {
        Self
    }

    fn capture_window_screenshot(window_id: &str) -> Result<String, String> {
        // Use gnome-screenshot or similar if available, otherwise convert from full screenshot
        // For simplicity, we'll use import (ImageMagick) which works well with X11
        let filename = format!("/tmp/window_{}.png", window_id.replace("0x", ""));

        let output = Command::new("import")
            .arg("-window")
            .arg(window_id)
            .arg(&filename)
            .output()
            .map_err(|e| format!("Failed to capture window screenshot: {}", e))?;

        if !output.status.success() {
            return Err(
                "Failed to capture window screenshot. Is ImageMagick installed?".to_string(),
            );
        }

        Ok(filename)
    }
}

#[async_trait]
impl McpTool for WindowScreenshotTool {
    fn name(&self) -> &str {
        "devit_window_screenshot"
    }

    fn description(&self) -> &str {
        "Capture a screenshot of a specific window by its ID."
    }

    async fn execute(&self, params: Value) -> McpResult<Value> {
        if !params.is_object() {
            return Err(validation_error(
                "Parameters must be a JSON object with 'window_id'.",
            ));
        }

        let window_id = params
            .get("window_id")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                validation_error("'window_id' parameter is required and must be a string.")
            })?;

        match Self::capture_window_screenshot(window_id) {
            Ok(filepath) => {
                let metadata = std::fs::metadata(&filepath).map(|m| m.len()).unwrap_or(0);

                Ok(json!({
                    "content": [{
                        "type": "text",
                        "text": format!("📸 Window screenshot saved: {}", filepath)
                    }],
                    "structuredContent": {
                        "path": filepath,
                        "size_bytes": metadata,
                        "window_id": window_id
                    }
                }))
            }
            Err(e) => Err(crate::internal_error(e)),
        }
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "window_id": {
                    "type": "string",
                    "description": "X11 window ID (e.g., '0x4400001')"
                }
            },
            "required": ["window_id"],
            "additionalProperties": false
        })
    }
}

// ============================================================================
// WINDOW GET CONTENT TOOL (OCR-based)
// ============================================================================

pub struct WindowGetContentTool;

impl WindowGetContentTool {
    pub fn new() -> Self {
        Self
    }

    fn get_window_content(window_id: &str) -> Result<String, String> {
        // First capture the window
        let filename = format!("/tmp/window_{}_content.png", window_id.replace("0x", ""));

        Command::new("import")
            .arg("-window")
            .arg(window_id)
            .arg(&filename)
            .output()
            .map_err(|e| format!("Failed to capture window: {}", e))?;

        // Then use OCR (tesseract) to extract text
        let output = Command::new("tesseract")
            .arg(&filename)
            .arg("stdout")
            .output()
            .map_err(|e| format!("Failed to run OCR: {}. Is tesseract installed?", e))?;

        if !output.status.success() {
            return Err("OCR failed".to_string());
        }

        let text = String::from_utf8_lossy(&output.stdout).to_string();

        // Clean up temp file
        let _ = std::fs::remove_file(&filename);

        Ok(text)
    }
}

#[async_trait]
impl McpTool for WindowGetContentTool {
    fn name(&self) -> &str {
        "devit_window_get_content"
    }

    fn description(&self) -> &str {
        "Extract visible text content from a window using OCR (Tesseract)."
    }

    async fn execute(&self, params: Value) -> McpResult<Value> {
        if !params.is_object() {
            return Err(validation_error(
                "Parameters must be a JSON object with 'window_id'.",
            ));
        }

        let window_id = params
            .get("window_id")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                validation_error("'window_id' parameter is required and must be a string.")
            })?;

        match Self::get_window_content(window_id) {
            Ok(content) => Ok(json!({
                "content": [{
                    "type": "text",
                    "text": "📖 Window content extracted (via OCR)"
                }],
                "structuredContent": {
                    "window_id": window_id,
                    "content": content,
                }
            })),
            Err(e) => Err(crate::internal_error(e)),
        }
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "window_id": {
                    "type": "string",
                    "description": "X11 window ID (e.g., '0x4400001')"
                }
            },
            "required": ["window_id"],
            "additionalProperties": false
        })
    }
}

// ============================================================================
// HELPER FUNCTIONS
// ============================================================================

fn parse_geometry(geometry: &str) -> (u32, u32, i32, i32) {
    // Format: WIDTHxHEIGHT+X+Y
    // Example: 1920x1080+0+0
    let parts: Vec<&str> = geometry
        .split(|c| c == 'x' || c == '+' || c == '-')
        .collect();

    let width = parts.get(0).and_then(|s| s.parse().ok()).unwrap_or(0);
    let height = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
    let x = parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
    let y = parts.get(3).and_then(|s| s.parse().ok()).unwrap_or(0);

    (width, height, x, y)
}
