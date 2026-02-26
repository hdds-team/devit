// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2025-2026 naskel.com

//! AIRCP Unified Tool - Single entry point for all AIRCP operations.
//!
//! All AIRCP functionality is now available through a single tool: `devit_aircp`
//! Use the `command` parameter to specify the operation.
//!
//! Commands:
//! - Core: status, send, history, join
//! - Daemon: claim, lock, presence
//! - Task: task/list, task/create, task/activity, task/complete
//! - Brainstorm: brainstorm/create, brainstorm/vote, brainstorm/status, brainstorm/list
//! - Mode: mode/status, mode/set, mode/history, ask, stop, handover
//! - Workflow: workflow/status, workflow/config, workflow/start, workflow/next, workflow/extend, workflow/skip, workflow/abort, workflow/history

mod unified;

pub use unified::AircpTool;
