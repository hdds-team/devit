// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2025-2026 naskel.com

//! Monitor orchestration tasks via the daemon backend.
//! Run with: `cargo run --example daemon_monitoring`

use anyhow::{Context, Result};
use devit_common::orchestration::{OrchestrationConfig, OrchestrationContext};
use devit_orchestration::types::{OrchestrationMode, StatusFilter};
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<()> {
    println!("\n--- DevIt Orchestration - Daemon Monitoring ---\n");

    let mut config = OrchestrationConfig::default();
    config.mode = OrchestrationMode::Daemon;
    config.auto_start_daemon = true;

    let ctx = OrchestrationContext::new(config)
        .await
        .context("failed to initialise orchestration context")?;

    if !ctx.is_using_daemon() {
        println!("[WARN] Daemon backend unavailable -- falling back to local mode.");
        println!("Start devitd manually to see live updates.");
        return Ok(());
    }

    loop {
        let status = ctx.status(Some("all")).await?;
        print!("\x1B[2J\x1B[1;1H"); // clear screen
        println!(
            "Active: {} | Completed: {} | Failed: {}",
            status.summary.total_active,
            status.summary.total_completed,
            status.summary.total_failed,
        );

        if !status.active_tasks.is_empty() {
            println!("\n>> Active Tasks");
            for task in &status.active_tasks {
                println!(
                    "  • {} → {} (status: {:?})",
                    task.id, task.delegated_to, task.status
                );
            }
        }

        if !status.completed_tasks.is_empty() {
            println!("\n>> Completed Tasks");
            for task in &status.completed_tasks {
                println!(
                    "  • {} → {} (final: {:?})",
                    task.id, task.delegated_to, task.status
                );
            }
        }

        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}
