// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2025-2026 naskel.com

//! LSP client - JSON-RPC communication with language servers

use lsp_types::{
    ClientCapabilities, CompletionItem, CompletionParams, CompletionResponse,
    DidOpenTextDocumentParams, Hover, HoverParams, InitializeParams, InitializeResult,
    InitializedParams, Position, PublishDiagnosticsParams, TextDocumentIdentifier,
    TextDocumentItem, TextDocumentPositionParams, Url,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader as AsyncBufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::{mpsc, oneshot, Mutex};

/// LSP server configuration
#[derive(Debug, Clone)]
pub struct LspServerConfig {
    pub language_id: String,
    pub server_command: String,
    pub server_args: Vec<String>,
}

/// JSON-RPC request
#[derive(Debug, Serialize)]
struct JsonRpcRequest<T: Serialize> {
    jsonrpc: &'static str,
    id: u64,
    method: &'static str,
    params: T,
}

/// JSON-RPC notification (no id, no response expected)
#[derive(Debug, Serialize)]
struct JsonRpcNotification<T: Serialize> {
    jsonrpc: &'static str,
    method: &'static str,
    params: T,
}

/// JSON-RPC response
#[derive(Debug, Deserialize)]
struct JsonRpcResponse<T> {
    #[allow(dead_code)]
    jsonrpc: String,
    id: Option<u64>,
    #[serde(default)]
    result: Option<T>,
    #[serde(default)]
    error: Option<JsonRpcError>,
}

/// JSON-RPC message (response or notification)
#[derive(Debug, Deserialize)]
struct JsonRpcMessage {
    #[allow(dead_code)]
    jsonrpc: String,
    #[serde(default)]
    id: Option<u64>,
    #[serde(default)]
    method: Option<String>,
    #[serde(default)]
    params: Option<Value>,
    #[serde(default)]
    result: Option<Value>,
    #[serde(default)]
    error: Option<JsonRpcError>,
}

#[derive(Debug, Deserialize)]
struct JsonRpcError {
    code: i32,
    message: String,
    #[serde(default)]
    data: Option<Value>,
}

/// Pending request channel
type ResponseSender = oneshot::Sender<Result<Value, String>>;

/// Shared diagnostics storage (file URI -> diagnostics)
pub type DiagnosticsStore = Arc<Mutex<HashMap<String, Vec<lsp_types::Diagnostic>>>>;

/// Active LSP client with JSON-RPC support
pub struct LspClient {
    pub config: LspServerConfig,
    pub workspace_root: PathBuf,
    process: Option<Child>,
    stdin: Option<ChildStdin>,
    request_id: AtomicU64,
    pending_requests: Arc<Mutex<HashMap<u64, ResponseSender>>>,
    diagnostics: DiagnosticsStore,
    initialized: bool,
}

impl LspClient {
    pub fn new(config: LspServerConfig, workspace_root: PathBuf) -> Self {
        Self {
            config,
            process: None,
            stdin: None,
            workspace_root,
            request_id: AtomicU64::new(1),
            pending_requests: Arc::new(Mutex::new(HashMap::new())),
            diagnostics: Arc::new(Mutex::new(HashMap::new())),
            initialized: false,
        }
    }

    /// Start the LSP server and initialize
    pub async fn start(&mut self) -> Result<(), String> {
        let mut child = Command::new(&self.config.server_command)
            .args(&self.config.server_args)
            .current_dir(&self.workspace_root)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| format!("Failed to start LSP server: {}", e))?;

        let stdin = child.stdin.take().ok_or("Failed to get stdin")?;
        let stdout = child.stdout.take().ok_or("Failed to get stdout")?;

        self.stdin = Some(stdin);
        self.process = Some(child);

        // Spawn response reader
        let pending = Arc::clone(&self.pending_requests);
        let diagnostics = Arc::clone(&self.diagnostics);
        tokio::spawn(async move {
            if let Err(e) = Self::read_responses(stdout, pending, diagnostics).await {
                tracing::error!("LSP response reader error: {}", e);
            }
        });

        // Send initialize request
        self.initialize().await?;
        self.initialized = true;

        Ok(())
    }

    /// Read and dispatch LSP responses and notifications
    async fn read_responses(
        stdout: ChildStdout,
        pending: Arc<Mutex<HashMap<u64, ResponseSender>>>,
        diagnostics: DiagnosticsStore,
    ) -> Result<(), String> {
        let mut reader = AsyncBufReader::new(stdout);

        loop {
            // Read headers until empty line
            let mut content_length: Option<usize> = None;
            loop {
                let mut line = String::new();
                let n = reader
                    .read_line(&mut line)
                    .await
                    .map_err(|e| e.to_string())?;
                if n == 0 {
                    return Ok(()); // EOF
                }

                let line = line.trim();
                if line.is_empty() {
                    break;
                }

                if let Some(len_str) = line.strip_prefix("Content-Length: ") {
                    content_length = Some(len_str.parse().map_err(|e| format!("{}", e))?);
                }
            }

            let content_length = content_length.ok_or("Missing Content-Length header")?;

            // Read JSON body
            let mut body = vec![0u8; content_length];
            reader
                .read_exact(&mut body)
                .await
                .map_err(|e| e.to_string())?;

            let message: JsonRpcMessage =
                serde_json::from_slice(&body).map_err(|e| e.to_string())?;

            // Check if this is a notification (has method, no id)
            if message.id.is_none() {
                if let Some(method) = &message.method {
                    if method == "textDocument/publishDiagnostics" {
                        if let Some(params) = message.params {
                            if let Ok(diag_params) =
                                serde_json::from_value::<PublishDiagnosticsParams>(params)
                            {
                                let uri = diag_params.uri.to_string();
                                let mut store = diagnostics.lock().await;
                                store.insert(uri, diag_params.diagnostics);
                            }
                        }
                    }
                }
                continue;
            }

            // Dispatch response to waiting request
            if let Some(id) = message.id {
                let mut pending = pending.lock().await;
                if let Some(tx) = pending.remove(&id) {
                    let result = if let Some(err) = message.error {
                        Err(format!("LSP error {}: {}", err.code, err.message))
                    } else {
                        Ok(message.result.unwrap_or(Value::Null))
                    };
                    let _ = tx.send(result);
                }
            }
        }
    }

    /// Send a JSON-RPC request and wait for response
    async fn request<P: Serialize, R: DeserializeOwned>(
        &mut self,
        method: &'static str,
        params: P,
    ) -> Result<R, String> {
        let stdin = self.stdin.as_mut().ok_or("LSP not started")?;

        let id = self.request_id.fetch_add(1, Ordering::SeqCst);
        let request = JsonRpcRequest {
            jsonrpc: "2.0",
            id,
            method,
            params,
        };

        // Create response channel
        let (tx, rx) = oneshot::channel();
        {
            let mut pending = self.pending_requests.lock().await;
            pending.insert(id, tx);
        }

        // Serialize and send
        let body = serde_json::to_string(&request).map_err(|e| e.to_string())?;
        let message = format!("Content-Length: {}\r\n\r\n{}", body.len(), body);
        stdin
            .write_all(message.as_bytes())
            .await
            .map_err(|e| e.to_string())?;
        stdin.flush().await.map_err(|e| e.to_string())?;

        // Wait for response
        let result = rx.await.map_err(|_| "Request cancelled")?;
        let value = result?;
        serde_json::from_value(value).map_err(|e| e.to_string())
    }

    /// Send a JSON-RPC notification (no response)
    async fn notify<P: Serialize>(
        &mut self,
        method: &'static str,
        params: P,
    ) -> Result<(), String> {
        let stdin = self.stdin.as_mut().ok_or("LSP not started")?;

        let notification = JsonRpcNotification {
            jsonrpc: "2.0",
            method,
            params,
        };

        let body = serde_json::to_string(&notification).map_err(|e| e.to_string())?;
        let message = format!("Content-Length: {}\r\n\r\n{}", body.len(), body);
        stdin
            .write_all(message.as_bytes())
            .await
            .map_err(|e| e.to_string())?;
        stdin.flush().await.map_err(|e| e.to_string())?;

        Ok(())
    }

    /// Initialize the LSP server
    async fn initialize(&mut self) -> Result<InitializeResult, String> {
        let root_uri =
            Url::from_file_path(&self.workspace_root).map_err(|_| "Invalid workspace path")?;

        #[allow(deprecated)]
        let params = InitializeParams {
            process_id: Some(std::process::id()),
            root_uri: Some(root_uri.clone()),
            capabilities: ClientCapabilities::default(),
            initialization_options: None,
            root_path: Some(self.workspace_root.to_string_lossy().to_string()),
            trace: None,
            workspace_folders: None,
            client_info: Some(lsp_types::ClientInfo {
                name: "devit-studio".into(),
                version: Some("0.1.0".into()),
            }),
            locale: None,
            work_done_progress_params: Default::default(),
        };

        let result: InitializeResult = self.request("initialize", params).await?;

        // Send initialized notification
        self.notify("initialized", InitializedParams {}).await?;

        Ok(result)
    }

    /// Notify server that a document was opened
    pub async fn did_open(&mut self, path: &str, content: &str) -> Result<(), String> {
        let uri = Url::from_file_path(path).map_err(|_| "Invalid file path")?;

        let params = DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri,
                language_id: self.config.language_id.clone(),
                version: 1,
                text: content.to_string(),
            },
        };

        self.notify("textDocument/didOpen", params).await
    }

    /// Get completions at position
    pub async fn completions(
        &mut self,
        path: &str,
        line: u32,
        character: u32,
    ) -> Result<Vec<CompletionItem>, String> {
        let uri = Url::from_file_path(path).map_err(|_| "Invalid file path")?;

        let params = CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: Position { line, character },
            },
            context: None,
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };

        let response: Option<CompletionResponse> =
            self.request("textDocument/completion", params).await?;

        Ok(match response {
            Some(CompletionResponse::Array(items)) => items,
            Some(CompletionResponse::List(list)) => list.items,
            None => vec![],
        })
    }

    /// Get hover info at position
    pub async fn hover(
        &mut self,
        path: &str,
        line: u32,
        character: u32,
    ) -> Result<Option<Hover>, String> {
        let uri = Url::from_file_path(path).map_err(|_| "Invalid file path")?;

        let params = HoverParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: Position { line, character },
            },
            work_done_progress_params: Default::default(),
        };

        self.request("textDocument/hover", params).await
    }

    /// Stop the LSP server
    pub async fn stop(&mut self) -> Result<(), String> {
        // Send shutdown request
        if self.initialized {
            let _: Value = self.request("shutdown", Value::Null).await?;
            self.notify("exit", Value::Null).await?;
        }

        // Kill process if still running
        if let Some(mut process) = self.process.take() {
            let _ = process.kill().await;
        }

        self.stdin = None;
        self.initialized = false;

        Ok(())
    }

    pub fn is_running(&self) -> bool {
        self.process.is_some() && self.initialized
    }

    /// Get diagnostics for a file (from cached notifications)
    pub async fn get_diagnostics(&self, path: &str) -> Vec<lsp_types::Diagnostic> {
        let uri = match Url::from_file_path(path) {
            Ok(u) => u.to_string(),
            Err(_) => return vec![],
        };
        let store = self.diagnostics.lock().await;
        store.get(&uri).cloned().unwrap_or_default()
    }
}

/// Known LSP server configurations
pub fn get_server_config(language: &str) -> Option<LspServerConfig> {
    match language {
        "rust" => Some(LspServerConfig {
            language_id: "rust".into(),
            server_command: "rust-analyzer".into(),
            server_args: vec![],
        }),
        "python" => Some(LspServerConfig {
            language_id: "python".into(),
            server_command: "pylsp".into(),
            server_args: vec![],
        }),
        "typescript" | "javascript" => Some(LspServerConfig {
            language_id: language.into(),
            server_command: "typescript-language-server".into(),
            server_args: vec!["--stdio".into()],
        }),
        "go" => Some(LspServerConfig {
            language_id: "go".into(),
            server_command: "gopls".into(),
            server_args: vec![],
        }),
        "c" | "cpp" => Some(LspServerConfig {
            language_id: language.into(),
            server_command: "clangd".into(),
            server_args: vec![],
        }),
        _ => None,
    }
}
