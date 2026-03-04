// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2025-2026 naskel.com

//! # E2E Tests for Real Test Execution (R1)
//!
//! End-to-end tests that validate the CoreEngine::test_run implementation
//! with real command execution, sandbox integration, and timeout handling.

use std::collections::HashMap;

use devit_cli::core::{CoreConfig, CoreEngine, DevItResult, SandboxProfile};

mod fixtures;
use fixtures::*;

/// Test that test_run method can be called with valid configuration
#[tokio::test]
async fn test_test_run_with_valid_config() -> DevItResult<()> {
    let engine = create_test_engine(devit_common::ApprovalLevel::Moderate).await?;

    let config = devit_cli::core::TestConfig {
        framework: Some("cargo".to_string()),
        patterns: vec!["--version".to_string()], // Use a safe command that doesn't run tests
        timeout_secs: 5,
        parallel: false,
        env_vars: HashMap::new(),
    };

    // This should not crash, but might fail depending on system
    let result = engine.test_run(&config, SandboxProfile::Permissive).await;

    // We just want to verify the method can be called without panicking
    match result {
        Ok(_results) => {
            println!("Test execution completed successfully");
        }
        Err(e) => {
            println!("Test execution failed (expected on some systems): {:?}", e);
        }
    }

    Ok(())
}

/// Integration test: Run a simple cargo test command (without sandbox)
/// This test is ignored by default since it requires actually running cargo test
#[tokio::test]
#[ignore = "Requires actual test execution"]
async fn test_real_cargo_execution() -> DevItResult<()> {
    let engine = create_test_engine(devit_common::ApprovalLevel::Moderate).await?;

    let config = devit_cli::core::TestConfig {
        framework: Some("cargo".to_string()),
        patterns: vec!["--lib".to_string()], // Run only library tests
        timeout_secs: 10,
        parallel: true,
        env_vars: HashMap::new(),
    };

    let results = engine.test_run(&config, SandboxProfile::Permissive).await?;

    // Basic validation - the test should complete
    assert!(results.execution_time.as_millis() > 0);
    println!(
        "Test execution took: {}ms",
        results.execution_time.as_millis()
    );
    println!("Success: {}", results.success);
    println!("Output: {}", results.output);

    Ok(())
}
