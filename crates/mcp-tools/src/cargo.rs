// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2025-2026 naskel.com

//! devit_cargo - Direct cargo tool for LLM convenience
//!
//! Runs cargo subcommands directly via tokio::process::Command.
//! No sandbox, no policy, no HMAC - just cargo on the user's project.
//!
//! For build/check/clippy: uses --message-format=json to parse structured
//! diagnostics. For other commands: uses OutputShaper for compression.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use mcp_core::{McpError, McpResult, McpTool};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::process::Command;
use tracing::info;

use crate::file_read::FileSystemContext;
use crate::output_shaper::{IntentHint, OutputShaper};

/// Cargo subcommands we support
const JSON_SUBCOMMANDS: &[&str] = &["build", "check", "clippy"];

#[derive(Debug, Deserialize)]
struct CargoParams {
    subcommand: String,
    #[serde(default)]
    package: Option<String>,
    #[serde(default)]
    release: bool,
    #[serde(default)]
    features: Option<String>,
    #[serde(default)]
    all_targets: bool,
    #[serde(default)]
    args: Option<String>,
    #[serde(default)]
    working_dir: Option<String>,
    #[serde(default)]
    timeout_secs: Option<u64>,
}

/// A single parsed diagnostic from cargo's JSON output
#[derive(Debug, Serialize)]
struct CargoDiagnostic {
    level: String,
    message: String,
    code: Option<String>,
    file: Option<String>,
    line: Option<u64>,
    rendered: Option<String>,
}

pub struct CargoTool {
    context: Arc<FileSystemContext>,
    output_shaper: OutputShaper,
}

impl CargoTool {
    pub fn new(context: Arc<FileSystemContext>) -> Self {
        Self {
            context,
            output_shaper: OutputShaper::new(),
        }
    }

    fn resolve_working_dir(&self, working_dir: &Option<String>) -> PathBuf {
        match working_dir {
            Some(dir) => {
                let candidate = self.context.root().join(dir);
                if candidate.is_dir() {
                    candidate
                } else {
                    self.context.root().to_path_buf()
                }
            }
            None => self.context.root().to_path_buf(),
        }
    }

    fn build_args(&self, params: &CargoParams) -> Vec<String> {
        let mut args = vec![params.subcommand.clone()];

        if let Some(ref pkg) = params.package {
            args.push("-p".into());
            args.push(pkg.clone());
        }

        if params.release {
            args.push("--release".into());
        }

        if let Some(ref features) = params.features {
            args.push("--features".into());
            args.push(features.clone());
        }

        if params.all_targets {
            args.push("--all-targets".into());
        }

        // Auto --message-format=json for build/check/clippy
        if JSON_SUBCOMMANDS.contains(&params.subcommand.as_str()) {
            args.push("--message-format=json".into());
        }

        // Extra raw args (split by whitespace)
        if let Some(ref extra) = params.args {
            args.extend(extra.split_whitespace().map(String::from));
        }

        args
    }

    /// Parse cargo's JSON output (one JSON object per line)
    fn parse_json_diagnostics(&self, output: &str) -> (Vec<CargoDiagnostic>, bool) {
        let mut diagnostics = Vec::new();
        let mut build_success = false;

        for line in output.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            let parsed: Value = match serde_json::from_str(line) {
                Ok(v) => v,
                Err(_) => continue,
            };

            match parsed.get("reason").and_then(|r| r.as_str()) {
                Some("compiler-message") => {
                    if let Some(msg) = parsed.get("message") {
                        let level = msg
                            .get("level")
                            .and_then(|l| l.as_str())
                            .unwrap_or("unknown")
                            .to_string();

                        // Skip notes/help - they're noise for the LLM
                        if level == "note" || level == "help" || level == "failure-note" {
                            continue;
                        }

                        let message = msg
                            .get("message")
                            .and_then(|m| m.as_str())
                            .unwrap_or("")
                            .to_string();

                        let code = msg
                            .get("code")
                            .and_then(|c| c.get("code"))
                            .and_then(|c| c.as_str())
                            .map(String::from);

                        // Get primary span location
                        let (file, line_num) = msg
                            .get("spans")
                            .and_then(|s| s.as_array())
                            .and_then(|spans| {
                                spans.iter().find(|s| {
                                    s.get("is_primary")
                                        .and_then(|p| p.as_bool())
                                        .unwrap_or(false)
                                })
                            })
                            .map(|span| {
                                let file = span
                                    .get("file_name")
                                    .and_then(|f| f.as_str())
                                    .map(String::from);
                                let line = span.get("line_start").and_then(|l| l.as_u64());
                                (file, line)
                            })
                            .unwrap_or((None, None));

                        let rendered = msg
                            .get("rendered")
                            .and_then(|r| r.as_str())
                            .map(String::from);

                        diagnostics.push(CargoDiagnostic {
                            level,
                            message,
                            code,
                            file,
                            line: line_num,
                            rendered,
                        });
                    }
                }
                Some("build-finished") => {
                    build_success = parsed
                        .get("success")
                        .and_then(|s| s.as_bool())
                        .unwrap_or(false);
                }
                _ => {}
            }
        }

        (diagnostics, build_success)
    }

    fn format_diagnostics_response(
        &self,
        params: &CargoParams,
        diagnostics: &[CargoDiagnostic],
        build_success: bool,
        stderr: &str,
        exit_code: i32,
        duration_ms: u128,
    ) -> Value {
        let errors: Vec<&CargoDiagnostic> =
            diagnostics.iter().filter(|d| d.level == "error").collect();
        let warnings: Vec<&CargoDiagnostic> = diagnostics
            .iter()
            .filter(|d| d.level == "warning")
            .collect();

        let status = if build_success { "OK" } else { "FAILED" };
        let icon = if build_success { "ok" } else { "err" };

        // Build compact text for the LLM
        let mut text = format!(
            "cargo {} — {} | exit {} | {:.1}s | {} error(s), {} warning(s)",
            params.subcommand,
            status,
            exit_code,
            duration_ms as f64 / 1000.0,
            errors.len(),
            warnings.len()
        );

        if !errors.is_empty() {
            text.push_str("\n\n## Errors\n");
            for (i, err) in errors.iter().enumerate() {
                let loc = match (&err.file, err.line) {
                    (Some(f), Some(l)) => format!(" ({}:{})", f, l),
                    (Some(f), None) => format!(" ({})", f),
                    _ => String::new(),
                };
                let code = err
                    .code
                    .as_ref()
                    .map(|c| format!("[{}] ", c))
                    .unwrap_or_default();
                text.push_str(&format!("{}. {}{}{}\n", i + 1, code, err.message, loc));
            }
        }

        if !warnings.is_empty() {
            let max_warnings = if errors.is_empty() { 10 } else { 5 };
            text.push_str(&format!(
                "\n## Warnings (showing {}/{})\n",
                warnings.len().min(max_warnings),
                warnings.len()
            ));
            for warn in warnings.iter().take(max_warnings) {
                let loc = match (&warn.file, warn.line) {
                    (Some(f), Some(l)) => format!(" ({}:{})", f, l),
                    _ => String::new(),
                };
                text.push_str(&format!("- {}{}\n", warn.message, loc));
            }
        }

        // If no diagnostics at all but build failed, show stderr
        if diagnostics.is_empty() && !build_success && !stderr.is_empty() {
            let stderr_trimmed = if stderr.len() > 4096 {
                format!("{}...\n[truncated]", &stderr[..4096])
            } else {
                stderr.to_string()
            };
            text.push_str(&format!("\n## stderr\n```\n{}\n```", stderr_trimmed));
        }

        // Structured diagnostics for programmatic use
        let diag_json: Vec<Value> = diagnostics
            .iter()
            .map(|d| {
                json!({
                    "level": d.level,
                    "message": d.message,
                    "code": d.code,
                    "file": d.file,
                    "line": d.line,
                })
            })
            .collect();

        json!({
            "content": [{"type": "text", "text": text}],
            "structuredContent": {
                "cargo": {
                    "subcommand": params.subcommand,
                    "success": build_success,
                    "status": icon,
                    "exit_code": exit_code,
                    "duration_ms": duration_ms,
                    "errors": errors.len(),
                    "warnings": warnings.len(),
                    "diagnostics": diag_json,
                    "package": params.package,
                    "release": params.release
                }
            }
        })
    }

    fn format_text_response(
        &self,
        params: &CargoParams,
        stdout: &str,
        stderr: &str,
        exit_code: i32,
        duration_ms: u128,
    ) -> Value {
        let success = exit_code == 0;
        let status = if success { "OK" } else { "FAILED" };

        // Combine output for shaping
        let combined = if stderr.is_empty() {
            stdout.to_string()
        } else if stdout.is_empty() {
            stderr.to_string()
        } else {
            format!("{}\n{}", stdout, stderr)
        };

        let shaped = self.output_shaper.shape(&combined, IntentHint::Auto);

        let summary = format!(
            "cargo {} — {} | exit {} | {:.1}s",
            params.subcommand,
            status,
            exit_code,
            duration_ms as f64 / 1000.0,
        );

        let mut text = summary.clone();
        if !shaped.compact.trim().is_empty() {
            text.push_str("\n\n");
            text.push_str(&shaped.compact);
        }

        json!({
            "content": [{"type": "text", "text": text}],
            "structuredContent": {
                "cargo": {
                    "subcommand": params.subcommand,
                    "success": success,
                    "exit_code": exit_code,
                    "duration_ms": duration_ms,
                    "output_size": shaped.original_size,
                    "compact_size": shaped.compact_size,
                    "raw_path": shaped.raw_path.map(|p| p.display().to_string()),
                    "package": params.package,
                    "release": params.release
                }
            }
        })
    }
}

#[async_trait]
impl McpTool for CargoTool {
    fn name(&self) -> &str {
        "devit_cargo"
    }

    fn description(&self) -> &str {
        "Run cargo commands (build, check, test, clippy, fmt, doc, run, bench). \
         Returns structured diagnostics for build/check/clippy (via --message-format=json). \
         Direct execution, no sandbox overhead."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "subcommand": {
                    "type": "string",
                    "enum": ["build", "check", "test", "clippy", "fmt", "doc", "run", "bench"],
                    "description": "Cargo subcommand to run"
                },
                "package": {
                    "type": "string",
                    "description": "Target package (-p <package>). Omit for workspace default."
                },
                "release": {
                    "type": "boolean",
                    "description": "Build in release mode (--release)"
                },
                "features": {
                    "type": "string",
                    "description": "Comma-separated features to enable (--features)"
                },
                "all_targets": {
                    "type": "boolean",
                    "description": "Build/check all targets (--all-targets)"
                },
                "args": {
                    "type": "string",
                    "description": "Additional raw arguments passed to cargo"
                },
                "working_dir": {
                    "type": "string",
                    "description": "Working directory relative to project root"
                },
                "timeout_secs": {
                    "type": "number",
                    "description": "Timeout in seconds (default: 300, max: 600)",
                    "minimum": 1
                }
            },
            "required": ["subcommand"]
        })
    }

    async fn execute(&self, params: Value) -> McpResult<Value> {
        let params: CargoParams = serde_json::from_value(params)
            .map_err(|e| McpError::InvalidRequest(format!("Invalid params: {e}")))?;

        let valid = [
            "build", "check", "test", "clippy", "fmt", "doc", "run", "bench",
        ];
        if !valid.contains(&params.subcommand.as_str()) {
            return Err(McpError::InvalidRequest(format!(
                "Unknown subcommand '{}'. Valid: {}",
                params.subcommand,
                valid.join(", ")
            )));
        }

        let cwd = self.resolve_working_dir(&params.working_dir);
        let args = self.build_args(&params);
        let timeout = params.timeout_secs.unwrap_or(300).min(600);
        let uses_json = JSON_SUBCOMMANDS.contains(&params.subcommand.as_str());

        info!(
            target: "devit_mcp_tools",
            "devit_cargo | cargo {} | cwd={} | json={} | timeout={}s",
            args.join(" "),
            cwd.display(),
            uses_json,
            timeout
        );

        let start = Instant::now();

        let output = tokio::time::timeout(
            std::time::Duration::from_secs(timeout),
            Command::new("cargo")
                .args(&args)
                .current_dir(&cwd)
                .env("CARGO_TERM_COLOR", "never")
                .output(),
        )
        .await
        .map_err(|_| {
            McpError::ExecutionFailed(format!(
                "cargo {} timed out after {}s",
                params.subcommand, timeout
            ))
        })?
        .map_err(|e| McpError::ExecutionFailed(format!("Failed to spawn cargo: {e}")))?;

        let duration_ms = start.elapsed().as_millis();
        let exit_code = output.status.code().unwrap_or(-1);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if uses_json {
            let (diagnostics, build_success) = self.parse_json_diagnostics(&stdout);
            // If no build-finished message, infer from exit code
            let success = if diagnostics.is_empty() && !stdout.contains("build-finished") {
                exit_code == 0
            } else {
                build_success
            };
            Ok(self.format_diagnostics_response(
                &params,
                &diagnostics,
                success,
                &stderr,
                exit_code,
                duration_ms,
            ))
        } else {
            Ok(self.format_text_response(&params, &stdout, &stderr, exit_code, duration_ms))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_args_simple() {
        let ctx =
            Arc::new(FileSystemContext::with_allowed_paths(PathBuf::from("/tmp"), vec![]).unwrap());
        let tool = CargoTool::new(ctx);

        let params = CargoParams {
            subcommand: "build".into(),
            package: None,
            release: false,
            features: None,
            all_targets: false,
            args: None,
            working_dir: None,
            timeout_secs: None,
        };

        let args = tool.build_args(&params);
        assert_eq!(args, vec!["build", "--message-format=json"]);
    }

    #[test]
    fn test_build_args_full() {
        let ctx =
            Arc::new(FileSystemContext::with_allowed_paths(PathBuf::from("/tmp"), vec![]).unwrap());
        let tool = CargoTool::new(ctx);

        let params = CargoParams {
            subcommand: "check".into(),
            package: Some("mcp-tools".into()),
            release: true,
            features: Some("test-utils".into()),
            all_targets: true,
            args: Some("--jobs 4".into()),
            working_dir: None,
            timeout_secs: None,
        };

        let args = tool.build_args(&params);
        assert_eq!(
            args,
            vec![
                "check",
                "-p",
                "mcp-tools",
                "--release",
                "--features",
                "test-utils",
                "--all-targets",
                "--message-format=json",
                "--jobs",
                "4"
            ]
        );
    }

    #[test]
    fn test_build_args_test_no_json() {
        let ctx =
            Arc::new(FileSystemContext::with_allowed_paths(PathBuf::from("/tmp"), vec![]).unwrap());
        let tool = CargoTool::new(ctx);

        let params = CargoParams {
            subcommand: "test".into(),
            package: None,
            release: false,
            features: None,
            all_targets: false,
            args: None,
            working_dir: None,
            timeout_secs: None,
        };

        let args = tool.build_args(&params);
        assert_eq!(args, vec!["test"]);
        // test doesn't get --message-format=json
    }

    #[test]
    fn test_parse_json_diagnostics() {
        let ctx =
            Arc::new(FileSystemContext::with_allowed_paths(PathBuf::from("/tmp"), vec![]).unwrap());
        let tool = CargoTool::new(ctx);

        let output = r#"{"reason":"compiler-message","message":{"level":"error","message":"unused variable","code":{"code":"E0381","explanation":null},"spans":[{"file_name":"src/main.rs","line_start":10,"line_end":10,"column_start":5,"column_end":6,"is_primary":true,"text":[]}],"rendered":"error: unused variable"}}
{"reason":"compiler-message","message":{"level":"warning","message":"dead code","code":null,"spans":[],"rendered":"warning: dead code"}}
{"reason":"build-finished","success":false}"#;

        let (diags, success) = tool.parse_json_diagnostics(output);
        assert_eq!(diags.len(), 2);
        assert_eq!(diags[0].level, "error");
        assert_eq!(diags[0].file.as_deref(), Some("src/main.rs"));
        assert_eq!(diags[0].line, Some(10));
        assert_eq!(diags[1].level, "warning");
        assert!(!success);
    }
}
