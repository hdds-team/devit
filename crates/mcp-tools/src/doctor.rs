// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2025-2026 naskel.com

//! devit_doctor — Project health check pipeline
//!
//! Chains cargo check → clippy → test → fmt --check in one call.
//! Returns structured pass/fail per step.

use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use mcp_core::{McpError, McpResult, McpTool};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::process::Command;
use tracing::info;

use crate::file_read::FileSystemContext;

#[derive(Debug, Deserialize)]
struct DoctorParams {
    /// Which checks to run. Default: all.
    #[serde(default)]
    checks: Option<Vec<String>>,
    /// Stop on first failure (default: false — run all)
    #[serde(default)]
    fail_fast: bool,
    /// Target package (-p)
    #[serde(default)]
    package: Option<String>,
}

#[derive(Debug)]
struct CheckResult {
    name: String,
    success: bool,
    exit_code: i32,
    duration_ms: u128,
    summary: String,
    error_count: usize,
    warning_count: usize,
}

pub struct DoctorTool {
    context: Arc<FileSystemContext>,
}

impl DoctorTool {
    pub fn new(context: Arc<FileSystemContext>) -> Self {
        Self { context }
    }

    fn cwd(&self) -> &std::path::Path {
        self.context.root()
    }

    async fn run_check(&self, name: &str, package: &Option<String>) -> CheckResult {
        let start = Instant::now();

        let mut args: Vec<String> = match name {
            "check" => vec!["check".into(), "--message-format=json".into()],
            "clippy" => vec![
                "clippy".into(),
                "--message-format=json".into(),
                "--".into(),
                "-D".into(),
                "warnings".into(),
            ],
            "test" => vec!["test".into()],
            "fmt" => vec!["fmt".into(), "--check".into()],
            other => {
                return CheckResult {
                    name: other.to_string(),
                    success: false,
                    exit_code: -1,
                    duration_ms: 0,
                    summary: format!("Unknown check: {other}"),
                    error_count: 1,
                    warning_count: 0,
                };
            }
        };

        if let Some(ref pkg) = package {
            // Insert -p <pkg> after the subcommand
            args.insert(1, pkg.clone());
            args.insert(1, "-p".into());
        }

        let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();

        let output = match Command::new("cargo")
            .args(&arg_refs)
            .current_dir(self.cwd())
            .env("CARGO_TERM_COLOR", "never")
            .output()
            .await
        {
            Ok(o) => o,
            Err(e) => {
                return CheckResult {
                    name: name.to_string(),
                    success: false,
                    exit_code: -1,
                    duration_ms: start.elapsed().as_millis(),
                    summary: format!("Failed to spawn cargo: {e}"),
                    error_count: 1,
                    warning_count: 0,
                };
            }
        };

        let duration_ms = start.elapsed().as_millis();
        let exit_code = output.status.code().unwrap_or(-1);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        let (error_count, warning_count, summary) = if name == "check" || name == "clippy" {
            self.parse_json_summary(&stdout, &stderr)
        } else {
            self.parse_text_summary(name, &stdout, &stderr, exit_code)
        };

        CheckResult {
            name: name.to_string(),
            success: exit_code == 0,
            exit_code,
            duration_ms,
            summary,
            error_count,
            warning_count,
        }
    }

    fn parse_json_summary(&self, stdout: &str, _stderr: &str) -> (usize, usize, String) {
        let mut errors = 0usize;
        let mut warnings = 0usize;
        let mut first_error: Option<String> = None;

        for line in stdout.lines() {
            if let Ok(parsed) = serde_json::from_str::<Value>(line) {
                if parsed.get("reason").and_then(|r| r.as_str()) == Some("compiler-message") {
                    if let Some(msg) = parsed.get("message") {
                        let level = msg.get("level").and_then(|l| l.as_str()).unwrap_or("");
                        match level {
                            "error" => {
                                errors += 1;
                                if first_error.is_none() {
                                    first_error = msg
                                        .get("message")
                                        .and_then(|m| m.as_str())
                                        .map(String::from);
                                }
                            }
                            "warning" => {
                                warnings += 1;
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        let summary = if errors > 0 {
            format!(
                "{} error(s), {} warning(s) — first: {}",
                errors,
                warnings,
                first_error.as_deref().unwrap_or("?")
            )
        } else if warnings > 0 {
            format!("{warnings} warning(s)")
        } else {
            "clean".to_string()
        };

        (errors, warnings, summary)
    }

    fn parse_text_summary(
        &self,
        name: &str,
        stdout: &str,
        stderr: &str,
        exit_code: i32,
    ) -> (usize, usize, String) {
        match name {
            "test" => {
                // Look for "test result:" line
                let combined = format!("{stdout}{stderr}");
                if let Some(line) = combined.lines().rev().find(|l| l.contains("test result:")) {
                    (
                        if exit_code != 0 { 1 } else { 0 },
                        0,
                        line.trim().to_string(),
                    )
                } else if exit_code == 0 {
                    (0, 0, "all tests passed".to_string())
                } else {
                    (1, 0, format!("tests failed (exit {})", exit_code))
                }
            }
            "fmt" => {
                if exit_code == 0 {
                    (0, 0, "formatted".to_string())
                } else {
                    let diff_files = stdout.lines().filter(|l| l.starts_with("Diff")).count();
                    let unformatted = if diff_files > 0 {
                        format!("{diff_files} file(s) need formatting")
                    } else {
                        format!("formatting issues (exit {})", exit_code)
                    };
                    (1, 0, unformatted)
                }
            }
            _ => {
                if exit_code == 0 {
                    (0, 0, "ok".into())
                } else {
                    (1, 0, format!("failed (exit {})", exit_code))
                }
            }
        }
    }
}

#[async_trait]
impl McpTool for DoctorTool {
    fn name(&self) -> &str {
        "devit_doctor"
    }

    fn description(&self) -> &str {
        "Project health check pipeline. Runs check → clippy → test → fmt in one call. \
         Returns structured pass/fail per step. Use fail_fast to stop on first failure."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "checks": {
                    "type": "array",
                    "items": {
                        "type": "string",
                        "enum": ["check", "clippy", "test", "fmt"]
                    },
                    "description": "Which checks to run (default: all in order check → clippy → test → fmt)"
                },
                "fail_fast": {
                    "type": "boolean",
                    "description": "Stop on first failure (default: false, runs all)"
                },
                "package": {
                    "type": "string",
                    "description": "Target package (-p). Omit for workspace."
                }
            }
        })
    }

    async fn execute(&self, params: Value) -> McpResult<Value> {
        let params: DoctorParams = serde_json::from_value(params)
            .map_err(|e| McpError::InvalidRequest(format!("Invalid params: {e}")))?;

        let checks: Vec<String> = params
            .checks
            .unwrap_or_else(|| vec!["check".into(), "clippy".into(), "test".into(), "fmt".into()]);

        info!(
            target: "devit_mcp_tools",
            "devit_doctor | checks={:?} | fail_fast={} | cwd={}",
            checks,
            params.fail_fast,
            self.cwd().display()
        );

        let total_start = Instant::now();
        let mut results: Vec<CheckResult> = Vec::new();
        let mut all_passed = true;

        for check_name in &checks {
            let result = self.run_check(check_name, &params.package).await;
            let failed = !result.success;
            results.push(result);

            if failed {
                all_passed = false;
                if params.fail_fast {
                    break;
                }
            }
        }

        let total_duration = total_start.elapsed().as_millis();
        let icon = if all_passed { "OK" } else { "ISSUES" };

        // Build compact text
        let mut text = format!(
            "devit_doctor — {} | {:.1}s total\n\n",
            icon,
            total_duration as f64 / 1000.0
        );

        for r in &results {
            let status = if r.success { "PASS" } else { "FAIL" };
            text.push_str(&format!(
                "  {} {} ({:.1}s) — {}\n",
                status,
                r.name,
                r.duration_ms as f64 / 1000.0,
                r.summary
            ));
        }

        let total_errors: usize = results.iter().map(|r| r.error_count).sum();
        let total_warnings: usize = results.iter().map(|r| r.warning_count).sum();

        text.push_str(&format!(
            "\nTotal: {} error(s), {} warning(s)",
            total_errors, total_warnings
        ));

        // Structured
        let steps: Vec<Value> = results
            .iter()
            .map(|r| {
                json!({
                    "name": r.name,
                    "success": r.success,
                    "exit_code": r.exit_code,
                    "duration_ms": r.duration_ms,
                    "summary": r.summary,
                    "errors": r.error_count,
                    "warnings": r.warning_count
                })
            })
            .collect();

        Ok(json!({
            "content": [{"type": "text", "text": text}],
            "structuredContent": {
                "doctor": {
                    "all_passed": all_passed,
                    "total_duration_ms": total_duration,
                    "total_errors": total_errors,
                    "total_warnings": total_warnings,
                    "steps": steps
                }
            }
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_params_defaults() {
        let v = json!({});
        let p: DoctorParams = serde_json::from_value(v).unwrap();
        assert!(p.checks.is_none());
        assert!(!p.fail_fast);
        assert!(p.package.is_none());
    }

    #[test]
    fn test_params_custom() {
        let v = json!({"checks": ["check", "fmt"], "fail_fast": true, "package": "mcp-tools"});
        let p: DoctorParams = serde_json::from_value(v).unwrap();
        assert_eq!(p.checks.unwrap(), vec!["check", "fmt"]);
        assert!(p.fail_fast);
    }

    #[test]
    fn test_parse_json_summary() {
        let ctx =
            Arc::new(FileSystemContext::with_allowed_paths(PathBuf::from("/tmp"), vec![]).unwrap());
        let tool = DoctorTool::new(ctx);

        let stdout = r#"{"reason":"compiler-message","message":{"level":"warning","message":"unused var","code":null,"spans":[],"rendered":"warn"}}
{"reason":"compiler-message","message":{"level":"error","message":"type mismatch","code":{"code":"E0308","explanation":null},"spans":[],"rendered":"err"}}
{"reason":"build-finished","success":false}"#;

        let (errors, warnings, summary) = tool.parse_json_summary(stdout, "");
        assert_eq!(errors, 1);
        assert_eq!(warnings, 1);
        assert!(summary.contains("type mismatch"));
    }
}
