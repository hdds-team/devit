// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2025-2026 naskel.com

//! Editor IPC commands

use crate::state::{AppState, OpenFile};
use parking_lot::RwLock;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::State;

#[derive(serde::Serialize)]
pub struct FileContent {
    pub path: String,
    pub content: String,
    pub language: Option<String>,
}

/// Open a file and return its content
#[tauri::command]
pub async fn open_file(
    path: String,
    state: State<'_, Arc<RwLock<AppState>>>,
) -> Result<FileContent, String> {
    let path_buf = PathBuf::from(&path);

    // Check cache first
    {
        let st = state.read();
        if let Some(file) = st.open_files.get(&path_buf) {
            return Ok(FileContent {
                path: path.clone(),
                content: file.content.clone(),
                language: file.language.clone(),
            });
        }
    }

    // Read from disk
    let content = tokio::fs::read_to_string(&path_buf)
        .await
        .map_err(|e| format!("Failed to read file: {}", e))?;

    let language = detect_language(&path_buf);

    // Cache it
    {
        let mut st = state.write();
        st.open_files.insert(
            path_buf.clone(),
            OpenFile {
                path: path_buf,
                content: content.clone(),
                modified: false,
                language: language.clone(),
            },
        );
    }

    Ok(FileContent {
        path,
        content,
        language,
    })
}

/// Reload file from disk (bypass cache)
#[tauri::command]
pub async fn reload_file(
    path: String,
    state: State<'_, Arc<RwLock<AppState>>>,
) -> Result<FileContent, String> {
    let path_buf = PathBuf::from(&path);

    // Always read from disk
    let content = tokio::fs::read_to_string(&path_buf)
        .await
        .map_err(|e| format!("Failed to read file: {}", e))?;

    let language = detect_language(&path_buf);

    // Update cache
    {
        let mut st = state.write();
        st.open_files.insert(
            path_buf.clone(),
            OpenFile {
                path: path_buf,
                content: content.clone(),
                modified: false,
                language: language.clone(),
            },
        );
    }

    Ok(FileContent {
        path,
        content,
        language,
    })
}

/// Save file content to disk
#[tauri::command]
pub async fn save_file(
    path: String,
    content: String,
    state: State<'_, Arc<RwLock<AppState>>>,
) -> Result<(), String> {
    let path_buf = PathBuf::from(&path);

    tokio::fs::write(&path_buf, &content)
        .await
        .map_err(|e| format!("Failed to write file: {}", e))?;

    // Update cache
    {
        let mut st = state.write();
        if let Some(file) = st.open_files.get_mut(&path_buf) {
            file.content = content;
            file.modified = false;
        }
    }

    Ok(())
}

/// Get symbols (functions, classes, etc.) from file content
/// Uses regex-based extraction as fallback when LSP is not available
#[tauri::command]
pub async fn get_symbols(
    path: String,
    state: State<'_, Arc<RwLock<AppState>>>,
) -> Result<Vec<Symbol>, String> {
    // Get content from cache (quick check, no await while holding lock)
    let cached_content = {
        let st = state.read();
        st.open_files
            .get(&PathBuf::from(&path))
            .map(|f| f.content.clone())
    };

    // Use cached content or read from disk
    let content = match cached_content {
        Some(c) => c,
        None => tokio::fs::read_to_string(&path)
            .await
            .map_err(|e| format!("Failed to read file: {}", e))?,
    };

    let language = detect_language(&PathBuf::from(&path));
    Ok(extract_symbols(&content, language.as_deref()))
}

/// Extract symbols from source code using regex patterns
fn extract_symbols(content: &str, language: Option<&str>) -> Vec<Symbol> {
    let mut symbols = Vec::new();

    match language {
        Some("rust") => {
            // Functions: fn name(
            for (i, line) in content.lines().enumerate() {
                let trimmed = line.trim();
                if let Some(rest) = trimmed.strip_prefix("fn ") {
                    if let Some(name) = rest.split('(').next() {
                        let name = name.split('<').next().unwrap_or(name).trim();
                        if !name.is_empty() {
                            symbols.push(Symbol {
                                name: name.to_string(),
                                kind: "function".to_string(),
                                line: u32::try_from(i + 1).unwrap_or(u32::MAX),
                            });
                        }
                    }
                }
                // Structs: struct Name
                else if let Some(rest) = trimmed.strip_prefix("struct ") {
                    if let Some(name) = rest
                        .split(|c| c == ' ' || c == '{' || c == '(' || c == '<')
                        .next()
                    {
                        symbols.push(Symbol {
                            name: name.to_string(),
                            kind: "struct".to_string(),
                            line: u32::try_from(i + 1).unwrap_or(u32::MAX),
                        });
                    }
                }
                // Enums: enum Name
                else if let Some(rest) = trimmed.strip_prefix("enum ") {
                    if let Some(name) = rest.split(|c| c == ' ' || c == '{' || c == '<').next() {
                        symbols.push(Symbol {
                            name: name.to_string(),
                            kind: "enum".to_string(),
                            line: u32::try_from(i + 1).unwrap_or(u32::MAX),
                        });
                    }
                }
                // Traits: trait Name
                else if let Some(rest) = trimmed.strip_prefix("trait ") {
                    if let Some(name) = rest.split(|c| c == ' ' || c == '{' || c == '<').next() {
                        symbols.push(Symbol {
                            name: name.to_string(),
                            kind: "trait".to_string(),
                            line: u32::try_from(i + 1).unwrap_or(u32::MAX),
                        });
                    }
                }
                // Impl blocks: impl Name or impl Trait for Name
                else if let Some(rest) = trimmed.strip_prefix("impl") {
                    if rest.starts_with(' ') || rest.starts_with('<') {
                        let rest = rest.trim_start_matches(|c| c == ' ' || c == '<');
                        if let Some(name) = rest
                            .split(|c| c == ' ' || c == '{' || c == '>' || c == '<')
                            .next()
                        {
                            if !name.is_empty() && name != "for" {
                                symbols.push(Symbol {
                                    name: format!("impl {}", name),
                                    kind: "impl".to_string(),
                                    line: u32::try_from(i + 1).unwrap_or(u32::MAX),
                                });
                            }
                        }
                    }
                }
            }
        }
        Some("python") => {
            for (i, line) in content.lines().enumerate() {
                let trimmed = line.trim();
                // def function_name(
                if let Some(rest) = trimmed.strip_prefix("def ") {
                    if let Some(name) = rest.split('(').next() {
                        symbols.push(Symbol {
                            name: name.to_string(),
                            kind: "function".to_string(),
                            line: u32::try_from(i + 1).unwrap_or(u32::MAX),
                        });
                    }
                }
                // class ClassName
                else if let Some(rest) = trimmed.strip_prefix("class ") {
                    if let Some(name) = rest.split(|c| c == '(' || c == ':').next() {
                        symbols.push(Symbol {
                            name: name.to_string(),
                            kind: "class".to_string(),
                            line: u32::try_from(i + 1).unwrap_or(u32::MAX),
                        });
                    }
                }
            }
        }
        Some("javascript")
        | Some("typescript")
        | Some("javascriptreact")
        | Some("typescriptreact") => {
            for (i, line) in content.lines().enumerate() {
                let trimmed = line.trim();
                // function name(
                if let Some(rest) = trimmed.strip_prefix("function ") {
                    if let Some(name) = rest.split('(').next() {
                        let name = name.split('<').next().unwrap_or(name).trim();
                        if !name.is_empty() {
                            symbols.push(Symbol {
                                name: name.to_string(),
                                kind: "function".to_string(),
                                line: u32::try_from(i + 1).unwrap_or(u32::MAX),
                            });
                        }
                    }
                }
                // class Name
                else if let Some(rest) = trimmed.strip_prefix("class ") {
                    if let Some(name) = rest.split(|c| c == ' ' || c == '{' || c == '<').next() {
                        symbols.push(Symbol {
                            name: name.to_string(),
                            kind: "class".to_string(),
                            line: u32::try_from(i + 1).unwrap_or(u32::MAX),
                        });
                    }
                }
                // const/let/var name = ... =>  (arrow function)
                // interface Name
                else if let Some(rest) = trimmed.strip_prefix("interface ") {
                    if let Some(name) = rest.split(|c| c == ' ' || c == '{' || c == '<').next() {
                        symbols.push(Symbol {
                            name: name.to_string(),
                            kind: "interface".to_string(),
                            line: u32::try_from(i + 1).unwrap_or(u32::MAX),
                        });
                    }
                }
                // type Name
                else if let Some(rest) = trimmed.strip_prefix("type ") {
                    if let Some(name) = rest.split(|c| c == ' ' || c == '=' || c == '<').next() {
                        symbols.push(Symbol {
                            name: name.to_string(),
                            kind: "type".to_string(),
                            line: u32::try_from(i + 1).unwrap_or(u32::MAX),
                        });
                    }
                }
            }
        }
        Some("c") | Some("cpp") => {
            // C/C++ symbol extraction
            let control_keywords = ["if", "for", "while", "switch", "catch", "else"];

            for (i, line) in content.lines().enumerate() {
                let trimmed = line.trim();

                // Skip preprocessor directives and comments
                if trimmed.starts_with('#')
                    || trimmed.starts_with("//")
                    || trimmed.starts_with("/*")
                {
                    continue;
                }

                // struct Name or struct Name {
                if let Some(rest) = trimmed.strip_prefix("struct ") {
                    if let Some(name) = rest
                        .split(|c: char| c.is_whitespace() || c == '{' || c == ':' || c == ';')
                        .next()
                    {
                        if !name.is_empty()
                            && name
                                .chars()
                                .next()
                                .map(|c| c.is_alphabetic())
                                .unwrap_or(false)
                        {
                            symbols.push(Symbol {
                                name: name.to_string(),
                                kind: "struct".to_string(),
                                line: u32::try_from(i + 1).unwrap_or(u32::MAX),
                            });
                        }
                    }
                    continue;
                }

                // class Name (C++)
                if let Some(rest) = trimmed.strip_prefix("class ") {
                    if let Some(name) = rest
                        .split(|c: char| c.is_whitespace() || c == '{' || c == ':' || c == ';')
                        .next()
                    {
                        if !name.is_empty()
                            && name
                                .chars()
                                .next()
                                .map(|c| c.is_alphabetic())
                                .unwrap_or(false)
                        {
                            symbols.push(Symbol {
                                name: name.to_string(),
                                kind: "class".to_string(),
                                line: u32::try_from(i + 1).unwrap_or(u32::MAX),
                            });
                        }
                    }
                    continue;
                }

                // enum Name
                if let Some(rest) = trimmed.strip_prefix("enum ") {
                    // Skip "enum class" for C++11
                    let rest = rest.strip_prefix("class ").unwrap_or(rest);
                    if let Some(name) = rest
                        .split(|c: char| c.is_whitespace() || c == '{' || c == ':' || c == ';')
                        .next()
                    {
                        if !name.is_empty()
                            && name
                                .chars()
                                .next()
                                .map(|c| c.is_alphabetic())
                                .unwrap_or(false)
                        {
                            symbols.push(Symbol {
                                name: name.to_string(),
                                kind: "enum".to_string(),
                                line: u32::try_from(i + 1).unwrap_or(u32::MAX),
                            });
                        }
                    }
                    continue;
                }

                // namespace Name (C++)
                if let Some(rest) = trimmed.strip_prefix("namespace ") {
                    if let Some(name) = rest.split(|c: char| c.is_whitespace() || c == '{').next() {
                        if !name.is_empty() {
                            symbols.push(Symbol {
                                name: name.to_string(),
                                kind: "namespace".to_string(),
                                line: u32::try_from(i + 1).unwrap_or(u32::MAX),
                            });
                        }
                    }
                    continue;
                }

                // Function definitions: look for lines with () that end with { or )
                // Pattern: [return_type] name(params) [const] [override] {
                if trimmed.contains('(')
                    && (trimmed.ends_with('{')
                        || trimmed.ends_with(')')
                        || trimmed.ends_with(") const")
                        || trimmed.ends_with(") override")
                        || trimmed.ends_with(") const override")
                        || trimmed.ends_with(") final"))
                {
                    // Extract function name (word before opening paren)
                    if let Some(paren_pos) = trimmed.find('(') {
                        let before_paren = &trimmed[..paren_pos];
                        // Split by whitespace and get last token (function name)
                        // Also handle Class::method
                        if let Some(name_part) = before_paren.split_whitespace().last() {
                            // Could be Class::method or just method
                            let name = if name_part.contains("::") {
                                name_part.to_string()
                            } else {
                                name_part.to_string()
                            };

                            // Skip control flow keywords
                            let base_name = name.split("::").last().unwrap_or(&name);
                            if !control_keywords.contains(&base_name)
                                && !name.is_empty()
                                && name
                                    .chars()
                                    .next()
                                    .map(|c| c.is_alphabetic() || c == '_' || c == '~')
                                    .unwrap_or(false)
                            {
                                symbols.push(Symbol {
                                    name,
                                    kind: "function".to_string(),
                                    line: u32::try_from(i + 1).unwrap_or(u32::MAX),
                                });
                            }
                        }
                    }
                }
            }
        }
        _ => {
            // Generic fallback: look for function-like patterns
            for (i, line) in content.lines().enumerate() {
                let trimmed = line.trim();
                if let Some(rest) = trimmed.strip_prefix("function ") {
                    if let Some(name) = rest.split('(').next() {
                        symbols.push(Symbol {
                            name: name.to_string(),
                            kind: "function".to_string(),
                            line: u32::try_from(i + 1).unwrap_or(u32::MAX),
                        });
                    }
                }
            }
        }
    }

    symbols
}

#[derive(serde::Serialize)]
pub struct Symbol {
    pub name: String,
    pub kind: String,
    pub line: u32,
}

/// Read a file as base64 (for attachments)
#[tauri::command]
pub async fn read_file_base64(path: String) -> Result<String, String> {
    use base64::{engine::general_purpose::STANDARD, Engine};

    let data = tokio::fs::read(&path)
        .await
        .map_err(|e| format!("Failed to read file: {}", e))?;

    Ok(STANDARD.encode(&data))
}

/// Detect language from file extension
fn detect_language(path: &PathBuf) -> Option<String> {
    let ext = path.extension()?.to_str()?;
    match ext {
        "rs" => Some("rust"),
        "py" => Some("python"),
        "js" => Some("javascript"),
        "ts" => Some("typescript"),
        "jsx" => Some("javascriptreact"),
        "tsx" => Some("typescriptreact"),
        "svelte" => Some("svelte"),
        "vue" => Some("vue"),
        "html" => Some("html"),
        "css" => Some("css"),
        "scss" => Some("scss"),
        "json" => Some("json"),
        "toml" => Some("toml"),
        "yaml" | "yml" => Some("yaml"),
        "md" => Some("markdown"),
        "c" => Some("c"),
        "cpp" | "cc" | "cxx" => Some("cpp"),
        "h" | "hpp" => Some("cpp"),
        "go" => Some("go"),
        "java" => Some("java"),
        "rb" => Some("ruby"),
        "php" => Some("php"),
        "sh" | "bash" => Some("shellscript"),
        "sql" => Some("sql"),
        _ => None,
    }
    .map(String::from)
}
