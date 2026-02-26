// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2025-2026 naskel.com

//! RC1 Integration Tests - Policies & Approvals
//! Tests for active policy enforcement with approval flow

mod common;
use common::*;

#[test]
fn policy_deny_forbidden_path() {
    let t = Tmp::new();
    // policy minimaliste
    t.write(
        ".devit/policies.toml",
        r#"
[tools.devit_patch_apply]
default = "on_request"
paths_forbidden = ["/etc", "/var"]
"#,
    );

    let diff = "\
--- /etc/shadow.orig\t2025-01-01 10:00:00 +0100
+++ /etc/shadow\t2025-01-01 10:01:00 +0100
@@ -1,1 +1,1 @@
-a
+b
";
    let cmd = format!("devit patch-apply <<'EOF'\n{}\nEOF", diff);
    let o = run(cmd, t.path());
    assert!(!o.status.success(), "must be denied");
}

#[test]
fn policy_on_request_via_approver_flow() {
    let t = Tmp::new();
    t.write("src/a.txt", "A\n");
    t.write(
        ".devit/policies.toml",
        r#"
[tools.devit_patch_apply]
default = "on_request"
paths_trusted = ["src"]
"#,
    );

    // No explicit approver needed: paths_trusted = ["src"] means
    // patch-apply auto-approves for paths under src/ with on_request policy.

    let diff = "\
--- src/a.txt.orig\t2025-01-01 10:00:00 +0100
+++ src/a.txt\t2025-01-01 10:01:00 +0100
@@ -1,1 +1,1 @@
-A
+B
";
    let o = run(
        &format!("devit patch-apply <<'EOF'\n{}\nEOF", diff),
        t.path(),
    );
    assert!(o.status.success(), "approval flow failed");
    assert_eq!(t.read("src/a.txt"), "B\n");
}
