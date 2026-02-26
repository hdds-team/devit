// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2025-2026 naskel.com

//! Output Shaper - Intelligent output compression for LLM context optimization
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────┐     ┌──────────────────┐     ┌─────────────────┐
//! │ Shell/Exec  │────▶│  OutputShaper    │────▶│ Compact Result  │
//! │ (raw output)│     │  + Format Detect │     │ + raw_path hint │
//! └─────────────┘     └──────────────────┘     └─────────────────┘
//!                              │
//!                     ┌───────┴───────┐
//!                     │ /tmp/devit_   │
//!                     │ output_xxxxx  │ (TTL: 10min)
//!                     └───────────────┘
//! ```

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Hint for output processing intent
#[derive(Debug, Clone, Copy, Default)]
pub enum IntentHint {
    #[default]
    Auto,
    Debug,   // Focus on errors
    Explore, // Structure/summary
    Verbose, // Keep more context
    Raw,     // No shaping, return as-is
}

/// Detected output format
#[derive(Debug, Clone, Copy, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputFormat {
    GccClang,      // C/C++ compiler output
    RustCargo,     // Rust compiler output
    Python,        // Python tracebacks
    Json,          // JSON data
    DirectoryTree, // ls, find, tree output
    LogFile,       // Generic log patterns
    Plain,         // Unknown/plain text
}

/// Result of output shaping
#[derive(Debug)]
pub struct ShapedOutput {
    /// Compressed/filtered content for LLM
    pub compact: String,
    /// Path to raw output (for deep dive)
    pub raw_path: Option<PathBuf>,
    /// Detected format
    pub format: OutputFormat,
    /// Original size in bytes
    pub original_size: usize,
    /// Compact size in bytes
    pub compact_size: usize,
    /// Summary metadata
    pub metadata: OutputMetadata,
}

/// Metadata extracted during shaping
#[derive(Debug, Default)]
pub struct OutputMetadata {
    pub error_count: usize,
    pub warning_count: usize,
    pub line_count: usize,
    pub truncated: bool,
}

/// Trait for format-specific shapers
pub trait FormatShaper: Send + Sync {
    fn detect(&self, output: &str) -> bool;
    fn shape(&self, output: &str, hint: IntentHint) -> (String, OutputMetadata);
    fn format(&self) -> OutputFormat;
}

/// Main OutputShaper coordinator
pub struct OutputShaper {
    shapers: Vec<Box<dyn FormatShaper>>,
    tmp_dir: PathBuf,
    /// Threshold in bytes - outputs larger than this get shaped
    threshold: usize,
    /// Max compact size to return
    max_compact_size: usize,
}

impl Default for OutputShaper {
    fn default() -> Self {
        Self::new()
    }
}

impl OutputShaper {
    pub fn new() -> Self {
        Self {
            shapers: vec![
                Box::new(GccClangShaper),
                Box::new(RustCargoShaper),
                Box::new(PythonTracebackShaper),
                Box::new(JsonShaper),
                Box::new(DirectoryShaper),
                Box::new(LogShaper),
            ],
            tmp_dir: PathBuf::from("/tmp"),
            threshold: 4096,        // 4KB - shape if larger
            max_compact_size: 8192, // 8KB max compact output
        }
    }

    /// Lazy cleanup: remove raw files older than 30 minutes
    /// Called automatically on each shape() invocation
    fn cleanup_old_files(&self) {
        let cutoff = SystemTime::now()
            .checked_sub(std::time::Duration::from_secs(30 * 60))
            .unwrap_or(UNIX_EPOCH);

        if let Ok(entries) = std::fs::read_dir(&self.tmp_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if name.starts_with("devit_output_") && name.ends_with(".txt") {
                        if let Ok(meta) = entry.metadata() {
                            if let Ok(modified) = meta.modified() {
                                if modified < cutoff {
                                    let _ = std::fs::remove_file(&path);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    pub fn with_threshold(mut self, threshold: usize) -> Self {
        self.threshold = threshold;
        self
    }

    /// Process output with optional intent hint
    pub fn shape(&self, output: &str, hint: IntentHint) -> ShapedOutput {
        // Lazy cleanup old raw files (> 30min)
        self.cleanup_old_files();

        let original_size = output.len();

        // Raw mode or small output - pass through
        if matches!(hint, IntentHint::Raw) || original_size <= self.threshold {
            return ShapedOutput {
                compact: output.to_string(),
                raw_path: None,
                format: OutputFormat::Plain,
                original_size,
                compact_size: original_size,
                metadata: OutputMetadata {
                    line_count: output.lines().count(),
                    ..Default::default()
                },
            };
        }

        // Save raw to temp file first
        let raw_path = self.save_raw(output);

        // Detect format and shape
        let (format, compact, metadata) = self.detect_and_shape(output, hint);

        // Truncate if still too large
        let compact = if compact.len() > self.max_compact_size {
            let mut truncated = compact[..self.max_compact_size].to_string();
            truncated.push_str("\n\n[... truncated, see raw_path for full output ...]");
            truncated
        } else {
            compact
        };

        let compact_size = compact.len();

        ShapedOutput {
            compact,
            raw_path,
            format,
            original_size,
            compact_size,
            metadata,
        }
    }

    fn detect_and_shape(
        &self,
        output: &str,
        hint: IntentHint,
    ) -> (OutputFormat, String, OutputMetadata) {
        // Try each shaper in order
        for shaper in &self.shapers {
            if shaper.detect(output) {
                let (compact, metadata) = shaper.shape(output, hint);
                return (shaper.format(), compact, metadata);
            }
        }

        // Fallback: plain text truncation
        let metadata = OutputMetadata {
            line_count: output.lines().count(),
            truncated: true,
            ..Default::default()
        };

        let lines: Vec<&str> = output.lines().collect();
        let compact = if lines.len() > 100 {
            format!(
                "{}\n\n[... {} lines omitted ...]\n\n{}",
                lines[..50].join("\n"),
                lines.len() - 100,
                lines[lines.len() - 50..].join("\n")
            )
        } else {
            output.to_string()
        };

        (OutputFormat::Plain, compact, metadata)
    }

    fn save_raw(&self, output: &str) -> Option<PathBuf> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);

        let path = self.tmp_dir.join(format!("devit_output_{}.txt", timestamp));

        match std::fs::File::create(&path) {
            Ok(mut file) => {
                if file.write_all(output.as_bytes()).is_ok() {
                    Some(path)
                } else {
                    None
                }
            }
            Err(_) => None,
        }
    }
}

// ============================================================================
// Format-specific shapers
// ============================================================================

/// GCC/Clang compiler output shaper
struct GccClangShaper;

impl FormatShaper for GccClangShaper {
    fn detect(&self, output: &str) -> bool {
        // Look for GCC/Clang error patterns
        output.contains(": error:")
            || output.contains(": warning:")
            || output.contains(": fatal error:")
    }

    fn shape(&self, output: &str, hint: IntentHint) -> (String, OutputMetadata) {
        let mut errors = Vec::new();
        let mut warnings = Vec::new();
        let mut other_lines = Vec::new();

        for line in output.lines() {
            if line.contains(": error:") || line.contains(": fatal error:") {
                errors.push(line);
            } else if line.contains(": warning:") {
                warnings.push(line);
            } else if line.contains("error:") || line.starts_with("make:") {
                other_lines.push(line);
            }
        }

        let metadata = OutputMetadata {
            error_count: errors.len(),
            warning_count: warnings.len(),
            line_count: output.lines().count(),
            truncated: errors.len() > 10 || warnings.len() > 5,
        };

        // Build compact output
        let mut compact = String::new();
        compact.push_str(&format!("## Compilation Summary\n"));
        compact.push_str(&format!("- Errors: {}\n", errors.len()));
        compact.push_str(&format!("- Warnings: {}\n\n", warnings.len()));

        if !errors.is_empty() {
            compact.push_str("### Errors\n```\n");
            for (i, err) in errors.iter().take(10).enumerate() {
                compact.push_str(&format!("{}. {}\n", i + 1, err));
            }
            if errors.len() > 10 {
                compact.push_str(&format!("... and {} more errors\n", errors.len() - 10));
            }
            compact.push_str("```\n\n");
        }

        // Include warnings only in verbose mode or if few errors
        if matches!(hint, IntentHint::Verbose) || errors.is_empty() {
            if !warnings.is_empty() {
                compact.push_str("### Warnings (first 5)\n```\n");
                for warn in warnings.iter().take(5) {
                    compact.push_str(&format!("{}\n", warn));
                }
                if warnings.len() > 5 {
                    compact.push_str(&format!("... and {} more warnings\n", warnings.len() - 5));
                }
                compact.push_str("```\n");
            }
        }

        // Add make errors at the end
        if !other_lines.is_empty() {
            compact.push_str("\n### Build System\n```\n");
            for line in other_lines.iter().take(5) {
                compact.push_str(&format!("{}\n", line));
            }
            compact.push_str("```\n");
        }

        (compact, metadata)
    }

    fn format(&self) -> OutputFormat {
        OutputFormat::GccClang
    }
}

/// Rust/Cargo compiler output shaper
struct RustCargoShaper;

impl FormatShaper for RustCargoShaper {
    fn detect(&self, output: &str) -> bool {
        output.contains("error[E")
            || output.contains("warning:") && output.contains("-->")
            || output.contains("Compiling ")
            || output.contains("cargo ")
    }

    fn shape(&self, output: &str, hint: IntentHint) -> (String, OutputMetadata) {
        let mut errors = Vec::new();
        let mut warnings = Vec::new();
        let mut current_block = String::new();
        let mut in_error = false;
        let mut in_warning = false;

        for line in output.lines() {
            if line.starts_with("error[E") || line.starts_with("error:") {
                if !current_block.is_empty() {
                    if in_error {
                        errors.push(current_block.clone());
                    } else if in_warning {
                        warnings.push(current_block.clone());
                    }
                }
                current_block = line.to_string();
                in_error = true;
                in_warning = false;
            } else if line.starts_with("warning:") {
                if !current_block.is_empty() {
                    if in_error {
                        errors.push(current_block.clone());
                    } else if in_warning {
                        warnings.push(current_block.clone());
                    }
                }
                current_block = line.to_string();
                in_error = false;
                in_warning = true;
            } else if in_error || in_warning {
                // Continue collecting the error/warning context
                if line.starts_with("   ") || line.starts_with(" -->") || line.trim().is_empty() {
                    current_block.push('\n');
                    current_block.push_str(line);
                } else if line.starts_with("For more information")
                    || line.starts_with("Some errors")
                {
                    // End of block
                    if in_error {
                        errors.push(current_block.clone());
                    } else {
                        warnings.push(current_block.clone());
                    }
                    current_block.clear();
                    in_error = false;
                    in_warning = false;
                }
            }
        }

        // Don't forget last block
        if !current_block.is_empty() {
            if in_error {
                errors.push(current_block);
            } else if in_warning {
                warnings.push(current_block);
            }
        }

        let metadata = OutputMetadata {
            error_count: errors.len(),
            warning_count: warnings.len(),
            line_count: output.lines().count(),
            truncated: errors.len() > 5,
        };

        let mut compact = String::new();
        compact.push_str(&format!("## Cargo Build Summary\n"));
        compact.push_str(&format!("- Errors: {}\n", errors.len()));
        compact.push_str(&format!("- Warnings: {}\n\n", warnings.len()));

        if !errors.is_empty() {
            compact.push_str("### Errors\n");
            for (i, err) in errors.iter().take(5).enumerate() {
                compact.push_str(&format!("#### Error {}\n```\n{}\n```\n\n", i + 1, err));
            }
            if errors.len() > 5 {
                compact.push_str(&format!(
                    "... and {} more errors (see raw output)\n\n",
                    errors.len() - 5
                ));
            }
        }

        if matches!(hint, IntentHint::Verbose) && !warnings.is_empty() {
            compact.push_str("### Warnings (first 3)\n");
            for warn in warnings.iter().take(3) {
                compact.push_str(&format!("```\n{}\n```\n", warn));
            }
        }

        (compact, metadata)
    }

    fn format(&self) -> OutputFormat {
        OutputFormat::RustCargo
    }
}

/// Python traceback shaper
struct PythonTracebackShaper;

impl FormatShaper for PythonTracebackShaper {
    fn detect(&self, output: &str) -> bool {
        output.contains("Traceback (most recent call last):")
            || output.contains("  File \"") && output.contains("\", line ")
            || output.contains("SyntaxError:")
            || output.contains("IndentationError:")
            || output.contains("ModuleNotFoundError:")
            || output.contains("ImportError:")
    }

    fn shape(&self, output: &str, hint: IntentHint) -> (String, OutputMetadata) {
        let mut tracebacks = Vec::new();
        let mut current_tb = String::new();
        let mut in_traceback = false;
        let mut error_types = std::collections::HashMap::new();

        for line in output.lines() {
            if line.contains("Traceback (most recent call last):") {
                if !current_tb.is_empty() {
                    tracebacks.push(current_tb.clone());
                }
                current_tb = line.to_string();
                in_traceback = true;
            } else if in_traceback {
                current_tb.push('\n');
                current_tb.push_str(line);

                // Detect end of traceback (error line)
                if !line.starts_with(' ') && !line.is_empty() && !line.starts_with("Traceback") {
                    // Extract error type
                    if let Some(colon_pos) = line.find(':') {
                        let error_type = &line[..colon_pos];
                        *error_types.entry(error_type.to_string()).or_insert(0) += 1;
                    }
                    tracebacks.push(current_tb.clone());
                    current_tb.clear();
                    in_traceback = false;
                }
            } else {
                // Check for standalone errors (SyntaxError without full traceback)
                let error_keywords = ["Error:", "Exception:", "Warning:"];
                for kw in &error_keywords {
                    if line.contains(kw) {
                        if let Some(colon_pos) = line.find(':') {
                            let error_type = line[..colon_pos].trim();
                            if !error_type.contains(' ')
                                || error_type.ends_with("Error")
                                || error_type.ends_with("Exception")
                            {
                                *error_types.entry(error_type.to_string()).or_insert(0) += 1;
                                tracebacks.push(line.to_string());
                            }
                        }
                        break;
                    }
                }
            }
        }

        let metadata = OutputMetadata {
            error_count: tracebacks.len(),
            warning_count: 0,
            line_count: output.lines().count(),
            truncated: tracebacks.len() > 3,
        };

        let mut compact = String::new();
        compact.push_str("## Python Error Summary\n");
        compact.push_str(&format!("- Total exceptions: {}\n", tracebacks.len()));

        if !error_types.is_empty() {
            compact.push_str("- Error types:\n");
            let mut types: Vec<_> = error_types.iter().collect();
            types.sort_by(|a, b| b.1.cmp(a.1));
            for (err_type, count) in types.iter().take(5) {
                compact.push_str(&format!("  - {}: {}\n", err_type, count));
            }
        }
        compact.push('\n');

        // Show tracebacks
        let max_tracebacks = if matches!(hint, IntentHint::Verbose) {
            5
        } else {
            3
        };

        if !tracebacks.is_empty() {
            compact.push_str("### Tracebacks\n");
            for (i, tb) in tracebacks.iter().take(max_tracebacks).enumerate() {
                compact.push_str(&format!(
                    "#### Exception {}\n```python\n{}\n```\n\n",
                    i + 1,
                    tb
                ));
            }
            if tracebacks.len() > max_tracebacks {
                compact.push_str(&format!(
                    "... and {} more tracebacks (see raw output)\n",
                    tracebacks.len() - max_tracebacks
                ));
            }
        }

        (compact, metadata)
    }

    fn format(&self) -> OutputFormat {
        OutputFormat::Python
    }
}

/// JSON output shaper
struct JsonShaper;

impl FormatShaper for JsonShaper {
    fn detect(&self, output: &str) -> bool {
        let trimmed = output.trim();
        (trimmed.starts_with('{') && trimmed.ends_with('}'))
            || (trimmed.starts_with('[') && trimmed.ends_with(']'))
    }

    fn shape(&self, output: &str, _hint: IntentHint) -> (String, OutputMetadata) {
        let metadata = OutputMetadata {
            line_count: output.lines().count(),
            ..Default::default()
        };

        // Try to parse and summarize
        match serde_json::from_str::<serde_json::Value>(output) {
            Ok(value) => {
                let summary = summarize_json(&value, 0, 3);
                let compact = format!(
                    "## JSON Output\n- Valid: ✓\n- Structure:\n```json\n{}\n```",
                    summary
                );
                (compact, metadata)
            }
            Err(e) => {
                let compact = format!(
                    "## JSON Output\n- Valid: ✗\n- Parse error: {}\n- First 500 chars:\n```\n{}\n```",
                    e,
                    &output[..output.len().min(500)]
                );
                (compact, metadata)
            }
        }
    }

    fn format(&self) -> OutputFormat {
        OutputFormat::Json
    }
}

fn summarize_json(value: &serde_json::Value, depth: usize, max_depth: usize) -> String {
    if depth >= max_depth {
        return "...".to_string();
    }

    match value {
        serde_json::Value::Object(map) => {
            let fields: Vec<String> = map
                .iter()
                .take(10)
                .map(|(k, v)| {
                    format!(
                        "{:indent$}\"{}\": {}",
                        "",
                        k,
                        summarize_json(v, depth + 1, max_depth),
                        indent = depth * 2
                    )
                })
                .collect();

            let mut result = "{\n".to_string();
            result.push_str(&fields.join(",\n"));
            if map.len() > 10 {
                result.push_str(&format!(
                    ",\n{:indent$}... {} more fields",
                    "",
                    map.len() - 10,
                    indent = depth * 2
                ));
            }
            result.push_str(&format!(
                "\n{:indent$}}}",
                "",
                indent = (depth.saturating_sub(1)) * 2
            ));
            result
        }
        serde_json::Value::Array(arr) => {
            if arr.is_empty() {
                "[]".to_string()
            } else {
                format!(
                    "[{} items, first: {}]",
                    arr.len(),
                    summarize_json(&arr[0], depth + 1, max_depth)
                )
            }
        }
        serde_json::Value::String(s) if s.len() > 50 => format!("\"{}...\"", &s[..47]),
        other => other.to_string(),
    }
}

/// Directory listing shaper (ls, find, tree)
struct DirectoryShaper;

impl FormatShaper for DirectoryShaper {
    fn detect(&self, output: &str) -> bool {
        let lines: Vec<&str> = output.lines().collect();
        if lines.len() < 20 {
            return false;
        }

        // Heuristic: many lines that look like paths
        let path_like = lines
            .iter()
            .filter(|l| l.contains('/') || l.ends_with(':'))
            .count();

        path_like as f64 / lines.len() as f64 > 0.5
    }

    fn shape(&self, output: &str, _hint: IntentHint) -> (String, OutputMetadata) {
        let lines: Vec<&str> = output.lines().collect();
        let total = lines.len();

        let metadata = OutputMetadata {
            line_count: total,
            truncated: total > 50,
            ..Default::default()
        };

        // Extract directory structure
        let mut dirs = std::collections::HashSet::new();
        let mut extensions = std::collections::HashMap::new();

        for line in &lines {
            let path = line.trim();
            if let Some(parent) = Path::new(path).parent() {
                dirs.insert(parent.to_string_lossy().to_string());
            }
            if let Some(ext) = Path::new(path).extension() {
                *extensions
                    .entry(ext.to_string_lossy().to_string())
                    .or_insert(0) += 1;
            }
        }

        let mut compact = format!("## Directory Listing Summary\n");
        compact.push_str(&format!("- Total entries: {}\n", total));
        compact.push_str(&format!("- Unique directories: {}\n\n", dirs.len()));

        compact.push_str("### File types:\n");
        let mut ext_vec: Vec<_> = extensions.iter().collect();
        ext_vec.sort_by(|a, b| b.1.cmp(a.1));
        for (ext, count) in ext_vec.iter().take(10) {
            compact.push_str(&format!("- .{}: {} files\n", ext, count));
        }

        compact.push_str("\n### First 20 entries:\n```\n");
        for line in lines.iter().take(20) {
            compact.push_str(&format!("{}\n", line));
        }
        compact.push_str("```\n");

        if total > 20 {
            compact.push_str(&format!(
                "\n... and {} more entries (see raw output)\n",
                total - 20
            ));
        }

        (compact, metadata)
    }

    fn format(&self) -> OutputFormat {
        OutputFormat::DirectoryTree
    }
}

/// Generic log file shaper
struct LogShaper;

impl FormatShaper for LogShaper {
    fn detect(&self, output: &str) -> bool {
        // Look for common log patterns
        let log_patterns = [
            "ERROR", "WARN", "INFO", "DEBUG", "[error]", "[warn]", "[info]",
        ];
        let matches = log_patterns.iter().filter(|p| output.contains(*p)).count();
        matches >= 2
    }

    fn shape(&self, output: &str, hint: IntentHint) -> (String, OutputMetadata) {
        let mut errors = Vec::new();
        let mut warnings = Vec::new();
        let mut info_count = 0;
        let mut debug_count = 0;

        for line in output.lines() {
            let upper = line.to_uppercase();
            if upper.contains("ERROR") || upper.contains("[ERROR]") {
                errors.push(line);
            } else if upper.contains("WARN") || upper.contains("[WARN]") {
                warnings.push(line);
            } else if upper.contains("INFO") || upper.contains("[INFO]") {
                info_count += 1;
            } else if upper.contains("DEBUG") || upper.contains("[DEBUG]") {
                debug_count += 1;
            }
        }

        let metadata = OutputMetadata {
            error_count: errors.len(),
            warning_count: warnings.len(),
            line_count: output.lines().count(),
            truncated: errors.len() > 20,
        };

        let mut compact = format!("## Log Summary\n");
        compact.push_str(&format!("- Errors: {}\n", errors.len()));
        compact.push_str(&format!("- Warnings: {}\n", warnings.len()));
        compact.push_str(&format!("- Info: {}\n", info_count));
        compact.push_str(&format!("- Debug: {}\n\n", debug_count));

        if !errors.is_empty() {
            compact.push_str("### Errors (last 10)\n```\n");
            for err in errors.iter().rev().take(10) {
                compact.push_str(&format!("{}\n", err));
            }
            compact.push_str("```\n\n");
        }

        if matches!(hint, IntentHint::Verbose) || errors.is_empty() {
            if !warnings.is_empty() {
                compact.push_str("### Warnings (last 5)\n```\n");
                for warn in warnings.iter().rev().take(5) {
                    compact.push_str(&format!("{}\n", warn));
                }
                compact.push_str("```\n");
            }
        }

        (compact, metadata)
    }

    fn format(&self) -> OutputFormat {
        OutputFormat::LogFile
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gcc_detection() {
        let shaper = GccClangShaper;
        assert!(shaper.detect("main.c:10:5: error: expected ';'"));
        assert!(shaper.detect("foo.cpp:1: warning: unused variable"));
        assert!(!shaper.detect("hello world"));
    }

    #[test]
    fn test_gcc_shaping() {
        let shaper = GccClangShaper;
        let output = r#"
main.c:10:5: error: expected ';' before 'return'
main.c:15:1: error: undefined reference to 'foo'
main.c:5:10: warning: unused variable 'x'
make: *** [Makefile:10: main] Error 1
"#;
        let (compact, metadata) = shaper.shape(output, IntentHint::Debug);
        assert_eq!(metadata.error_count, 2);
        assert_eq!(metadata.warning_count, 1);
        assert!(compact.contains("Errors: 2"));
    }

    #[test]
    fn test_small_output_passthrough() {
        let shaper = OutputShaper::new();
        let small = "Hello, world!";
        let result = shaper.shape(small, IntentHint::Auto);
        assert_eq!(result.compact, small);
        assert!(result.raw_path.is_none());
    }

    #[test]
    fn test_rust_shaping() {
        let shaper = RustCargoShaper;
        let output = r#"
error[E0382]: borrow of moved value: `x`
 --> src/main.rs:10:5
  |
5 |     let x = String::new();
  |         - move occurs because `x` has type `String`
8 |     drop(x);
  |          - value moved here
10|     println!("{}", x);
  |                    ^ value borrowed here after move

For more information about this error, try `rustc --explain E0382`.
"#;
        let (compact, metadata) = shaper.shape(output, IntentHint::Debug);
        assert_eq!(metadata.error_count, 1);
        assert!(compact.contains("error[E0382]"));
    }
}
