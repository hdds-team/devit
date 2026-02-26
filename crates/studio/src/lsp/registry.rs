// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2025-2026 naskel.com

//! LSP registry - manages multiple LSP clients

use super::client::{get_server_config, LspClient};
use lsp_types::{CompletionItem, Hover};
use std::collections::HashMap;
use std::path::PathBuf;

/// Registry of active LSP clients
pub struct LspRegistry {
    clients: HashMap<String, LspClient>,
}

impl Default for LspRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl LspRegistry {
    pub fn new() -> Self {
        Self {
            clients: HashMap::new(),
        }
    }

    /// Start LSP for a language
    pub async fn start(&mut self, language: &str, workspace: &PathBuf) -> Result<(), String> {
        if self.clients.contains_key(language) {
            return Ok(()); // Already running
        }

        let config = get_server_config(language)
            .ok_or_else(|| format!("No LSP configured for {}", language))?;

        let mut client = LspClient::new(config, workspace.clone());
        client.start().await?;

        self.clients.insert(language.to_string(), client);
        Ok(())
    }

    /// Stop LSP for a language
    pub async fn stop(&mut self, language: &str) -> Result<(), String> {
        if let Some(mut client) = self.clients.remove(language) {
            client.stop().await?;
        }
        Ok(())
    }

    /// Stop all LSP servers
    pub async fn stop_all(&mut self) -> Result<(), String> {
        for (_, mut client) in self.clients.drain() {
            let _ = client.stop().await;
        }
        Ok(())
    }

    /// Check if LSP is running for a language
    pub fn is_running(&self, language: &str) -> bool {
        self.clients
            .get(language)
            .map(|c| c.is_running())
            .unwrap_or(false)
    }

    /// Get list of running LSP servers
    pub fn running_servers(&self) -> Vec<String> {
        self.clients
            .iter()
            .filter(|(_, c)| c.is_running())
            .map(|(lang, _)| lang.clone())
            .collect()
    }

    /// Detect language from file path
    pub fn detect_language(path: &str) -> Option<&'static str> {
        let ext = std::path::Path::new(path)
            .extension()
            .and_then(|s| s.to_str())?;

        match ext {
            "rs" => Some("rust"),
            "py" => Some("python"),
            "ts" | "tsx" => Some("typescript"),
            "js" | "jsx" => Some("javascript"),
            "go" => Some("go"),
            "c" | "h" => Some("c"),
            "cpp" | "hpp" | "cc" | "cxx" => Some("cpp"),
            _ => None,
        }
    }

    /// Notify that a document was opened
    pub async fn did_open(&mut self, path: &str, content: &str) -> Result<(), String> {
        let lang = Self::detect_language(path).ok_or("Unknown language")?;
        if let Some(client) = self.clients.get_mut(lang) {
            client.did_open(path, content).await?;
        }
        Ok(())
    }

    /// Get completions at position
    pub async fn completions(
        &mut self,
        path: &str,
        line: u32,
        character: u32,
    ) -> Result<Vec<CompletionItem>, String> {
        let lang = Self::detect_language(path).ok_or("Unknown language")?;
        if let Some(client) = self.clients.get_mut(lang) {
            return client.completions(path, line, character).await;
        }
        Ok(vec![])
    }

    /// Get hover info at position
    pub async fn hover(
        &mut self,
        path: &str,
        line: u32,
        character: u32,
    ) -> Result<Option<Hover>, String> {
        let lang = Self::detect_language(path).ok_or("Unknown language")?;
        if let Some(client) = self.clients.get_mut(lang) {
            return client.hover(path, line, character).await;
        }
        Ok(None)
    }

    /// Get diagnostics for a file (from cached notifications)
    pub async fn diagnostics(&self, path: &str) -> Result<Vec<lsp_types::Diagnostic>, String> {
        let lang = Self::detect_language(path).ok_or("Unknown language")?;
        if let Some(client) = self.clients.get(lang) {
            return Ok(client.get_diagnostics(path).await);
        }
        Ok(vec![])
    }
}
