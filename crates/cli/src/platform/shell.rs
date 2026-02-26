// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2025-2026 naskel.com

use std::process::Command;

/// Returns a platform-appropriate shell command to execute `script`.
/// - Unix: `bash -c <script>`
/// - Windows: `bash -c <script>` if Git Bash is available, otherwise `cmd /C <script>`
///
/// On Windows, MCP server commands often contain bash-specific syntax
/// (pipes, single-quotes, heredocs). We prefer bash via Git Bash when
/// available so those commands work unchanged.
pub fn shell_command(script: &str) -> Command {
    #[cfg(unix)]
    {
        let mut cmd = Command::new("bash");
        cmd.arg("-c").arg(script);
        cmd
    }
    #[cfg(windows)]
    {
        // Prefer bash (Git Bash) for compatibility with bash-specific MCP commands.
        // Fall back to cmd.exe if bash is not on PATH.
        if bash_available_on_windows() {
            let mut cmd = Command::new("bash");
            cmd.arg("-c").arg(script);
            cmd
        } else {
            let mut cmd = Command::new("cmd");
            cmd.arg("/C").arg(script);
            cmd
        }
    }
}

/// Check if bash is available on Windows (Git Bash, WSL, etc.)
#[cfg(windows)]
fn bash_available_on_windows() -> bool {
    use std::sync::OnceLock;
    static AVAILABLE: OnceLock<bool> = OnceLock::new();
    *AVAILABLE.get_or_init(|| {
        Command::new("bash")
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    })
}
