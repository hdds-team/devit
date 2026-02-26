// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2025-2026 naskel.com

//! # DevIt CLI Library
//!
//! Core functionality for the DevIt CLI application.

pub mod capabilities;
pub mod core;
pub mod platform;

// Re-export core types for convenience
pub use core::{
    ApprovalLevel, CoreConfig, CoreEngine, DevItError, DevItResult, FileChange, FileChangeKind,
    JournalEntry, JournalOperationType, PatchPreview, PatchResult, PolicyContext, PolicyDecision,
    PolicyEngineConfig, SandboxProfile, SnapshotId, StdError, StdResponse, TestConfig, TestResults,
};

// Re-export path security types
pub use core::path_security::{PathSecurityContext, PathSecurityViolation};

// Re-export capabilities types
pub use capabilities::{SandboxCapabilities, SystemCapabilities, SystemLimits, VcsCapabilities};
