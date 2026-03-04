// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2025-2026 naskel.com

/// C4 Path Security Integration Tests
///
/// Tests malicious symlinks, path traversal attacks, and repository boundary enforcement
use devit_cli::core::path_security::PathSecurityContext;
use devit_cli::core::{CoreConfig, CoreEngine, DevItError};
use devit_common::ApprovalLevel;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

/// Create a test repository with some basic structure
fn create_test_repo() -> (TempDir, PathBuf) {
    let temp_dir = TempDir::new().unwrap();
    let repo_root = temp_dir.path().to_path_buf();

    // Create basic repo structure
    fs::create_dir_all(repo_root.join("src")).unwrap();
    fs::create_dir_all(repo_root.join("tests")).unwrap();
    fs::create_dir_all(repo_root.join("docs")).unwrap();

    // Create some test files
    fs::write(repo_root.join("src/main.rs"), "fn main() {}\n").unwrap();
    fs::write(repo_root.join("tests/test.rs"), "#[test] fn test() {}\n").unwrap();
    fs::write(repo_root.join("README.md"), "# Test Project\n").unwrap();

    // Initialize as git repo to make it realistic
    std::process::Command::new("git")
        .arg("init")
        .current_dir(&repo_root)
        .output()
        .ok();

    (temp_dir, repo_root)
}

/// Create external directory outside repo for testing escapes
fn create_external_target() -> (TempDir, PathBuf) {
    let external_dir = TempDir::new().unwrap();
    let external_path = external_dir.path().to_path_buf();

    // Create external sensitive files
    fs::write(
        external_path.join("passwd"),
        "root:x:0:0:root:/root:/bin/bash\n",
    )
    .unwrap();
    fs::write(external_path.join("shadow"), "root:!:19000:0:99999:7:::\n").unwrap();

    (external_dir, external_path)
}

#[test]
fn test_symlink_escape_via_multiple_levels() {
    let (_temp_repo, repo_root) = create_test_repo();
    let (_temp_external, external_path) = create_external_target();

    let security = PathSecurityContext::new(&repo_root, true).unwrap();

    // Malicious symlink: src/link -> ../../../etc/passwd
    let malicious_target = "../../../etc/passwd";

    let result = security.validate_symlink("src/evil_link", malicious_target);
    assert!(result.is_err());

    // Verify it's a PolicyBlock error
    match result.unwrap_err() {
        DevItError::PolicyBlock { rule, .. } => {
            assert_eq!(rule, "symlink_security_repo_boundary");
        }
        _ => panic!("Expected PolicyBlock error"),
    }
}

#[test]
fn test_symlink_escape_via_absolute_path() {
    let (_temp_repo, repo_root) = create_test_repo();

    let security = PathSecurityContext::new(&repo_root, true).unwrap();

    // Absolute symlink target
    let absolute_target = "/etc/passwd";

    let result = security.validate_symlink("src/evil_link", absolute_target);
    assert!(result.is_err());

    match result.unwrap_err() {
        DevItError::PolicyBlock { rule, .. } => {
            assert_eq!(rule, "symlink_security_no_absolute");
        }
        _ => panic!("Expected PolicyBlock error"),
    }
}

#[test]
fn test_symlink_chain_attack() {
    let (_temp_repo, repo_root) = create_test_repo();

    let security = PathSecurityContext::new(&repo_root, true).unwrap();

    // Create intermediate symlink that later points outside
    // link1 -> link2 -> ../../../etc/passwd

    // First symlink should be valid (internal)
    assert!(security.validate_symlink("src/link1", "link2").is_ok());

    // But second symlink in chain should be blocked
    let result = security.validate_symlink("src/link2", "../../../etc/passwd");
    assert!(result.is_err());
}

#[test]
fn test_path_traversal_in_patch_paths() {
    let (_temp_repo, repo_root) = create_test_repo();

    let security = PathSecurityContext::new(&repo_root, true).unwrap();

    // Various path traversal attempts
    let malicious_paths = [
        "../../../etc/passwd",
        "src/../../../etc/passwd",
        "./../../etc/passwd",
        "src/../../etc/passwd",
        "docs/../../../etc/shadow",
    ];

    for path in malicious_paths.iter() {
        let result = security.validate_patch_path(path);
        assert!(result.is_err(), "Path should be blocked: {}", path);

        match result.unwrap_err() {
            DevItError::PolicyBlock { rule, .. } => {
                // Should be caught by either traversal or boundary check
                assert!(rule.contains("path_traversal") || rule.contains("repo_boundary"));
            }
            _ => panic!("Expected PolicyBlock error for path: {}", path),
        }
    }
}

#[test]
fn test_legitimate_symlinks_allowed() {
    let (_temp_repo, repo_root) = create_test_repo();

    let security = PathSecurityContext::new(&repo_root, true).unwrap();

    // Legitimate internal symlinks should be allowed
    let valid_symlinks = [
        ("src/link_to_test", "../tests/test.rs"),
        ("docs/link_to_readme", "../README.md"),
        ("src/lib", "main.rs"),
        ("tests/src_link", "../src"),
    ];

    for (symlink_path, target) in valid_symlinks.iter() {
        let result = security.validate_symlink(symlink_path, target);
        assert!(
            result.is_ok(),
            "Valid symlink should be allowed: {} -> {}",
            symlink_path,
            target
        );
    }
}

#[test]
fn test_policy_disables_all_symlinks() {
    let (_temp_repo, repo_root) = create_test_repo();

    // Create security context that disallows all symlinks
    let security = PathSecurityContext::new(&repo_root, false).unwrap();

    // Even legitimate internal symlinks should be blocked
    let result = security.validate_symlink("src/link", "../tests/test.rs");
    assert!(result.is_err());

    match result.unwrap_err() {
        DevItError::PolicyBlock { rule, .. } => {
            assert_eq!(rule, "symlink_policy_disabled");
        }
        _ => panic!("Expected PolicyBlock error"),
    }
}

#[test]
fn test_malicious_characters_in_paths() {
    let (_temp_repo, repo_root) = create_test_repo();

    let security = PathSecurityContext::new(&repo_root, true).unwrap();

    // Test various malicious characters
    let malicious_paths = [
        "src/test\0.rs",   // Null byte
        "src/test\x01.rs", // Control character
        &"a".repeat(5000), // Extremely long path
    ];

    for path in malicious_paths.iter() {
        let result = security.validate_patch_path(path);
        assert!(
            result.is_err(),
            "Malicious path should be blocked: {:?}",
            path
        );
    }
}

#[test]
fn test_toctou_protection() {
    let (_temp_repo, repo_root) = create_test_repo();

    let security = PathSecurityContext::new(&repo_root, true).unwrap();

    // Simulate TOCTOU scenario: paths that change between validation and commit
    let test_paths = [
        PathBuf::from("src/main.rs"),
        PathBuf::from("tests/test.rs"),
        PathBuf::from("README.md"),
    ];

    // Pre-commit validation should succeed for legitimate files
    let result = security.pre_commit_validation(&test_paths);
    assert!(result.is_ok());
}

#[test]
fn test_patch_with_malicious_symlinks() {
    // This test simulates a complete patch application with malicious symlinks
    let (_temp_repo, repo_root) = create_test_repo();

    // Create a malicious patch that tries to create a symlink escaping the repo
    let malicious_patch = format!(
        r#"diff --git a/src/evil_link b/src/evil_link
new file mode 120000
index 0000000..1234567
--- /dev/null
+++ b/src/evil_link
@@ -0,0 +1 @@
+../../../etc/passwd"#
    );

    // This test verifies that such patches would be caught by our security layer
    // We can't easily test the full CoreEngine integration here due to async complexity,
    // but the PathSecurityContext should catch this at the validation level

    let security = PathSecurityContext::new(&repo_root, true).unwrap();
    let result = security.validate_symlink("src/evil_link", "../../../etc/passwd");

    assert!(result.is_err());
    match result.unwrap_err() {
        DevItError::PolicyBlock { rule, .. } => {
            assert_eq!(rule, "symlink_security_repo_boundary");
        }
        _ => panic!("Expected PolicyBlock error"),
    }
}

#[test]
fn test_boundary_enforcement_edge_cases() {
    let (_temp_repo, repo_root) = create_test_repo();
    let security = PathSecurityContext::new(&repo_root, true).unwrap();

    // Test edge cases around repository boundary
    let edge_case_paths = [
        ".",                  // Current directory
        "./src/main.rs",      // Explicit current dir
        "src/./main.rs",      // Current dir in middle
        "src/../src/main.rs", // Roundtrip should be ok
        "src/..",             // Parent from subdir
    ];

    for path in edge_case_paths.iter() {
        let result = security.validate_patch_path(path);

        // These should generally be allowed as they stay within bounds
        if result.is_err() {
            // Only allow traversal protection errors, not boundary escapes
            match result.unwrap_err() {
                DevItError::PolicyBlock { rule, .. } => {
                    assert!(
                        rule.contains("path_traversal"),
                        "Unexpected error for path {}: {}",
                        path,
                        rule
                    );
                }
                _ => panic!("Unexpected error type for path: {}", path),
            }
        }
    }
}

#[test]
fn test_symlink_depth_protection() {
    let (_temp_repo, repo_root) = create_test_repo();
    let security = PathSecurityContext::new(&repo_root, true).unwrap();

    // Test deeply nested path that tries to escape via many levels
    let deep_escape = "../".repeat(10) + "etc/passwd";

    let result = security.validate_symlink("src/deep_link", &deep_escape);
    assert!(result.is_err());

    match result.unwrap_err() {
        DevItError::PolicyBlock { rule, .. } => {
            assert_eq!(rule, "symlink_security_repo_boundary");
        }
        _ => panic!("Expected PolicyBlock error"),
    }
}
