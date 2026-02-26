// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2025-2026 naskel.com

use std::ffi::OsString;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use devit_cli::core::{
    file_ops::FileContent as CoreFileContent,
    formats::{Compressible, OutputFormat},
    fs::FsService,
};
use mcp_core::{McpResult, McpTool};
use serde_json::{json, Map, Number, Value};

use crate::errors::{
    internal_error, invalid_diff_error, io_error, missing_param, policy_block_error,
    validation_error,
};

const MAX_FILE_SIZE: u64 = 1024 * 1024; // 1 MB

#[derive(Clone, Copy)]
enum FileReadMode {
    Basic,
    Extended,
}

pub struct FileReadTool {
    context: Arc<FileSystemContext>,
    mode: FileReadMode,
}

impl FileReadTool {
    pub fn new(context: Arc<FileSystemContext>) -> Self {
        Self {
            context,
            mode: FileReadMode::Basic,
        }
    }

    pub fn new_extended(context: Arc<FileSystemContext>) -> Self {
        Self {
            context,
            mode: FileReadMode::Extended,
        }
    }

    async fn render_structured(
        &self,
        path: &str,
        format: &OutputFormat,
        fields: Option<&[String]>,
        line_numbers: bool,
        offset: Option<u64>,
        limit: Option<u64>,
    ) -> McpResult<String> {
        let service = FsService::new(self.context.root().to_path_buf())
            .map_err(|err| internal_error(err.to_string()))?;

        service
            .read_ext(
                path,
                format,
                fields,
                Some(line_numbers),
                offset.and_then(|v| u32::try_from(v).ok()),
                limit.and_then(|v| u32::try_from(v).ok()),
            )
            .await
            .map_err(|err| internal_error(err.to_string()))
    }
}

#[async_trait]
impl McpTool for FileReadTool {
    fn name(&self) -> &str {
        match self.mode {
            FileReadMode::Basic => "devit_file_read",
            FileReadMode::Extended => "devit_file_read_ext",
        }
    }

    fn description(&self) -> &str {
        match self.mode {
            FileReadMode::Basic => {
                "Read file content with security validation and optional line numbers"
            }
            FileReadMode::Extended => {
                "Read file content with compression, field filtering, and token optimization"
            }
        }
    }

    async fn execute(&self, params: Value) -> McpResult<Value> {
        let path = params
            .get("path")
            .and_then(Value::as_str)
            .ok_or_else(|| missing_param("path", "string"))?;

        let line_numbers = params
            .get("line_numbers")
            .and_then(Value::as_bool)
            .unwrap_or(false);

        let offset_raw = params.get("offset").and_then(Value::as_u64);
        let limit_raw = params.get("limit").and_then(Value::as_u64);

        let offset = offset_raw.map(|value| value as usize);
        let limit = limit_raw.map(|value| value as usize);

        let format = params
            .get("format")
            .and_then(Value::as_str)
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "text".to_string());

        let fields: Option<Vec<String>> = params
            .get("fields")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(|value| value.as_str().map(|s| s.trim().to_string()))
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
            })
            .and_then(|vec| if vec.is_empty() { None } else { Some(vec) });

        if matches!(format.as_str(), "text" | "plain") && fields.is_some() {
            return Err(validation_error(
                "The 'fields' parameter is only supported for json, compact and table formats",
            ));
        }

        let canonical_path = self.context.resolve_read_path(path)?;
        let file_content = self
            .context
            .read_file(&canonical_path, line_numbers, offset, limit)?;

        let mut metadata = Map::new();
        metadata.insert(
            "path".to_string(),
            Value::String(file_content.path.to_string_lossy().to_string()),
        );
        metadata.insert(
            "size".to_string(),
            Value::Number(Number::from(file_content.size)),
        );
        metadata.insert(
            "encoding".to_string(),
            Value::String(file_content.encoding.clone()),
        );
        metadata.insert("line_numbers".to_string(), Value::Bool(line_numbers));
        metadata.insert(
            "line_count".to_string(),
            Value::Number(Number::from(file_content.content.lines().count() as u64)),
        );
        if let Some(raw) = offset_raw {
            metadata.insert("offset".to_string(), Value::Number(Number::from(raw)));
        }
        if let Some(raw) = limit_raw {
            metadata.insert("limit".to_string(), Value::Number(Number::from(raw)));
        }
        metadata.insert(
            "mode".to_string(),
            Value::String(
                match self.mode {
                    FileReadMode::Basic => "basic",
                    FileReadMode::Extended => "extended",
                }
                .to_string(),
            ),
        );
        if let Some(list) = fields.as_ref() {
            metadata.insert(
                "fields".to_string(),
                Value::Array(list.iter().cloned().map(Value::String).collect()),
            );
        }

        match format.as_str() {
            "text" | "plain" => {
                metadata.insert("format".to_string(), Value::String("text".to_string()));
                let text_output = if line_numbers {
                    file_content
                        .lines
                        .as_ref()
                        .map(|values| values.join("\n"))
                        .unwrap_or_else(|| file_content.content.clone())
                } else {
                    file_content.content.clone()
                };

                Ok(json!({
                    "content": [
                        {
                            "type": "text",
                            "text": text_output
                        }
                    ],
                    "metadata": metadata
                }))
            }
            "json" | "compact" | "table" => {
                let output_format = match format.as_str() {
                    "json" => OutputFormat::Json,
                    "compact" => OutputFormat::Compact,
                    "table" => OutputFormat::Table,
                    _ => unreachable!("handled above"),
                };

                let formatted = self
                    .render_structured(
                        path,
                        &output_format,
                        fields.as_ref().map(|vec| vec.as_slice()),
                        line_numbers,
                        offset_raw,
                        limit_raw,
                    )
                    .await?;

                let compression_ratio = file_content
                    .get_compression_ratio(&output_format)
                    .map_err(|err| internal_error(err.to_string()))?;

                let format_label = match output_format {
                    OutputFormat::Json => "Json",
                    OutputFormat::Compact => "Compact",
                    OutputFormat::Table => "Table",
                    OutputFormat::MessagePack => "MessagePack",
                };

                metadata.insert("format".to_string(), Value::String(format.to_string()));
                if let Some(number) = Number::from_f64(compression_ratio as f64) {
                    metadata.insert("compression_ratio".to_string(), Value::Number(number));
                }

                let code_fence = match output_format {
                    OutputFormat::Table => "table",
                    _ => "json",
                };

                let header = format!(
                    "📄 File: {} (format: {})",
                    file_content.path.to_string_lossy(),
                    format_label
                );

                let text_output = format!("{header}\n\n```{code_fence}\n{formatted}\n```");

                Ok(json!({
                    "content": [
                        {
                            "type": "text",
                            "text": text_output
                        }
                    ],
                    "metadata": metadata
                }))
            }
            other => Err(validation_error(&format!(
                "Format '{}' not supported. Use text, json, compact or table.",
                other
            ))),
        }
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "line_numbers": {"type": "boolean"},
                "offset": {"type": "integer", "minimum": 0},
                "limit": {"type": "integer", "minimum": 1},
                "format": {
                    "type": "string",
                    "enum": ["text", "json", "compact", "table"],
                    "description": "Output format (default: text)"
                },
                "fields": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Limit returned fields (json/compact/table formats)"
                }
            },
            "required": ["path"]
        })
    }
}

pub struct FileSystemContext {
    root_path: PathBuf,
    allowed_paths: Vec<PathBuf>,
}

impl FileSystemContext {
    pub fn new(root_path: PathBuf) -> McpResult<Self> {
        Self::with_allowed_paths(root_path, vec![])
    }

    pub fn with_allowed_paths(root_path: PathBuf, allowed_paths: Vec<PathBuf>) -> McpResult<Self> {
        let canonical_root = root_path.canonicalize().map_err(|err| {
            io_error(
                "canonicalize repository root",
                Some(&root_path),
                err.to_string(),
            )
        })?;

        // Canonicalize all allowed paths (skip non-existent ones)
        let canonical_allowed: Vec<PathBuf> = allowed_paths
            .into_iter()
            .filter_map(|p| p.canonicalize().ok())
            .collect();

        Ok(Self {
            root_path: canonical_root,
            allowed_paths: canonical_allowed,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root_path
    }

    /// Resolve a path for read-oriented operations.
    /// This method never creates filesystem entries.
    pub fn resolve_read_path(&self, raw_path: &str) -> McpResult<PathBuf> {
        self.resolve_path_internal(raw_path)
    }

    /// Resolve a path for write-oriented operations.
    /// This method only resolves the target path and never mutates the filesystem.
    pub fn resolve_write_path(&self, raw_path: &str) -> McpResult<PathBuf> {
        self.resolve_path_internal(raw_path)
    }

    /// Check if a canonical path is within any allowed directory
    fn is_path_allowed(&self, canonical: &Path) -> bool {
        // Check root_path
        if canonical.starts_with(&self.root_path) {
            return true;
        }

        // Check /tmp
        let temp_dir = std::env::temp_dir();
        if canonical.starts_with(&temp_dir) {
            return true;
        }

        // Check allowed_paths
        for allowed in &self.allowed_paths {
            if canonical.starts_with(allowed) {
                return true;
            }
        }

        false
    }

    /// Backward-compatible resolver.
    /// Prefer `resolve_read_path`/`resolve_write_path` in new code.
    pub fn resolve_path(&self, raw_path: &str) -> McpResult<PathBuf> {
        self.resolve_read_path(raw_path)
    }

    fn resolve_path_internal(&self, raw_path: &str) -> McpResult<PathBuf> {
        let input_path = Path::new(raw_path);

        let path_str = raw_path;

        // Reject traversal markers early for both relative and absolute input.
        if path_str.contains("../") || path_str.contains("..\\") {
            return Err(policy_block_error(
                "path_traversal_protection",
                "any",
                "patch",
                "Path traversal attempt detected",
            ));
        }

        if path_str.contains('\0') {
            return Err(policy_block_error(
                "path_security_null_byte",
                "any",
                "patch",
                "Null byte detected in path",
            ));
        }

        if path_str.len() > 4096 {
            return Err(policy_block_error(
                "path_security_length_limit",
                "any",
                "patch",
                "Path too long",
            ));
        }

        if input_path.is_absolute() {
            // For existing paths, canonicalize directly
            if input_path.exists() {
                let canonical = input_path.canonicalize().map_err(|err| {
                    io_error("canonicalize path", Some(input_path), err.to_string())
                })?;

                if self.is_path_allowed(&canonical) {
                    return Ok(canonical);
                }
            } else if let Some(parent) = input_path.parent() {
                let canonical_parent = self.canonicalize_ancestor(parent)?;

                if let Some(filename) = input_path.file_name() {
                    let full_path = canonical_parent.join(filename);

                    if self.is_path_allowed(&canonical_parent) {
                        return Ok(full_path);
                    }
                }
            }

            return Err(policy_block_error(
                "path_security_repo_boundary",
                "any",
                "patch",
                format!("Absolute path outside project: {}", raw_path),
            ));
        }

        let joined = self.root_path.join(input_path);

        let canonical = if joined.exists() {
            joined
                .canonicalize()
                .map_err(|err| io_error("canonicalize path", Some(&joined), err.to_string()))?
        } else {
            self.manual_resolve(input_path)?
        };

        if !canonical.starts_with(&self.root_path) {
            return Err(policy_block_error(
                "path_security_repo_boundary",
                "any",
                "patch",
                format!(
                    "Path escapes repository: {} -> {}",
                    raw_path,
                    canonical.display()
                ),
            ));
        }

        Ok(canonical)
    }

    fn canonicalize_ancestor(&self, path: &Path) -> McpResult<PathBuf> {
        if path.exists() {
            return path
                .canonicalize()
                .map_err(|err| io_error("canonicalize parent path", Some(path), err.to_string()));
        }

        let mut cursor = path.to_path_buf();
        let mut missing_components: Vec<OsString> = Vec::new();

        while !cursor.exists() {
            let name = cursor.file_name().ok_or_else(|| {
                policy_block_error(
                    "path_security_repo_boundary",
                    "any",
                    "patch",
                    format!("Cannot resolve path ancestor: {}", path.display()),
                )
            })?;
            missing_components.push(name.to_os_string());

            cursor = cursor
                .parent()
                .ok_or_else(|| {
                    policy_block_error(
                        "path_security_repo_boundary",
                        "any",
                        "patch",
                        format!("Cannot resolve path ancestor: {}", path.display()),
                    )
                })?
                .to_path_buf();
        }

        let mut canonical = cursor
            .canonicalize()
            .map_err(|err| io_error("canonicalize parent path", Some(&cursor), err.to_string()))?;

        for component in missing_components.iter().rev() {
            canonical.push(component);
        }

        Ok(canonical)
    }

    pub fn read_file(
        &self,
        canonical_path: &Path,
        line_numbers: bool,
        offset: Option<usize>,
        limit: Option<usize>,
    ) -> McpResult<CoreFileContent> {
        if !canonical_path.exists() {
            return Err(io_error(
                "read file content",
                Some(canonical_path),
                "File not found",
            ));
        }

        if !canonical_path.is_file() {
            return Err(invalid_diff_error("Path is not a file", None));
        }

        let metadata = fs::metadata(canonical_path)
            .map_err(|err| io_error("read file metadata", Some(canonical_path), err.to_string()))?;

        let file_size = metadata.len();
        if file_size > MAX_FILE_SIZE {
            return Err(invalid_diff_error(
                format!(
                    "File too large: {} bytes (max: {} bytes)",
                    file_size, MAX_FILE_SIZE
                ),
                None,
            ));
        }

        let content = fs::read_to_string(canonical_path)
            .map_err(|err| io_error("read file content", Some(canonical_path), err.to_string()))?;

        let filtered_content = if let (Some(offset), Some(limit)) = (offset, limit) {
            let lines: Vec<&str> = content.lines().collect();
            let start = offset.min(lines.len());
            let end = (offset + limit).min(lines.len());
            lines[start..end].join("\n")
        } else {
            content.clone()
        };

        let lines = if line_numbers {
            Some(
                filtered_content
                    .lines()
                    .enumerate()
                    .map(|(index, line)| format!("{:4}: {}", index + 1, line))
                    .collect(),
            )
        } else {
            None
        };

        let encoding = detect_encoding(&filtered_content);

        Ok(CoreFileContent {
            path: canonical_path.to_path_buf(),
            content: filtered_content,
            size: file_size,
            lines,
            encoding,
        })
    }

    fn manual_resolve(&self, target: &Path) -> McpResult<PathBuf> {
        let mut resolved = self.root_path.clone();

        for component in target.components() {
            match component {
                Component::Normal(name) => {
                    resolved.push(name);
                }
                Component::ParentDir => {
                    if !resolved.pop() || !resolved.starts_with(&self.root_path) {
                        return Err(policy_block_error(
                            "path_resolution_escape",
                            "any",
                            "patch",
                            "Path resolution would escape repository",
                        ));
                    }
                }
                Component::CurDir | Component::RootDir | Component::Prefix(_) => {
                    // Skip these components
                }
            }
        }

        Ok(resolved)
    }
}

fn detect_encoding(content: &str) -> String {
    if content.bytes().take(1000).any(|byte| byte > 127) {
        if content.starts_with('\u{FEFF}') {
            "utf-8-bom".to_string()
        } else {
            "utf-8".to_string()
        }
    } else {
        "utf-8".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::FileSystemContext;
    use tempfile::TempDir;

    #[test]
    fn resolve_write_path_does_not_create_missing_dirs_for_absolute_path() {
        let root = TempDir::new().unwrap();
        let allowed = TempDir::new().unwrap();
        let ctx = FileSystemContext::with_allowed_paths(
            root.path().to_path_buf(),
            vec![allowed.path().to_path_buf()],
        )
        .unwrap();

        let target = allowed.path().join("nested/new.txt");
        assert!(!allowed.path().join("nested").exists());

        let resolved = ctx.resolve_write_path(target.to_str().unwrap()).unwrap();
        assert_eq!(resolved, target);
        assert!(!allowed.path().join("nested").exists());
    }

    #[test]
    fn resolve_write_path_rejects_absolute_path_outside_allowed_roots() {
        let root = TempDir::new().unwrap();
        let ctx = FileSystemContext::new(root.path().to_path_buf()).unwrap();

        let target = "/etc/devit-should-not-write-here.txt";
        let err = ctx.resolve_write_path(target);
        assert!(err.is_err());
    }

    #[test]
    fn resolve_read_path_rejects_traversal() {
        let root = TempDir::new().unwrap();
        let ctx = FileSystemContext::new(root.path().to_path_buf()).unwrap();
        assert!(ctx.resolve_read_path("../etc/passwd").is_err());
    }
}
