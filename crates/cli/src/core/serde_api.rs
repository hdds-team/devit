// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2025-2026 naskel.com

//! # DevIt Serialization API
//!
//! Standardized response formats and error mapping for the JSON API.
//! Provides consistent structures for all API responses.

use uuid::Uuid;

use super::errors::DevItError::{self, Io};
pub use devit_common::{StdError, StdResponse};

pub fn std_error_from_devit_error(devit_error: DevItError) -> StdError {
    let (code, message, hint, actionable, details) = map_devit_error_to_std_error(&devit_error);

    let mut error = StdError::new(code, message);
    if let Some(h) = hint {
        error = error.with_hint(h);
    }
    if let Some(flag) = actionable {
        error = error.with_actionable(flag);
    }
    if let Some(payload) = details {
        error = error.with_details(payload);
    }
    error
}

/// Maps a DevItError to StdError components.
///
/// # Arguments
/// * `error` - DevIt error to map
///
/// # Returns
/// Tuple (code, message, hint, actionable, details)
pub fn map_devit_error_to_std_error(
    error: &DevItError,
) -> (
    String,
    String,
    Option<String>,
    Option<bool>,
    Option<serde_json::Value>,
) {
    match error {
        DevItError::InvalidDiff {
            reason,
            line_number,
        } => {
            let mut details = serde_json::Map::new();
            details.insert(
                "reason".to_string(),
                serde_json::Value::String(reason.clone()),
            );
            if let Some(line) = line_number {
                details.insert(
                    "line_number".to_string(),
                    serde_json::Value::Number((*line).into()),
                );
            }

            (
                "E_INVALID_DIFF".to_string(),
                "Invalid or corrupted patch format".to_string(),
                Some("Verify that the patch is a valid unified diff".to_string()),
                Some(true),
                Some(serde_json::Value::Object(details)),
            )
        }

        DevItError::SnapshotRequired {
            operation,
            expected,
        } => {
            let mut details = serde_json::Map::new();
            details.insert(
                "operation".to_string(),
                serde_json::Value::String(operation.clone()),
            );
            details.insert(
                "expected".to_string(),
                serde_json::Value::String(expected.clone()),
            );

            (
                "E_SNAPSHOT_REQUIRED".to_string(),
                "A valid snapshot is required for this operation".to_string(),
                Some("Create a snapshot before continuing".to_string()),
                Some(true),
                Some(serde_json::Value::Object(details)),
            )
        }

        DevItError::SnapshotStale {
            snapshot_id,
            created_at,
            staleness_reason,
        } => {
            let mut details = serde_json::Map::new();
            details.insert(
                "snapshot_id".to_string(),
                serde_json::Value::String(snapshot_id.clone()),
            );
            if let Some(timestamp) = created_at {
                details.insert(
                    "created_at".to_string(),
                    serde_json::Value::String(timestamp.to_rfc3339()),
                );
            }
            if let Some(reason) = staleness_reason {
                details.insert(
                    "staleness_reason".to_string(),
                    serde_json::Value::String(reason.clone()),
                );
            }

            (
                "E_SNAPSHOT_STALE".to_string(),
                "Snapshot is stale compared to current state".to_string(),
                Some("Create a new snapshot or validate the changes".to_string()),
                Some(true),
                Some(serde_json::Value::Object(details)),
            )
        }

        DevItError::PolicyBlock {
            rule,
            required_level,
            current_level,
            context,
        } => {
            let mut details = serde_json::Map::new();
            details.insert("rule".to_string(), serde_json::Value::String(rule.clone()));
            details.insert(
                "required_level".to_string(),
                serde_json::Value::String(required_level.clone()),
            );
            details.insert(
                "current_level".to_string(),
                serde_json::Value::String(current_level.clone()),
            );
            details.insert(
                "context".to_string(),
                serde_json::Value::String(context.clone()),
            );

            (
                "E_POLICY_BLOCK".to_string(),
                "Security policy blocks the requested operation".to_string(),
                Some("Increase approval level or modify the policy".to_string()),
                Some(true),
                Some(serde_json::Value::Object(details)),
            )
        }

        DevItError::ProtectedPath {
            path,
            protection_rule,
            attempted_operation,
        } => {
            let mut details = serde_json::Map::new();
            details.insert(
                "path".to_string(),
                serde_json::Value::String(path.to_string_lossy().to_string()),
            );
            details.insert(
                "protection_rule".to_string(),
                serde_json::Value::String(protection_rule.clone()),
            );
            details.insert(
                "attempted_operation".to_string(),
                serde_json::Value::String(attempted_operation.clone()),
            );

            (
                "E_PROTECTED_PATH".to_string(),
                "Operation affects a protected file or directory".to_string(),
                Some("Use a higher approval level or exclude this path".to_string()),
                Some(true),
                Some(serde_json::Value::Object(details)),
            )
        }

        DevItError::PrivilegeEscalation {
            escalation_type,
            current_privileges,
            attempted_privileges,
            security_context,
        } => {
            let mut details = serde_json::Map::new();
            details.insert(
                "escalation_type".to_string(),
                serde_json::Value::String(escalation_type.clone()),
            );
            details.insert(
                "current_privileges".to_string(),
                serde_json::Value::String(current_privileges.clone()),
            );
            details.insert(
                "attempted_privileges".to_string(),
                serde_json::Value::String(attempted_privileges.clone()),
            );
            details.insert(
                "security_context".to_string(),
                serde_json::Value::String(security_context.clone()),
            );

            (
                "E_PRIV_ESCALATION".to_string(),
                "Operation attempts privilege escalation".to_string(),
                None,
                Some(false),
                Some(serde_json::Value::Object(details)),
            )
        }

        DevItError::GitDirty {
            dirty_files,
            modified_files,
            branch,
        } => {
            let mut details = serde_json::Map::new();
            details.insert(
                "dirty_files".to_string(),
                serde_json::Value::Number((*dirty_files).into()),
            );
            let files: Vec<serde_json::Value> = modified_files
                .iter()
                .map(|p| serde_json::Value::String(p.to_string_lossy().to_string()))
                .collect();
            details.insert(
                "modified_files".to_string(),
                serde_json::Value::Array(files),
            );
            if let Some(branch_name) = branch {
                details.insert(
                    "branch".to_string(),
                    serde_json::Value::String(branch_name.clone()),
                );
            }

            (
                "E_GIT_DIRTY".to_string(),
                "Git working directory has uncommitted changes".to_string(),
                Some("Commit or stash changes before continuing".to_string()),
                Some(true),
                Some(serde_json::Value::Object(details)),
            )
        }

        DevItError::VcsConflict {
            location,
            conflict_type,
            conflicted_files,
            resolution_hint,
        } => {
            let mut details = serde_json::Map::new();
            details.insert(
                "location".to_string(),
                serde_json::Value::String(location.clone()),
            );
            details.insert(
                "conflict_type".to_string(),
                serde_json::Value::String(conflict_type.clone()),
            );
            let files: Vec<serde_json::Value> = conflicted_files
                .iter()
                .map(|p| serde_json::Value::String(p.to_string_lossy().to_string()))
                .collect();
            details.insert(
                "conflicted_files".to_string(),
                serde_json::Value::Array(files),
            );
            if let Some(hint) = resolution_hint {
                details.insert(
                    "resolution_hint".to_string(),
                    serde_json::Value::String(hint.clone()),
                );
            }

            (
                "E_VCS_CONFLICT".to_string(),
                "Conflict detected in version control system".to_string(),
                resolution_hint.clone(),
                Some(true),
                Some(serde_json::Value::Object(details)),
            )
        }

        DevItError::TestFail {
            failed_count,
            total_count,
            test_framework,
            failure_details,
        } => {
            let mut details = serde_json::Map::new();
            details.insert(
                "failed_count".to_string(),
                serde_json::Value::Number((*failed_count).into()),
            );
            details.insert(
                "total_count".to_string(),
                serde_json::Value::Number((*total_count).into()),
            );
            details.insert(
                "test_framework".to_string(),
                serde_json::Value::String(test_framework.clone()),
            );
            let failure_array: Vec<serde_json::Value> = failure_details
                .iter()
                .map(|f| serde_json::Value::String(f.clone()))
                .collect();
            details.insert(
                "failure_details".to_string(),
                serde_json::Value::Array(failure_array),
            );

            (
                "E_TEST_FAIL".to_string(),
                "Test execution failed".to_string(),
                Some("Fix failing tests and retry".to_string()),
                Some(true),
                Some(serde_json::Value::Object(details)),
            )
        }

        DevItError::TestTimeout {
            timeout_secs,
            test_framework,
            running_tests,
        } => {
            let mut details = serde_json::Map::new();
            details.insert(
                "timeout_secs".to_string(),
                serde_json::Value::Number((*timeout_secs).into()),
            );
            details.insert(
                "test_framework".to_string(),
                serde_json::Value::String(test_framework.clone()),
            );
            let tests_array: Vec<serde_json::Value> = running_tests
                .iter()
                .map(|t| serde_json::Value::String(t.clone()))
                .collect();
            details.insert(
                "running_tests".to_string(),
                serde_json::Value::Array(tests_array),
            );

            (
                "E_TEST_TIMEOUT".to_string(),
                "Test execution exceeded time limit".to_string(),
                Some("Increase timeout or optimize tests".to_string()),
                Some(true),
                Some(serde_json::Value::Object(details)),
            )
        }

        DevItError::SandboxDenied {
            reason,
            active_profile,
            attempted_operation,
            violated_policy,
        } => {
            let mut details = serde_json::Map::new();
            details.insert(
                "reason".to_string(),
                serde_json::Value::String(reason.clone()),
            );
            details.insert(
                "active_profile".to_string(),
                serde_json::Value::String(active_profile.clone()),
            );
            details.insert(
                "attempted_operation".to_string(),
                serde_json::Value::String(attempted_operation.clone()),
            );
            if let Some(policy) = violated_policy {
                details.insert(
                    "violated_policy".to_string(),
                    serde_json::Value::String(policy.clone()),
                );
            }

            (
                "E_SANDBOX_DENIED".to_string(),
                "Sandbox policy denied the operation".to_string(),
                Some("Use a less restrictive sandbox profile".to_string()),
                Some(true),
                Some(serde_json::Value::Object(details)),
            )
        }

        DevItError::ResourceLimit {
            resource_type,
            current_usage,
            limit,
            unit,
        } => {
            let mut details = serde_json::Map::new();
            details.insert(
                "resource_type".to_string(),
                serde_json::Value::String(resource_type.clone()),
            );
            details.insert(
                "current_usage".to_string(),
                serde_json::Value::Number((*current_usage).into()),
            );
            details.insert(
                "limit".to_string(),
                serde_json::Value::Number((*limit).into()),
            );
            details.insert("unit".to_string(), serde_json::Value::String(unit.clone()));

            (
                "E_RESOURCE_LIMIT".to_string(),
                "System resource limit exceeded".to_string(),
                Some("Increase limits or optimize resource usage".to_string()),
                Some(true),
                Some(serde_json::Value::Object(details)),
            )
        }

        Io {
            operation,
            path,
            source,
        } => {
            let mut details = serde_json::Map::new();
            details.insert(
                "operation".to_string(),
                serde_json::Value::String(operation.clone()),
            );
            if let Some(p) = path {
                details.insert(
                    "path".to_string(),
                    serde_json::Value::String(p.to_string_lossy().to_string()),
                );
            }
            details.insert(
                "source".to_string(),
                serde_json::Value::String(source.to_string()),
            );

            let is_permission_error = source.kind() == std::io::ErrorKind::PermissionDenied;
            let hint = if is_permission_error {
                Some("Check file access permissions".to_string())
            } else {
                Some("Check that the file exists and is accessible".to_string())
            };

            (
                "E_IO".to_string(),
                "I/O error during the operation".to_string(),
                hint,
                Some(true),
                Some(serde_json::Value::Object(details)),
            )
        }

        DevItError::Internal {
            component,
            message,
            cause,
            correlation_id,
        } => {
            let mut details = serde_json::Map::new();
            details.insert(
                "component".to_string(),
                serde_json::Value::String(component.clone()),
            );
            details.insert(
                "message".to_string(),
                serde_json::Value::String(message.clone()),
            );
            if let Some(cause_desc) = cause {
                details.insert(
                    "cause".to_string(),
                    serde_json::Value::String(cause_desc.clone()),
                );
            }
            details.insert(
                "correlation_id".to_string(),
                serde_json::Value::String(correlation_id.clone()),
            );

            (
                "E_INTERNAL".to_string(),
                "Internal error or unexpected condition".to_string(),
                Some("Contact technical support with error details".to_string()),
                Some(false),
                Some(serde_json::Value::Object(details)),
            )
        }

        DevItError::InvalidTestConfig {
            field,
            value,
            reason,
        } => {
            let mut details = serde_json::Map::new();
            details.insert(
                "field".to_string(),
                serde_json::Value::String(field.clone()),
            );
            details.insert(
                "value".to_string(),
                serde_json::Value::String(value.clone()),
            );
            details.insert(
                "reason".to_string(),
                serde_json::Value::String(reason.clone()),
            );

            (
                "E_INVALID_TEST_CONFIG".to_string(),
                "Invalid test configuration".to_string(),
                Some("Check test configuration parameters".to_string()),
                Some(true),
                Some(serde_json::Value::Object(details)),
            )
        }

        DevItError::InvalidFormat { format, supported } => {
            let mut details = serde_json::Map::new();
            details.insert(
                "format".to_string(),
                serde_json::Value::String(format.clone()),
            );
            details.insert(
                "supported".to_string(),
                serde_json::Value::Array(
                    supported
                        .iter()
                        .map(|s| serde_json::Value::String(s.clone()))
                        .collect(),
                ),
            );

            (
                "E_INVALID_FORMAT".to_string(),
                "Unsupported output format".to_string(),
                Some("Use one of the supported formats".to_string()),
                Some(true),
                Some(serde_json::Value::Object(details)),
            )
        }
    }
}

/// Utility functions for creating common standardized responses.
pub mod responses {
    use super::*;

    /// Creates a simple success response without data.
    ///
    /// # Arguments
    /// * `request_id` - Optional request identifier
    ///
    /// # Returns
    /// Success response with data = ()
    pub fn success_empty(request_id: Option<Uuid>) -> StdResponse<()> {
        StdResponse::success((), request_id)
    }

    /// Creates an error response for a failed validation.
    ///
    /// # Arguments
    /// * `message` - Error message
    /// * `request_id` - Optional request identifier
    ///
    /// # Returns
    /// Validation error response
    pub fn validation_error(message: String, request_id: Option<Uuid>) -> StdResponse<()> {
        let error = StdError::new("E_VALIDATION".to_string(), message).with_actionable(true);
        StdResponse::error(error, request_id)
    }

    /// Creates an error response for a malformed request.
    ///
    /// # Arguments
    /// * `details` - Details about the format error
    /// * `request_id` - Optional request identifier
    ///
    /// # Returns
    /// Format error response
    pub fn malformed_request(details: String, request_id: Option<Uuid>) -> StdResponse<()> {
        let error = StdError::new(
            "E_MALFORMED_REQUEST".to_string(),
            "Malformed request".to_string(),
        )
        .with_hint("Check the JSON format of your request".to_string())
        .with_actionable(true)
        .with_details(serde_json::json!({ "details": details }));

        StdResponse::error(error, request_id)
    }
}
