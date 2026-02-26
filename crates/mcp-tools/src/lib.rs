// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2025-2026 naskel.com

#![recursion_limit = "256"]
//! Collection de tools MCP réalistes basés sur des opérations locales.

use std::env;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use mcp_core::{McpResult, McpTool};
use tracing::warn;

mod atomic_patcher;
mod directory_list;
mod errors;
mod exec;
mod explorer;
mod fetch_url;
mod file_explore;
pub mod file_read;
mod file_write;
mod git;
mod help;
mod journal;
mod journal_best_effort;
mod memory;
#[cfg(target_os = "linux")]
mod keyboard;
mod kill;
#[cfg(target_os = "linux")]
mod mouse;
mod net_utils;
mod ocr;
mod ocr_alerts;
mod orchestration;
mod output_shaper;
mod patch_apply;
mod ps;
mod pwd;
mod resource_monitor;
mod screenshot;
mod search_web;
#[cfg(any(test, feature = "test-utils"))]
pub use search_web::__test_domain_of;
mod aircp;
mod archive;
mod cargo;
mod clipboard;
mod db_query;
mod docker;
mod doctor;
mod file_ops;
mod forum;
mod git_ops;
mod ports;
mod read_image;
mod read_pdf;
mod shell;
mod snapshot;
mod test_run;
mod window_manager;
mod worker;

use devit_cli::core::config::CoreConfig;
use exec::DevitExec;
#[cfg(target_os = "linux")]
use keyboard::KeyboardTool;
use kill::DevitKill;
#[cfg(target_os = "linux")]
use mouse::MouseTool;
use ps::DevitPs;

pub use aircp::AircpTool;
pub use cargo::CargoTool;
pub use devit_common::orchestration::{
    format_status, OrchestrationConfig, OrchestrationContext, OrchestrationMode, StatusFormat,
};
pub use directory_list::DirectoryListTool;
pub use errors::{
    desktop_env_error, internal_error, invalid_diff_error, io_error, policy_block_error,
    validation_error,
};
pub use explorer::{ExplorationResult, ExplorerConfig, ExplorerMode, ExplorerTool};
pub use file_explore::{
    FileExplorer, FileListExtTool, FileListTool, FileSearchExtTool, FileSearchTool,
    ProjectStructureExtTool, ProjectStructureTool,
};
pub use file_read::{FileReadTool, FileSystemContext};
pub use file_write::FileWriteTool;
pub use forum::{ForumPostTool, ForumPostsTool, ForumStatusTool};
pub use git::{GitBlameTool, GitDiffTool, GitLogTool, GitSearchTool, GitShowTool};
pub use help::{HelpAllTool, HelpTool};
pub use journal::{JournalAppendResult, JournalAppendTool, JournalContext};
pub use memory::{MemoryContext, MemoryTool, SoulTool};
pub use orchestration::{DelegateTool, NotifyTool, OrchestrationStatusTool, TaskResultTool};
pub use patch_apply::{PatchApplyTool, PatchContext};
pub use pwd::PwdTool;
pub use resource_monitor::ResourceMonitor;
pub use screenshot::ScreenshotTool;
pub use shell::ShellTool;
pub use snapshot::{SnapshotContext, SnapshotTool};
pub use test_run::{TestRunContext, TestRunTool};
pub use window_manager::{
    WindowFocusTool, WindowGetContentTool, WindowListTool, WindowScreenshotTool, WindowSendTextTool,
};
pub use worker::{PollTasksTool, ToolOptions, WorkerBridge, WorkerTask};

// Test-only helpers re-export (S1 harness Option A)
#[cfg(any(test, feature = "test-utils"))]
pub mod test_helpers {
    pub use crate::net_utils::{
        detect_injection_text, detect_paywall_hint, extract_article_content, robots_policy_for,
        sanitize_html_to_text, ArticleContent, ExtractMode, RobotsPolicy,
    };
    pub use crate::search_web::SearchResult;
}

/// Construit l'ensemble de tools MCP prêts à l'emploi pour un répertoire projet.
pub async fn default_tools(root_path: PathBuf) -> McpResult<Vec<Arc<dyn McpTool>>> {
    default_tools_with_options(root_path, ToolOptions::default()).await
}

/// Variante acceptant des options avancées (worker-mode, etc.).
pub async fn default_tools_with_options(
    root_path: PathBuf,
    options: ToolOptions,
) -> McpResult<Vec<Arc<dyn McpTool>>> {
    let ToolOptions {
        worker_bridge,
        exec_config: provided_exec_config,
        sandbox_root: provided_sandbox_root,
        allowed_paths,
    } = options;

    let file_context = Arc::new(FileSystemContext::with_allowed_paths(
        root_path.clone(),
        allowed_paths,
    )?);
    let dir_context = Arc::clone(&file_context);
    let patch_context = Arc::new(PatchContext::new(root_path.clone())?);
    let test_context = Arc::new(TestRunContext::new(root_path.clone())?);
    let snapshot_context = Arc::new(SnapshotContext::new(root_path)?);
    let journal_context = Arc::new(JournalContext::new(Arc::clone(&file_context))?);
    let memory_context = Arc::new(MemoryContext::new(Arc::clone(&file_context))?);
    let mut core_config =
        load_core_config(file_context.root()).map_err(|err| internal_error(err.to_string()))?;
    apply_orchestration_env_overrides(&mut core_config.orchestration.base);
    let orchestration_context = Arc::new(
        OrchestrationContext::new(core_config.orchestration.base.clone())
            .await
            .map_err(|err| internal_error(err.to_string()))?,
    );

    let file_tool = FileReadTool::new(Arc::clone(&file_context));
    let file_tool_ext = FileReadTool::new_extended(Arc::clone(&file_context));
    let explorer = Arc::new(FileExplorer::new(Arc::clone(&file_context))?);
    let file_write_tool = FileWriteTool::new(Arc::clone(&file_context))?;
    let patch_tool = PatchApplyTool::new(patch_context);
    let test_tool = TestRunTool::new(test_context);
    let snapshot_tool = SnapshotTool::new(snapshot_context);
    let journal_tool = JournalAppendTool::new(journal_context);
    let delegate_tool = DelegateTool::new(
        Arc::clone(&orchestration_context),
        Arc::clone(&file_context),
    );

    let notify_tool: Arc<dyn McpTool> = if let Some(worker) = worker_bridge.as_ref() {
        Arc::new(NotifyTool::with_worker(
            Arc::clone(&orchestration_context),
            Arc::clone(worker),
        ))
    } else {
        Arc::new(NotifyTool::new(Arc::clone(&orchestration_context)))
    };
    let status_tool = OrchestrationStatusTool::new(Arc::clone(&orchestration_context));
    let task_result_tool = TaskResultTool::new(Arc::clone(&orchestration_context));
    let git_log = GitLogTool::new(Arc::clone(&file_context));
    let git_blame = GitBlameTool::new(Arc::clone(&file_context));
    let git_show = GitShowTool::new(Arc::clone(&file_context));
    let git_diff = GitDiffTool::new(Arc::clone(&file_context));
    let git_search = GitSearchTool::new(Arc::clone(&file_context));
    let ocr_tool = ocr::OcrTool::new(Arc::clone(&file_context));
    let ocr_alerts_tool = ocr_alerts::OcrAlertsTool::new(
        Arc::clone(&file_context),
        Arc::clone(&orchestration_context),
    );
    let web_search_tool: Arc<dyn McpTool> = search_web::SearchWebTool::new_default();
    let fetch_url_tool: Arc<dyn McpTool> = fetch_url::FetchUrlTool::new();
    let exec_config = provided_exec_config.unwrap_or_else(|| core_config.tools.exec.clone());
    let sandbox_root = provided_sandbox_root.unwrap_or_else(|| file_context.root().to_path_buf());
    let exec_tool: Arc<dyn McpTool> = Arc::new(DevitExec::with_config(
        exec_config.clone(),
        sandbox_root.clone(),
    )?);
    let shell_tool: Arc<dyn McpTool> =
        Arc::new(shell::ShellTool::with_config(exec_config, sandbox_root)?);
    let ps_tool: Arc<dyn McpTool> = Arc::new(DevitPs::new());
    let kill_tool: Arc<dyn McpTool> = Arc::new(DevitKill::new());
    let resource_monitor_tool: Arc<dyn McpTool> = Arc::new(ResourceMonitor::new());

    // Window manager tools (X11)
    let window_list_tool: Arc<dyn McpTool> = Arc::new(WindowListTool::new());
    let window_send_text_tool: Arc<dyn McpTool> = Arc::new(WindowSendTextTool::new());
    let window_focus_tool: Arc<dyn McpTool> = Arc::new(WindowFocusTool::new());
    let window_screenshot_tool: Arc<dyn McpTool> = Arc::new(WindowScreenshotTool::new());
    let window_get_content_tool: Arc<dyn McpTool> = Arc::new(WindowGetContentTool::new());

    let explorer_tool = ExplorerTool::new(Arc::clone(&explorer));

    let mut tools: Vec<Arc<dyn McpTool>> = vec![
        Arc::new(file_tool),
        Arc::new(file_tool_ext),
        Arc::new(DirectoryListTool::new(dir_context)),
        Arc::new(FileListTool::new(Arc::clone(&explorer))),
        Arc::new(FileSearchTool::new(Arc::clone(&explorer))),
        Arc::new(ProjectStructureTool::new(Arc::clone(&explorer))),
        Arc::new(FileListExtTool::new(Arc::clone(&explorer))),
        Arc::new(FileSearchExtTool::new(Arc::clone(&explorer))),
        Arc::new(ProjectStructureExtTool::new(Arc::clone(&explorer))),
        Arc::new(HelpTool::new(Arc::clone(&file_context))),
        Arc::new(HelpAllTool::new()),
        Arc::new(explorer_tool),
        Arc::new(file_write_tool),
        Arc::new(patch_tool),
        Arc::new(test_tool),
        Arc::new(snapshot_tool),
        Arc::new(journal_tool),
        Arc::new(MemoryTool::new(Arc::clone(&memory_context))),
        Arc::new(SoulTool::new(file_context.root().to_path_buf())),
        Arc::new(delegate_tool),
        notify_tool,
        Arc::new(status_tool),
        Arc::new(task_result_tool),
        Arc::new(PwdTool::new(Arc::clone(&file_context))),
        Arc::new(git_log),
        Arc::new(git_blame),
        Arc::new(git_show),
        Arc::new(git_diff),
        Arc::new(git_search),
        Arc::new(ocr_tool),
        Arc::new(ocr_alerts_tool),
        web_search_tool,
        fetch_url_tool,
        exec_tool,
        shell_tool,
        ps_tool,
        kill_tool,
        resource_monitor_tool,
        window_list_tool,
        window_send_text_tool,
        window_focus_tool,
        window_screenshot_tool,
        window_get_content_tool,
        // Image & PDF readers for LLM vision
        Arc::new(read_image::ReadImageTool::new(Arc::clone(&file_context))),
        Arc::new(read_pdf::ReadPdfTool::new(Arc::clone(&file_context))),
        // Local dev utilities
        Arc::new(clipboard::ClipboardTool::new()),
        Arc::new(ports::PortsTool::new()),
        Arc::new(docker::DockerTool::new()),
        Arc::new(db_query::DbQueryTool::new(Arc::clone(&file_context))),
        Arc::new(archive::ArchiveTool::new(Arc::clone(&file_context))),
    ];

    if let Some(worker) = worker_bridge {
        tools.push(Arc::new(PollTasksTool::new(worker)));
    }

    if let Ok(Some(screenshot_tool)) = ScreenshotTool::from_config(
        &core_config.tools.screenshot,
        &core_config.orchestration.base,
    ) {
        tools.push(Arc::new(screenshot_tool));
    }

    #[cfg(target_os = "linux")]
    {
        tools.push(Arc::new(MouseTool::new()));
        tools.push(Arc::new(KeyboardTool::new()));
    }

    // Claude Desktop convenience tools — gated by DEVIT_CLAUDE_DESKTOP=true|1
    if is_claude_desktop_mode() {
        tools.push(Arc::new(CargoTool::new(Arc::clone(&file_context))));
        tools.push(Arc::new(git_ops::GitOpsTool::new(Arc::clone(
            &file_context,
        ))));
        tools.push(Arc::new(doctor::DoctorTool::new(Arc::clone(&file_context))));
        tools.push(Arc::new(file_ops::FileOpsTool::new(Arc::clone(
            &file_context,
        ))));
        tracing::info!(target: "devit_mcp_tools", "DEVIT_CLAUDE_DESKTOP=true → convenience tools enabled (cargo, git, doctor, file_ops)");
    }

    // AIRCP tools (experimental) — gated by DEVIT_AIRCP=true|1
    if is_aircp_mode() {
        tools.push(Arc::new(AircpTool::new()));
        tools.push(Arc::new(ForumPostsTool::new()));
        tools.push(Arc::new(ForumPostTool::new()));
        tools.push(Arc::new(ForumStatusTool::new()));
        tracing::info!(target: "devit_mcp_tools", "DEVIT_AIRCP=true → AIRCP tools enabled (aircp, forum)");
    }

    Ok(tools)
}

fn load_core_config(root_path: &Path) -> Result<CoreConfig, String> {
    match resolve_core_config(root_path) {
        Some(path) => match CoreConfig::from_file(&path) {
            Ok(cfg) => Ok(cfg),
            Err(err) => {
                warn!(
                    "Failed to load core config at {}: {} (falling back to defaults)",
                    path.display(),
                    err
                );
                Ok(CoreConfig::default())
            }
        },
        None => Ok(CoreConfig::default()),
    }
}

fn resolve_core_config(root_path: &Path) -> Option<PathBuf> {
    if let Ok(path) = env::var("DEVIT_CORE_CONFIG") {
        let candidate = PathBuf::from(path);
        if candidate.exists() {
            return Some(candidate);
        }
    }

    let candidates = [
        root_path.join("devit.core.toml"),
        root_path.join(".devit.core.toml"),
        root_path.join(".devit").join("devit.core.toml"),
    ];

    for candidate in candidates {
        if candidate.exists() {
            return Some(candidate);
        }
    }

    None
}

fn apply_orchestration_env_overrides(config: &mut OrchestrationConfig) {
    if let Ok(socket) = env::var("DEVIT_DAEMON_SOCKET") {
        config.daemon_socket = Some(socket);
    }

    let mode_override = env::var("DEVIT_ORCHESTRATION_MODE")
        .ok()
        .and_then(|value| parse_mode(&value));

    if let Some(mode) = mode_override {
        config.mode = mode;
    } else if env::var("DEVIT_DAEMON_SOCKET").is_ok() {
        config.mode = OrchestrationMode::Daemon;
    }
}

/// Check if Claude Desktop convenience tools should be exposed
fn is_claude_desktop_mode() -> bool {
    env::var("DEVIT_CLAUDE_DESKTOP")
        .map(|v| v == "true" || v == "1")
        .unwrap_or(false)
}

/// Check if AIRCP tools should be exposed
fn is_aircp_mode() -> bool {
    env::var("DEVIT_AIRCP")
        .map(|v| v == "true" || v == "1")
        .unwrap_or(false)
}

fn parse_mode(value: &str) -> Option<OrchestrationMode> {
    match value.to_lowercase().as_str() {
        "local" => Some(OrchestrationMode::Local),
        "daemon" => Some(OrchestrationMode::Daemon),
        "auto" => Some(OrchestrationMode::Auto),
        _ => {
            warn!(
                "Unknown DEVIT_ORCHESTRATION_MODE '{}', keeping existing setting",
                value
            );
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::sync::Mutex;
    use tempfile::TempDir;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    struct EnvVarGuard {
        key: &'static str,
        previous: Option<String>,
    }

    impl EnvVarGuard {
        fn set(key: &'static str, value: Option<&str>) -> Self {
            let previous = std::env::var(key).ok();
            match value {
                Some(v) => std::env::set_var(key, v),
                None => std::env::remove_var(key),
            }
            Self { key, previous }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            if let Some(value) = self.previous.as_deref() {
                std::env::set_var(self.key, value);
            } else {
                std::env::remove_var(self.key);
            }
        }
    }

    fn collect_tool_names(claude_flag: Option<&str>, aircp_flag: Option<&str>) -> HashSet<String> {
        let _lock = ENV_LOCK.lock().expect("env lock");
        let _claude = EnvVarGuard::set("DEVIT_CLAUDE_DESKTOP", claude_flag);
        let _aircp = EnvVarGuard::set("DEVIT_AIRCP", aircp_flag);
        let _core_config = EnvVarGuard::set("DEVIT_CORE_CONFIG", None);

        let root = TempDir::new().expect("temp dir");
        let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
        let tools = runtime
            .block_on(default_tools_with_options(
                root.path().to_path_buf(),
                ToolOptions::default(),
            ))
            .expect("default tools");

        tools
            .into_iter()
            .map(|tool| tool.name().to_string())
            .collect()
    }

    #[test]
    fn registry_excludes_flagged_tools_by_default() {
        let names = collect_tool_names(None, None);

        assert!(!names.contains("devit_aircp"));
        assert!(!names.contains("devit_forum_posts"));
        assert!(!names.contains("devit_forum_post"));
        assert!(!names.contains("devit_forum_status"));

        assert!(!names.contains("devit_cargo"));
        assert!(!names.contains("devit_git"));
        assert!(!names.contains("devit_doctor"));
        assert!(!names.contains("devit_file_ops"));
    }

    #[test]
    fn registry_includes_aircp_tools_when_flag_enabled() {
        let names = collect_tool_names(None, Some("1"));

        assert!(names.contains("devit_aircp"));
        assert!(names.contains("devit_forum_posts"));
        assert!(names.contains("devit_forum_post"));
        assert!(names.contains("devit_forum_status"));

        assert!(!names.contains("devit_cargo"));
        assert!(!names.contains("devit_git"));
        assert!(!names.contains("devit_doctor"));
        assert!(!names.contains("devit_file_ops"));
    }

    #[test]
    fn registry_includes_claude_desktop_tools_when_flag_enabled() {
        let names = collect_tool_names(Some("1"), None);

        assert!(names.contains("devit_cargo"));
        assert!(names.contains("devit_git"));
        assert!(names.contains("devit_doctor"));
        assert!(names.contains("devit_file_ops"));

        assert!(!names.contains("devit_aircp"));
        assert!(!names.contains("devit_forum_posts"));
        assert!(!names.contains("devit_forum_post"));
        assert!(!names.contains("devit_forum_status"));
    }
}
