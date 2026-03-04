// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2025-2026 naskel.com

// Minimal placeholder for test runner integration used by CLI.
// This stub unblocks formatting/CI while the real runner is refactored.

#[derive(Debug, Clone, Default)]
pub struct ImpactedOpts {
    pub changed_from: Option<String>,
    pub changed_paths: Option<Vec<String>>,
    pub max_jobs: Option<usize>,
    pub framework: Option<String>,
    pub timeout_secs: Option<u64>,
}

#[derive(Debug, Clone, Default)]
pub struct ImpactedReport {
    pub framework: String,
    pub ran: usize,
    pub failed: usize,
    pub logs_path: String,
}

/// Runs impacted tests based on the provided options.
///
/// Placeholder implementation: returns an empty successful report.
pub fn run_impacted(_opts: &ImpactedOpts) -> anyhow::Result<ImpactedReport> {
    Ok(ImpactedReport {
        framework: _opts.framework.clone().unwrap_or_else(|| "auto".into()),
        ran: 0,
        failed: 0,
        logs_path: ".devit/reports/junit.xml".into(),
    })
}
