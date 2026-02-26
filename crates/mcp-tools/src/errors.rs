// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2025-2026 naskel.com

use std::path::{Path, PathBuf};

use chrono::{SecondsFormat, Utc};
use mcp_core::McpError;
use serde_json::{json, Value};
use uuid::Uuid;

pub fn validation_error(message: &str) -> McpError {
    McpError::rpc(
        -32602,
        format!(
            "{}. Please rewrite the input so it satisfies the expected schema.",
            message
        ),
        Some(json!({
            "code": "E_VALIDATION",
            "message": message,
            "hint": "Please rewrite the input so it satisfies the expected schema.",
            "actionable": true,
            "timestamp": current_timestamp()
        })),
    )
}

/// Structured error for a missing required parameter.
/// Produces per-field error detail like:
/// `{ "expected": "string", "code": "missing_required", "path": ["name"], "message": "..." }`
pub fn missing_param(name: &str, expected_type: &str) -> McpError {
    let field_error = json!({
        "expected": expected_type,
        "code": "missing_required",
        "path": [name],
        "message": format!("Missing required parameter: expected {}, received undefined", expected_type)
    });
    McpError::rpc(
        -32602,
        format!(
            "Invalid arguments: [{}]. Please rewrite the input so it satisfies the expected schema.",
            field_error
        ),
        Some(json!({
            "code": "E_VALIDATION",
            "errors": [field_error],
            "hint": "Please rewrite the input so it satisfies the expected schema.",
            "actionable": true,
            "timestamp": current_timestamp()
        })),
    )
}

/// Structured error for an invalid parameter value.
pub fn invalid_param(name: &str, expected: &str, received: &str) -> McpError {
    let field_error = json!({
        "expected": expected,
        "received": received,
        "code": "invalid_type",
        "path": [name],
        "message": format!("Invalid input: expected {}, received {}", expected, received)
    });
    McpError::rpc(
        -32602,
        format!(
            "Invalid arguments: [{}]. Please rewrite the input so it satisfies the expected schema.",
            field_error
        ),
        Some(json!({
            "code": "E_VALIDATION",
            "errors": [field_error],
            "hint": "Please rewrite the input so it satisfies the expected schema.",
            "actionable": true,
            "timestamp": current_timestamp()
        })),
    )
}

/// Structured error with multiple field violations.
pub fn validation_errors(fields: Vec<(&str, &str, &str)>) -> McpError {
    let errors: Vec<Value> = fields
        .iter()
        .map(|(name, expected, message)| {
            json!({
                "expected": expected,
                "code": "invalid_input",
                "path": [name],
                "message": message
            })
        })
        .collect();
    McpError::rpc(
        -32602,
        format!(
            "Invalid arguments: {}. Please rewrite the input so it satisfies the expected schema.",
            serde_json::to_string(&errors).unwrap_or_default()
        ),
        Some(json!({
            "code": "E_VALIDATION",
            "errors": errors,
            "hint": "Please rewrite the input so it satisfies the expected schema.",
            "actionable": true,
            "timestamp": current_timestamp()
        })),
    )
}

pub fn desktop_env_error(operation: &str, exit_code: Option<i32>, stderr: &str) -> McpError {
    let normalized = stderr.trim();
    let lower = normalized.to_ascii_lowercase();
    let display_hint = lower.contains("can't open display");
    let hint = if display_hint {
        "Set DISPLAY/XAUTHORITY and use an X11/XWayland session so xdotool can drive the screen."
    } else {
        "Ensure xdotool is installed and callable from the daemon environment."
    };
    let status_text = exit_code
        .map(|code| format!("exit code {}", code))
        .unwrap_or_else(|| "signal".to_string());
    let base = if normalized.is_empty() {
        format!("Desktop automation failed during {operation} ({status_text}).")
    } else {
        format!("Desktop automation failed during {operation} ({status_text}: {normalized}).")
    };
    let message = format!("{base} Hint: {hint}");

    McpError::rpc(
        -32001,
        message.clone(),
        Some(json!({
            "code": "E_DESKTOP_ENV",
            "message": message,
            "operation": operation,
            "exit_code": exit_code,
            "stderr": if normalized.is_empty() { Value::Null } else { Value::String(normalized.to_string()) },
            "hint": hint,
            "actionable": true,
            "timestamp": current_timestamp()
        })),
    )
}

pub fn invalid_diff_error(reason: impl Into<String>, line_number: Option<usize>) -> McpError {
    let reason = reason.into();
    let message = if let Some(line) = line_number {
        format!(
            "❌ Patch failed: invalid unified diff at line {} ({})",
            line, reason
        )
    } else {
        format!("❌ Patch failed: {}", reason)
    };
    McpError::rpc(
        -32600,
        message.clone(),
        Some(json!({
            "code": "E_INVALID_DIFF",
            "message": message,
            "hint": "Check that the patch is a valid unified diff",
            "actionable": true,
            "timestamp": current_timestamp(),
            "details": {
                "reason": reason,
                "line_number": line_number
            }
        })),
    )
}

pub fn policy_block_error(
    rule: &str,
    required_level: &str,
    current_level: &str,
    context: impl Into<String>,
) -> McpError {
    let context = context.into();
    let formatted = format!("❌ Operation blocked: security violation ({})", context);
    McpError::rpc(
        -32601,
        formatted.clone(),
        Some(json!({
            "code": "E_POLICY_BLOCK",
            "message": formatted,
            "hint": "Increase the approval level or modify the policy",
            "actionable": true,
            "timestamp": current_timestamp(),
            "details": {
                "rule": rule,
                "required_level": required_level,
                "current_level": current_level,
                "context": context
            }
        })),
    )
}

pub fn io_error(operation: &str, path: Option<&Path>, source: impl Into<String>) -> McpError {
    McpError::rpc(
        -32603,
        format!("❌ Operation failed: I/O error during {}", operation),
        Some(json!({
            "code": "E_IO",
            "message": "I/O error during the operation",
            "hint": "Check permissions and available disk space",
            "actionable": true,
            "timestamp": current_timestamp(),
            "details": {
                "operation": operation,
                "path": path.map(|p| p.to_string_lossy().to_string()),
                "source": source.into()
            }
        })),
    )
}

pub fn internal_error(message: impl Into<String>) -> McpError {
    let message = message.into();
    let formatted = format!("❌ Internal error: {}", message);
    McpError::rpc(
        -32603,
        formatted.clone(),
        Some(json!({
            "code": "E_INTERNAL",
            "message": formatted,
            "hint": "Contact technical support with error details",
            "actionable": false,
            "timestamp": current_timestamp(),
            "details": {
                "component": "mcp-tools",
                "message": message,
                "correlation_id": Uuid::new_v4().to_string()
            }
        })),
    )
}

pub fn empty_patch_error() -> McpError {
    build_rpc_error(
        -32602,
        "E_EMPTY_DIFF",
        "❌ Patch failed: empty diff provided",
        "Provide a unified diff containing at least one change.",
        true,
        None,
    )
}

pub fn unsupported_format_error(detected: &str) -> McpError {
    build_rpc_error(
        -32600,
        "E_UNSUPPORTED_FORMAT",
        format!(
            "❌ Patch failed: unsupported diff format detected ({})",
            detected
        ),
        "Generate the patch with `git diff` to get a unified diff.",
        true,
        None,
    )
}

pub fn file_not_found_error(path: impl AsRef<std::path::Path>) -> McpError {
    let path_str = path.as_ref().display().to_string();
    build_rpc_error(
        -32001,
        "E_FILE_MISSING",
        format!(
            "❌ Patch failed: file '{}' not found in workspace",
            path_str
        ),
        "Check that the file exists locally and is tracked by the patch.",
        true,
        Some(json!({ "path": path_str })),
    )
}

fn current_timestamp() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn build_rpc_error(
    rpc_code: i32,
    code: &str,
    message: impl Into<String>,
    hint: impl Into<String>,
    actionable: bool,
    details: Option<Value>,
) -> McpError {
    let message = message.into();
    let hint = hint.into();
    let mut data = json!({
        "code": code,
        "message": message,
        "hint": hint,
        "actionable": actionable,
        "timestamp": current_timestamp(),
    });

    if let Some(details) = details {
        data["details"] = details;
    }

    McpError::rpc(rpc_code, message, Some(data))
}

pub fn git_dirty_error(
    dirty_files: usize,
    modified_files: &[PathBuf],
    branch: Option<&str>,
) -> McpError {
    let mut details = json!({
        "dirty_files": dirty_files,
        "modified_files": modified_files
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect::<Vec<_>>()
    });
    if let Some(branch) = branch {
        details["branch"] = Value::String(branch.to_string());
    }

    build_rpc_error(
        -32001,
        "E_GIT_DIRTY",
        "❌ Patch failed: Git working tree not clean",
        "Clean, stash or commit your local changes before applying the patch.",
        true,
        Some(details),
    )
}

pub fn vcs_conflict_error(
    location: &str,
    conflict_type: &str,
    conflicted_files: &[PathBuf],
    resolution_hint: Option<&str>,
) -> McpError {
    let message = format!(
        "❌ Patch failed: VCS conflict in {} ({})",
        location, conflict_type
    );
    let mut details = json!({
        "location": location,
        "conflict_type": conflict_type,
        "files": conflicted_files
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect::<Vec<_>>(),
    });
    if let Some(hint) = resolution_hint {
        details["resolution_hint"] = Value::String(hint.to_string());
    }

    build_rpc_error(
        -32001,
        "E_VCS_CONFLICT",
        if let Some(hint) = resolution_hint {
            format!("{message} — {hint}")
        } else {
            message
        },
        "Pull the latest version or resolve git conflicts before replaying the patch.",
        true,
        Some(details),
    )
}

pub fn resource_limit_error(
    resource_type: &str,
    current_usage: u64,
    limit: u64,
    unit: &str,
) -> McpError {
    build_rpc_error(
        -32001,
        "E_RESOURCE_LIMIT",
        format!(
            "❌ Patch failed: resource limit exceeded for {} ({} {} used / {} {})",
            resource_type, current_usage, unit, limit, unit
        ),
        "Reduce the patch size or adjust the limit configuration before retrying.",
        true,
        Some(json!({
            "resource_type": resource_type,
            "current_usage": current_usage,
            "limit": limit,
            "unit": unit,
        })),
    )
}

pub fn test_fail_error(failed_count: u32, total_count: u32, test_framework: &str) -> McpError {
    build_rpc_error(
        -32001,
        "E_TEST_FAILURE",
        format!(
            "❌ Patch failed: {} test(s) failed out of {} using {}",
            failed_count, total_count, test_framework
        ),
        "Check the test logs, fix the failures, then retry the patch.",
        true,
        Some(json!({
            "failed": failed_count,
            "total": total_count,
            "framework": test_framework
        })),
    )
}

pub fn test_timeout_error(timeout_secs: u64, test_framework: &str) -> McpError {
    build_rpc_error(
        -32001,
        "E_TEST_TIMEOUT",
        format!(
            "❌ Patch failed: tests timed out after {}s ({})",
            timeout_secs, test_framework
        ),
        "Increase the timeout or optimize the tests to reduce their duration.",
        true,
        Some(json!({
            "timeout_secs": timeout_secs,
            "framework": test_framework
        })),
    )
}
