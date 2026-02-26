// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2025-2026 naskel.com

//! Claude CLI backend - uses `claude -p --output-format stream-json` for
//! zero-cost API access via Claude Max subscription.
//!
//! Spawns a `claude` process per request with plain text on stdin and
//! structured JSON streaming on stdout.
//!
//! NOTE: `--input-format stream-json` (persistent process) is broken in
//! Claude CLI v2.1.x (`--verbose` is required for stream-json output but
//! `--verbose` + `--input-format stream-json` hangs). When Anthropic fixes
//! this, we can switch to the persistent process model (cf. aIRCp's
//! `claude_stream_agent.py`).

use anyhow::{Context, Result};
use async_trait::async_trait;
use devit_backend_core::{
    ChatMessage, ChatRequest, FinishReason, LlmBackend, ModelInfo, RawChatResponse, Usage,
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;

const INIT_TIMEOUT_SECS: u64 = 30;
const RESPONSE_TIMEOUT_SECS: u64 = 300;

/// Streaming chunk types (same shape as other backends)
#[derive(Debug, Clone)]
pub enum StreamChunk {
    /// Text delta (partial content)
    Delta(String),
    /// Thinking/reasoning content
    Thinking(String),
    /// Final response with metadata
    Done(RawChatResponse),
    /// Error
    Error(String),
}

/// Claude CLI backend -- per-request `claude -p` with stream-json output.
/// Uses Max subscription token via the `claude` binary (zero API cost).
pub struct ClaudeBackend {
    model: String,
}

impl ClaudeBackend {
    pub fn new(model: String) -> Self {
        Self { model }
    }

    /// Stream chat response from Claude CLI
    pub async fn chat_stream(
        &self,
        request: ChatRequest,
    ) -> Result<mpsc::Receiver<StreamChunk>> {
        let system_prompt = request
            .messages
            .iter()
            .find(|m| m.role == "system")
            .map(|m| m.content.clone());

        let user_content = format_messages_for_claude(&request.messages);

        if user_content.is_empty() {
            anyhow::bail!("No user content to send to Claude");
        }

        let (tx, rx) = mpsc::channel(100);
        let model = self.model.clone();

        tokio::spawn(async move {
            let result =
                run_request(&model, system_prompt.as_deref(), &user_content, &tx).await;
            if let Err(e) = result {
                let _ = tx.send(StreamChunk::Error(e.to_string())).await;
            }
        });

        Ok(rx)
    }
}

/// Spawn claude, send plain text on stdin, read stream-json on stdout.
async fn run_request(
    model: &str,
    system_prompt: Option<&str>,
    user_content: &str,
    tx: &mpsc::Sender<StreamChunk>,
) -> Result<()> {
    let mut cmd = Command::new("claude");
    cmd.args([
        "-p",
        "--verbose",
        "--output-format",
        "stream-json",
        "--model",
        model,
    ]);

    if let Some(sp) = system_prompt {
        cmd.args(["--system-prompt", sp]);
    }

    // Remove env vars that trigger nested Claude Code detection
    cmd.env_remove("CLAUDECODE")
        .env_remove("CLAUDE_CODE_SESSION")
        .env_remove("CLAUDE_CODE_ENTRYPOINT")
        .env_remove("CLAUDE_CODE_RUNNING")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());

    let mut child = cmd
        .spawn()
        .context("Failed to spawn `claude` CLI. Is it installed?")?;

    let mut stdin = child.stdin.take().context("No stdin")?;
    let stdout = child.stdout.take().context("No stdout")?;
    let mut reader = BufReader::new(stdout);

    // In plain text mode, Claude CLI waits for stdin EOF before producing
    // any output (including the init message). So we must send the prompt
    // and close stdin FIRST, then read init + response from stdout.
    stdin.write_all(user_content.as_bytes()).await?;
    stdin.flush().await?;
    drop(stdin);

    // Now wait for init message
    let init_result = tokio::time::timeout(
        std::time::Duration::from_secs(INIT_TIMEOUT_SECS),
        wait_for_init(&mut reader),
    )
    .await;

    match init_result {
        Ok(Ok(())) => {}
        Ok(Err(e)) => {
            let _ = child.kill().await;
            anyhow::bail!("Claude init failed: {}", e);
        }
        Err(_) => {
            let _ = child.kill().await;
            anyhow::bail!("Claude init timeout ({}s)", INIT_TIMEOUT_SECS);
        }
    }

    // Read response events
    let mut accumulated_content = String::new();
    let mut line_buf = String::new();
    let deadline =
        tokio::time::Instant::now() + std::time::Duration::from_secs(RESPONSE_TIMEOUT_SECS);

    loop {
        line_buf.clear();

        let read_result =
            tokio::time::timeout_at(deadline, reader.read_line(&mut line_buf)).await;

        let n = match read_result {
            Ok(Ok(n)) => n,
            Ok(Err(e)) => {
                anyhow::bail!("Read error from Claude stdout: {}", e);
            }
            Err(_) => {
                anyhow::bail!("Claude response timeout ({}s)", RESPONSE_TIMEOUT_SECS);
            }
        };

        if n == 0 {
            break;
        }

        let trimmed = line_buf.trim();
        if trimmed.is_empty() {
            continue;
        }

        let event: serde_json::Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let event_type = event.get("type").and_then(|t| t.as_str()).unwrap_or("");

        match event_type {
            "content_block_delta" => {
                if let Some(delta) = event.get("delta") {
                    let delta_type =
                        delta.get("type").and_then(|t| t.as_str()).unwrap_or("");
                    match delta_type {
                        "text_delta" => {
                            if let Some(text) =
                                delta.get("text").and_then(|t| t.as_str())
                            {
                                accumulated_content.push_str(text);
                                let _ =
                                    tx.send(StreamChunk::Delta(text.to_string())).await;
                            }
                        }
                        "thinking_delta" => {
                            if let Some(text) =
                                delta.get("thinking").and_then(|t| t.as_str())
                            {
                                let _ = tx
                                    .send(StreamChunk::Thinking(text.to_string()))
                                    .await;
                            }
                        }
                        _ => {}
                    }
                }
            }
            "assistant" => {
                // Full assistant message -- extract text blocks
                if let Some(message) = event.get("message") {
                    if let Some(content) =
                        message.get("content").and_then(|c| c.as_array())
                    {
                        for block in content {
                            if block.get("type").and_then(|t| t.as_str()) == Some("text")
                            {
                                if let Some(text) =
                                    block.get("text").and_then(|t| t.as_str())
                                {
                                    if !accumulated_content.contains(text) {
                                        accumulated_content.push_str(text);
                                        let _ = tx
                                            .send(StreamChunk::Delta(text.to_string()))
                                            .await;
                                    }
                                }
                            }
                        }
                    }
                }
            }
            "result" => {
                let subtype =
                    event.get("subtype").and_then(|s| s.as_str()).unwrap_or("");

                if subtype == "error" {
                    let error_msg = event
                        .get("error")
                        .and_then(|e| e.as_str())
                        .or_else(|| event.get("result").and_then(|r| r.as_str()))
                        .unwrap_or("Unknown Claude error");
                    let _ =
                        tx.send(StreamChunk::Error(error_msg.to_string())).await;
                } else {
                    let usage = extract_usage(&event);
                    let finish_reason = if subtype == "success" {
                        FinishReason::Stop
                    } else {
                        FinishReason::Other
                    };

                    if accumulated_content.is_empty() {
                        if let Some(result_text) =
                            event.get("result").and_then(|r| r.as_str())
                        {
                            accumulated_content = result_text.to_string();
                        }
                    }

                    let mut response =
                        RawChatResponse::new(accumulated_content.clone())
                            .with_finish_reason(finish_reason);

                    if let Some(u) = usage {
                        response = response.with_usage(u);
                    }

                    let _ = tx.send(StreamChunk::Done(response)).await;
                }
                break;
            }
            "error" => {
                let msg = event
                    .get("error")
                    .or_else(|| event.get("message"))
                    .and_then(|e| e.as_str())
                    .unwrap_or("Unknown error from Claude CLI");
                let _ = tx.send(StreamChunk::Error(msg.to_string())).await;
                break;
            }
            // system, content_block_start, content_block_stop, rate_limit_event => ignored
            _ => {}
        }
    }

    // Process exits naturally after stdin EOF
    let _ = child.wait().await;

    Ok(())
}

/// Wait for the init message from Claude CLI stdout
async fn wait_for_init(
    reader: &mut BufReader<tokio::process::ChildStdout>,
) -> Result<()> {
    let mut line = String::new();
    loop {
        line.clear();
        let n = reader.read_line(&mut line).await?;
        if n == 0 {
            anyhow::bail!("Claude process exited before sending init");
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        if let Ok(data) = serde_json::from_str::<serde_json::Value>(trimmed) {
            if data.get("type").and_then(|t| t.as_str()) == Some("system")
                && data.get("subtype").and_then(|t| t.as_str()) == Some("init")
            {
                tracing::debug!(
                    "Claude session_id: {}",
                    data.get("session_id")
                        .and_then(|s| s.as_str())
                        .unwrap_or("unknown")
                );
                return Ok(());
            }
        }
    }
}

/// Extract usage stats from a result event
fn extract_usage(event: &serde_json::Value) -> Option<Usage> {
    let usage = event.get("usage")?;
    let input = usage
        .get("input_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let output = usage
        .get("output_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    let input_u32 = u32::try_from(input).unwrap_or(u32::MAX);
    let output_u32 = u32::try_from(output).unwrap_or(u32::MAX);

    Some(Usage {
        prompt_tokens: input_u32,
        completion_tokens: output_u32,
        total_tokens: input_u32.saturating_add(output_u32),
    })
}

/// Format Studio messages into a single user content string for Claude CLI.
///
/// Claude CLI expects one user message at a time. We flatten the conversation
/// history into a readable format with role markers.
fn format_messages_for_claude(messages: &[ChatMessage]) -> String {
    let mut parts: Vec<String> = Vec::new();

    for msg in messages {
        match msg.role.as_str() {
            "system" => continue, // Handled via --system-prompt flag
            "user" => parts.push(msg.content.clone()),
            "assistant" => {
                parts.push(format!("[Assistant]: {}", msg.content));
            }
            "tool" => {
                let tool_name = msg.tool_name.as_deref().unwrap_or("tool");
                parts.push(format!("[Tool Result ({})]: {}", tool_name, msg.content));
            }
            other => {
                parts.push(format!("[{}]: {}", other, msg.content));
            }
        }
    }

    parts.join("\n\n")
}

#[async_trait]
impl LlmBackend for ClaudeBackend {
    async fn chat(&self, request: ChatRequest) -> Result<RawChatResponse> {
        let rx = self.chat_stream(request).await?;
        collect_stream(rx).await
    }

    async fn get_model_info(&self) -> Result<ModelInfo> {
        Ok(ModelInfo {
            name: self.model.clone(),
            family: Some("claude".to_string()),
            context_window: Some(200_000),
            supports_native_tools: false, // Studio handles tools via XML in system prompt
            max_output_tokens: Some(64_000),
        })
    }
}

/// Collect a streaming response into a single RawChatResponse
async fn collect_stream(mut rx: mpsc::Receiver<StreamChunk>) -> Result<RawChatResponse> {
    let mut content = String::new();
    let mut final_response = None;

    while let Some(chunk) = rx.recv().await {
        match chunk {
            StreamChunk::Delta(text) => content.push_str(&text),
            StreamChunk::Thinking(_) => {} // Discard thinking in non-streaming mode
            StreamChunk::Done(resp) => {
                final_response = Some(resp);
                break;
            }
            StreamChunk::Error(e) => anyhow::bail!("Claude error: {}", e),
        }
    }

    Ok(final_response.unwrap_or_else(|| RawChatResponse::new(content)))
}

/// Check if `claude` CLI binary is available in PATH
pub fn is_available() -> bool {
    which::which("claude").is_ok()
}

/// Check if `claude` CLI binary is available (async wrapper for consistency)
pub async fn probe() -> bool {
    is_available()
}

/// List available Claude models (static list -- no API call needed)
pub fn available_models() -> Vec<ClaudeModel> {
    vec![
        ClaudeModel {
            id: "sonnet".to_string(),
            name: "Claude Sonnet 4".to_string(),
            context: 200_000,
        },
        ClaudeModel {
            id: "opus".to_string(),
            name: "Claude Opus 4".to_string(),
            context: 200_000,
        },
        ClaudeModel {
            id: "haiku".to_string(),
            name: "Claude Haiku 3.5".to_string(),
            context: 200_000,
        },
    ]
}

/// Claude model descriptor
pub struct ClaudeModel {
    pub id: String,
    pub name: String,
    pub context: u32,
}
