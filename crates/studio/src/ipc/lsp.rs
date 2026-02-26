// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2025-2026 naskel.com

//! LSP IPC commands - Language Server Protocol bridge

use crate::lsp::{get_server_config, LspRegistry};
use lsp_types::CompletionItemKind;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::State;
use tokio::sync::RwLock;

/// LSP registry shared state (async-compatible)
pub type LspState = Arc<RwLock<LspRegistry>>;

#[derive(serde::Serialize)]
pub struct Completion {
    pub label: String,
    pub kind: String,
    pub detail: Option<String>,
    pub insert_text: String,
}

#[derive(serde::Serialize)]
pub struct HoverInfo {
    pub contents: String,
    pub range: Option<Range>,
}

#[derive(serde::Serialize)]
pub struct Range {
    pub start_line: u32,
    pub start_col: u32,
    pub end_line: u32,
    pub end_col: u32,
}

#[derive(serde::Serialize)]
pub struct Diagnostic {
    pub message: String,
    pub severity: String,
    pub range: Range,
    pub source: Option<String>,
}

#[derive(serde::Serialize)]
pub struct LspStatus {
    pub language: String,
    pub server: String,
    pub running: bool,
}

/// Start LSP server for a language
#[tauri::command]
pub async fn start_lsp(
    language: String,
    workspace_path: String,
    state: State<'_, LspState>,
) -> Result<LspStatus, String> {
    let config = get_server_config(&language)
        .ok_or_else(|| format!("No LSP configured for {}", language))?;

    let server = config.server_command.clone();

    let mut registry = state.write().await;
    registry
        .start(&language, &PathBuf::from(&workspace_path))
        .await?;

    tracing::info!(
        "Started LSP {} for {} in {}",
        server,
        language,
        workspace_path
    );

    Ok(LspStatus {
        language,
        server,
        running: true,
    })
}

/// Stop LSP server
#[tauri::command]
pub async fn stop_lsp(language: String, state: State<'_, LspState>) -> Result<(), String> {
    let mut registry = state.write().await;
    registry.stop(&language).await?;
    tracing::info!("Stopped LSP for {}", language);
    Ok(())
}

/// Get completions at position
#[tauri::command]
pub async fn get_completions(
    file_path: String,
    line: u32,
    column: u32,
    state: State<'_, LspState>,
) -> Result<Vec<Completion>, String> {
    let mut registry = state.write().await;
    let items = registry.completions(&file_path, line, column).await?;

    Ok(items
        .into_iter()
        .map(|item| Completion {
            label: item.label.clone(),
            kind: completion_kind_to_string(item.kind),
            detail: item.detail.clone(),
            insert_text: item.insert_text.unwrap_or(item.label),
        })
        .collect())
}

fn completion_kind_to_string(kind: Option<CompletionItemKind>) -> String {
    match kind {
        Some(CompletionItemKind::TEXT) => "text",
        Some(CompletionItemKind::METHOD) => "method",
        Some(CompletionItemKind::FUNCTION) => "function",
        Some(CompletionItemKind::CONSTRUCTOR) => "constructor",
        Some(CompletionItemKind::FIELD) => "field",
        Some(CompletionItemKind::VARIABLE) => "variable",
        Some(CompletionItemKind::CLASS) => "class",
        Some(CompletionItemKind::INTERFACE) => "interface",
        Some(CompletionItemKind::MODULE) => "module",
        Some(CompletionItemKind::PROPERTY) => "property",
        Some(CompletionItemKind::UNIT) => "unit",
        Some(CompletionItemKind::VALUE) => "value",
        Some(CompletionItemKind::ENUM) => "enum",
        Some(CompletionItemKind::KEYWORD) => "keyword",
        Some(CompletionItemKind::SNIPPET) => "snippet",
        Some(CompletionItemKind::COLOR) => "color",
        Some(CompletionItemKind::FILE) => "file",
        Some(CompletionItemKind::REFERENCE) => "reference",
        Some(CompletionItemKind::FOLDER) => "folder",
        Some(CompletionItemKind::ENUM_MEMBER) => "enumMember",
        Some(CompletionItemKind::CONSTANT) => "constant",
        Some(CompletionItemKind::STRUCT) => "struct",
        Some(CompletionItemKind::EVENT) => "event",
        Some(CompletionItemKind::OPERATOR) => "operator",
        Some(CompletionItemKind::TYPE_PARAMETER) => "typeParameter",
        _ => "unknown",
    }
    .into()
}

/// Get hover info at position
#[tauri::command]
pub async fn get_hover(
    file_path: String,
    line: u32,
    column: u32,
    state: State<'_, LspState>,
) -> Result<Option<HoverInfo>, String> {
    let mut registry = state.write().await;
    let hover = registry.hover(&file_path, line, column).await?;

    Ok(hover.map(|h| {
        let contents = match h.contents {
            lsp_types::HoverContents::Scalar(markup) => extract_markup_content(&markup),
            lsp_types::HoverContents::Array(markups) => markups
                .into_iter()
                .map(|m| extract_markup_content(&m))
                .collect::<Vec<_>>()
                .join("\n\n"),
            lsp_types::HoverContents::Markup(content) => content.value,
        };

        let range = h.range.map(|r| Range {
            start_line: r.start.line,
            start_col: r.start.character,
            end_line: r.end.line,
            end_col: r.end.character,
        });

        HoverInfo { contents, range }
    }))
}

fn extract_markup_content(content: &lsp_types::MarkedString) -> String {
    match content {
        lsp_types::MarkedString::String(s) => s.clone(),
        lsp_types::MarkedString::LanguageString(ls) => {
            format!("```{}\n{}\n```", ls.language, ls.value)
        }
    }
}

/// Get diagnostics for file (from cached LSP notifications)
#[tauri::command]
pub async fn get_diagnostics(
    file_path: String,
    state: State<'_, LspState>,
) -> Result<Vec<Diagnostic>, String> {
    let registry = state.read().await;
    let lsp_diagnostics = registry.diagnostics(&file_path).await?;

    Ok(lsp_diagnostics
        .into_iter()
        .map(|d| Diagnostic {
            message: d.message,
            severity: match d.severity {
                Some(lsp_types::DiagnosticSeverity::ERROR) => "error",
                Some(lsp_types::DiagnosticSeverity::WARNING) => "warning",
                Some(lsp_types::DiagnosticSeverity::INFORMATION) => "info",
                Some(lsp_types::DiagnosticSeverity::HINT) => "hint",
                _ => "unknown",
            }
            .into(),
            range: Range {
                start_line: d.range.start.line,
                start_col: d.range.start.character,
                end_line: d.range.end.line,
                end_col: d.range.end.character,
            },
            source: d.source,
        })
        .collect())
}
