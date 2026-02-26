// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2025-2026 naskel.com

//! LLM IPC commands - streaming chat with multiple providers

// ============================================================================
// XML Tool Call Filter - Extracted for testability
// ============================================================================

/// Result of filtering tool call XML from streaming content
#[derive(Debug, Clone, PartialEq)]
pub struct FilterResult {
    /// Text to display (outside of tool_call tags)
    pub display_text: String,
    /// Remaining buffer (partial tags)
    pub new_buffer: String,
    /// New XML depth (0 = outside, 1 = inside tool_call)
    pub new_depth: i32,
    /// True if we entered a tool_call during this filter
    pub entered_tool_call: bool,
}

/// Filter tool_call XML tags from streaming content.
/// Shows text outside of <tool_call>...</tool_call> blocks.
/// Handles partial tags at chunk boundaries.
pub fn filter_tool_call_xml(buffer: &str, xml_depth: i32) -> FilterResult {
    let mut display_text = String::new();
    let mut new_buffer = String::new();
    let mut depth = xml_depth;
    let mut pos = 0;
    let mut entered_tool_call = false;

    while pos < buffer.len() {
        let remaining = &buffer[pos..];

        // Check for <tool_call> start
        if depth == 0 && remaining.starts_with("<tool_call>") {
            depth = 1;
            entered_tool_call = true;
            pos += 11;
            continue;
        }

        // Check for </tool_call> end
        if depth > 0 && remaining.starts_with("</tool_call>") {
            depth = 0;
            pos += 12;
            continue;
        }

        // Check for partial tag at end (could be start of <tool_call> or </tool_call>)
        if remaining.starts_with("<") {
            // If it could be a tool tag but not complete, buffer it
            if remaining.len() < 12
                && (remaining.starts_with("<t") || remaining.starts_with("</") || remaining == "<")
            {
                new_buffer = remaining.to_string();
                break;
            }
        }

        // Get one UTF-8 char and advance
        if let Some(ch) = remaining.chars().next() {
            if depth == 0 {
                display_text.push(ch);
            }
            pos += ch.len_utf8();
        } else {
            break;
        }
    }

    FilterResult {
        display_text,
        new_buffer,
        new_depth: depth,
        entered_tool_call,
    }
}

/// Filter out narration patterns that fine-tuned models sometimes generate.
/// These patterns come from training data and shouldn't be shown to the user.
pub fn filter_narration_patterns(text: &str) -> String {
    // Patterns to filter out (learned during fine-tuning)
    let patterns = [
        "[Executed shell - see result below]",
        "[Executed file_read - see result below]",
        "[Executed file_write - see result below]",
        "[Executed file_list - see result below]",
        "[Executed project_structure - see result below]",
        "[Executed pwd - see result below]",
        "[Executed patch_apply - see result below]",
    ];

    let mut result = text.to_string();
    for pattern in patterns {
        result = result.replace(pattern, "");
    }

    // Also filter partial patterns like "[Executed " at chunk boundaries
    if result.starts_with("[Executed ") && !result.contains(']') {
        return String::new(); // Wait for more content
    }

    result
}

// ============================================================================
// Main module code
// ============================================================================

use crate::ipc::context_manager::{
    estimate_message_tokens, parse_summary_response, ContextStats, TopicSummary, SUMMARIZE_PROMPT,
};
use crate::state::{AppState, ToolState};
use devit_backend_core::tool_calling::{DevItFormatParser, ToolCall};
use devit_backend_core::ChatMessage as BackendMessage;
use devit_backend_core::{ChatRequest, LlmBackend};
use devit_claude::ClaudeBackend;
use devit_llama_cpp::LlamaCppBackend;
use devit_lmstudio::LmstudioBackend;
use devit_ollama::OllamaBackend;
use mcp_core::McpTool;
use parking_lot::RwLock;
use std::sync::Arc;
use tauri::{Emitter, State, Window};
use tokio::sync::watch;

/// Unified stream chunk type for all backends
#[derive(Debug, Clone)]
enum UnifiedStreamChunk {
    Delta(String),
    Thinking(String),
    Done(devit_backend_core::RawChatResponse),
    Error(String),
}

/// Backend abstraction for streaming
enum BackendStream {
    Ollama(tokio::sync::mpsc::Receiver<devit_ollama::StreamChunk>),
    Lmstudio(tokio::sync::mpsc::Receiver<devit_lmstudio::StreamChunk>),
    LlamaCpp(tokio::sync::mpsc::Receiver<devit_llama_cpp::StreamChunk>),
    Petals(tokio::sync::mpsc::Receiver<UnifiedStreamChunk>),
    Claude(tokio::sync::mpsc::Receiver<devit_claude::StreamChunk>),
}

impl BackendStream {
    async fn recv(&mut self) -> Option<UnifiedStreamChunk> {
        match self {
            BackendStream::Ollama(rx) => rx.recv().await.map(|chunk| match chunk {
                devit_ollama::StreamChunk::Delta(s) => UnifiedStreamChunk::Delta(s),
                devit_ollama::StreamChunk::Thinking(s) => UnifiedStreamChunk::Thinking(s),
                devit_ollama::StreamChunk::Done(r) => UnifiedStreamChunk::Done(r),
                devit_ollama::StreamChunk::Error(e) => UnifiedStreamChunk::Error(e),
            }),
            BackendStream::Lmstudio(rx) => rx.recv().await.map(|chunk| match chunk {
                devit_lmstudio::StreamChunk::Delta(s) => UnifiedStreamChunk::Delta(s),
                devit_lmstudio::StreamChunk::Thinking(s) => UnifiedStreamChunk::Thinking(s),
                devit_lmstudio::StreamChunk::Done(r) => UnifiedStreamChunk::Done(r),
                devit_lmstudio::StreamChunk::Error(e) => UnifiedStreamChunk::Error(e),
            }),
            BackendStream::LlamaCpp(rx) => rx.recv().await.map(|chunk| match chunk {
                devit_llama_cpp::StreamChunk::Delta(s) => UnifiedStreamChunk::Delta(s),
                devit_llama_cpp::StreamChunk::Thinking(s) => UnifiedStreamChunk::Thinking(s),
                devit_llama_cpp::StreamChunk::Done(r) => UnifiedStreamChunk::Done(r),
                devit_llama_cpp::StreamChunk::Error(e) => UnifiedStreamChunk::Error(e),
            }),
            BackendStream::Petals(rx) => rx.recv().await,
            BackendStream::Claude(rx) => rx.recv().await.map(|chunk| match chunk {
                devit_claude::StreamChunk::Delta(s) => UnifiedStreamChunk::Delta(s),
                devit_claude::StreamChunk::Thinking(s) => UnifiedStreamChunk::Thinking(s),
                devit_claude::StreamChunk::Done(r) => UnifiedStreamChunk::Done(r),
                devit_claude::StreamChunk::Error(e) => UnifiedStreamChunk::Error(e),
            }),
        }
    }
}

/// Maximum number of tool call iterations
const MAX_TOOL_ITERATIONS: usize = 10;

#[derive(serde::Deserialize, Clone)]
pub struct ChatMessage {
    pub role: String, // "user" | "assistant" | "tool_call" | "tool_result"
    pub content: String,
    #[serde(default)]
    pub attachments: Vec<Attachment>,
    /// Tool name (for tool_call and tool_result messages)
    #[serde(default, rename = "toolName")]
    pub tool_name: Option<String>,
}

#[derive(serde::Deserialize, Clone)]
pub struct Attachment {
    pub name: String,
    pub mime_type: String, // "image/png", "image/jpeg", "application/pdf"
    pub data: String,      // base64 encoded
}

impl Attachment {
    /// Check if this is an image attachment
    pub fn is_image(&self) -> bool {
        self.mime_type.starts_with("image/")
    }

    /// Check if this is a PDF attachment
    pub fn is_pdf(&self) -> bool {
        self.mime_type == "application/pdf"
    }
}

#[derive(serde::Serialize, Clone)]
pub struct Provider {
    pub id: String,
    pub name: String,
    pub kind: String, // "local" | "cloud"
    pub available: bool,
}

#[derive(serde::Serialize, Clone)]
pub struct StreamChunkPayload {
    pub delta: String,
    pub done: bool,
}

#[derive(serde::Serialize, Clone)]
pub struct ThinkingPayload {
    pub delta: String,
    pub done: bool,
}

/// Status update during streaming (e.g., "Detecting tool call...")
#[derive(serde::Serialize, Clone)]
pub struct StatusPayload {
    pub message: String,
    pub icon: String,
}

/// Performance stats after response completion
#[derive(serde::Serialize, Clone)]
pub struct StatsPayload {
    /// Tokens generated per second
    pub tokens_per_second: Option<f64>,
    /// Total tokens (prompt + completion)
    pub total_tokens: Option<u32>,
    /// Prompt tokens
    pub prompt_tokens: Option<u32>,
    /// Completion tokens
    pub completion_tokens: Option<u32>,
    /// Total time in milliseconds
    pub total_ms: Option<f64>,
}

/// Global cancellation token for active stream
static CANCEL_TX: std::sync::OnceLock<watch::Sender<bool>> = std::sync::OnceLock::new();

fn get_cancel_channel() -> &'static watch::Sender<bool> {
    CANCEL_TX.get_or_init(|| {
        let (tx, _rx) = watch::channel(false);
        tx
    })
}

/// Default system prompt - balanced between helpfulness and tool usage
pub const DEFAULT_SYSTEM_PROMPT: &str = r#"You are a helpful coding assistant with access to tools.

## MANDATORY TOOL CALL FORMAT

When you need to use a tool, you MUST output this EXACT XML format:

<tool_call>
<tool_name>TOOL_NAME</tool_name>
<arguments>
{"param": "value"}
</arguments>
</tool_call>

Example - Reading a file:
<tool_call>
<tool_name>file_read</tool_name>
<arguments>
{"path": "main.rs"}
</arguments>
</tool_call>

Example - Running a command:
<tool_call>
<tool_name>shell</tool_name>
<arguments>
{"command": "ls -la"}
</arguments>
</tool_call>

CRITICAL RULES:
1. You MUST output the <tool_call> XML block when you want to use a tool
2. Do NOT just say "I will read the file" - actually output the XML
3. Do NOT wrap tool calls in markdown code blocks
4. Output the XML directly in your response

## GUIDELINES

- For general questions or greetings, respond naturally without using tools
- Use tools when you need to access files, run commands, or get real information
- After receiving tool results, synthesize the information in your response
- After writing a file, confirm briefly without repeating content

## TOOL SELECTION

- file_read: Read file content
- file_write: Create or overwrite files
- shell: Execute commands
- patch_apply: Modify existing files with a diff"#;

/// Convert tool name to short alias (strip "devit_" prefix for LLM simplicity)
/// e.g., "devit_file_read" -> "file_read"
fn to_short_name(name: &str) -> &str {
    name.strip_prefix("devit_").unwrap_or(name)
}

/// Build system prompt with tool list appended (using short names)
fn build_system_prompt(base_prompt: &str, tools: &[Arc<dyn McpTool>]) -> String {
    let mut prompt = base_prompt.to_string();

    if !tools.is_empty() {
        prompt.push_str("\n\nAVAILABLE TOOLS:\n");
        for tool in tools {
            prompt.push_str(&format!(
                "- {}: {}\n",
                to_short_name(tool.name()),
                tool.description()
            ));
        }
    }

    prompt
}

/// Convert MCP tools to Ollama native tool format (using short names)
fn build_ollama_tools(tools: &[Arc<dyn McpTool>]) -> serde_json::Value {
    let ollama_tools: Vec<serde_json::Value> = tools
        .iter()
        .map(|tool| {
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": to_short_name(tool.name()),
                    "description": tool.description(),
                    "parameters": {
                        "type": "object",
                        "properties": tool.input_schema().get("properties").cloned().unwrap_or(serde_json::json!({})),
                        "required": tool.input_schema().get("required").cloned().unwrap_or(serde_json::json!([]))
                    }
                }
            })
        })
        .collect();

    serde_json::json!(ollama_tools)
}

/// Execute a single tool call (supports both short and full tool names)
async fn execute_tool(tool_call: &ToolCall, tools: &[Arc<dyn McpTool>]) -> Result<String, String> {
    // Find tool by short name OR full name (for backwards compatibility)
    let tool = tools
        .iter()
        .find(|t| to_short_name(t.name()) == tool_call.name || t.name() == tool_call.name)
        .ok_or_else(|| format!("Tool not found: {}", tool_call.name))?;

    // Normalize arguments: some LLMs return arguments as a JSON string instead of an object
    // e.g., {"path": "cube.rs"} vs "{\"path\": \"cube.rs\"}"
    let arguments = match &tool_call.arguments {
        serde_json::Value::String(s) => {
            // Try to parse the string as JSON
            serde_json::from_str(s).unwrap_or_else(|_| tool_call.arguments.clone())
        }
        _ => tool_call.arguments.clone(),
    };

    let result = tool.execute(arguments).await.map_err(|e| e.to_string())?;

    serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
}

/// Payload for tool call events
#[derive(serde::Serialize, Clone)]
pub struct ToolCallPayload {
    pub name: String,
    pub args: String,
}

/// Payload for tool result events
#[derive(serde::Serialize, Clone)]
pub struct ToolResultPayload {
    pub name: String,
    pub result: String,
}

/// Payload for context compression events
#[derive(serde::Serialize, Clone)]
pub struct CompressionPayload {
    pub topic_title: String,
    pub messages_compressed: usize,
    pub stats: ContextStats,
}

/// Default context size for compression (can be overridden per model)
const DEFAULT_MAX_CONTEXT: usize = 8192;
/// Compression threshold (50% of context - conservative because we don't count system prompt)
const COMPRESSION_THRESHOLD: f32 = 0.5;
/// Estimated tokens for system prompt + tools (rough overhead)
const SYSTEM_PROMPT_OVERHEAD: usize = 2500;

/// Type alias for managed tool state
type ManagedToolState = Arc<tokio::sync::RwLock<ToolState>>;

/// Start streaming chat response with tool calling support
#[tauri::command]
pub async fn stream_chat(
    messages: Vec<ChatMessage>,
    window: Window,
    state: State<'_, Arc<RwLock<AppState>>>,
    tool_state: State<'_, ManagedToolState>,
) -> Result<(), String> {
    let (provider_id, model_name, ollama_url, lmstudio_url, llamacpp_url) = {
        let st = state.read();
        let provider = st
            .current_provider
            .clone()
            .unwrap_or_else(|| "ollama".into());
        let model = st
            .current_model
            .clone()
            .unwrap_or_else(|| "qwen2.5-coder:7b".into());
        (
            provider,
            model,
            st.settings.ollama_url.clone(),
            st.settings.lmstudio_url.clone(),
            st.settings.llamacpp_url.clone(),
        )
    };

    tracing::debug!("Using provider: {}, model: {}", provider_id, model_name);

    // Get custom system prompt from settings (or use default)
    let base_prompt = {
        let st = state.read();
        st.settings
            .system_prompt
            .clone()
            .unwrap_or_else(|| DEFAULT_SYSTEM_PROMPT.to_string())
    };

    // Get tools from tool state
    let (tools, ollama_tools, system_prompt) = {
        let ts = tool_state.read().await;
        let tools = ts.tools.clone();
        let ollama_tools = build_ollama_tools(&tools);
        let system_prompt = build_system_prompt(&base_prompt, &tools);
        (tools, ollama_tools, system_prompt)
    };

    // Reset cancellation
    let _ = get_cancel_channel().send(false);

    // Convert messages to backend format, adding system prompt
    let mut backend_messages: Vec<BackendMessage> = vec![BackendMessage {
        role: "system".to_string(),
        content: system_prompt,
        tool_calls: None,
        tool_name: None,
        images: None,
    }];

    backend_messages.extend(messages.into_iter().filter_map(|msg| {
        // Convert UI-specific roles to LLM-compatible roles
        // LMStudio/OpenAI only accept: user, assistant, system, tool
        let role = match msg.role.as_str() {
            "tool_call" => return None, // Skip tool_call messages (UI-only, info already in assistant message)
            "tool_result" => "tool".to_string(), // Convert to standard "tool" role
            other => other.to_string(),
        };

        // Extract images from attachments (base64 only, no data: prefix)
        let images: Vec<String> = msg
            .attachments
            .iter()
            .filter(|a| a.is_image())
            .map(|a| a.data.clone())
            .collect();

        // For PDFs, append text description to content
        let mut content = msg.content.clone();
        for att in msg.attachments.iter().filter(|a| a.is_pdf()) {
            content.push_str(&format!("\n\n[Attached PDF: {}]", att.name));
        }

        Some(BackendMessage {
            role,
            content,
            tool_calls: None,
            tool_name: msg.tool_name,
            images: if images.is_empty() {
                None
            } else {
                Some(images)
            },
        })
    }));

    // Validate provider before spawning
    if !matches!(
        provider_id.as_str(),
        "ollama" | "lmstudio" | "llamacpp" | "petals" | "claude"
    ) {
        return Err(format!("Unknown provider: {}", provider_id));
    }

    // Clone state for use in spawned task (for cache invalidation)
    let state_clone = state.inner().clone();

    // Spawn tool loop task
    tokio::spawn(async move {
        let mut messages = backend_messages;
        let mut iterations = 0;
        let mut cancel_rx = get_cancel_channel().subscribe();

        // Create backends (we'll use them based on provider_id)
        let ollama_backend = OllamaBackend::new(ollama_url.clone(), model_name.clone());
        let lmstudio_backend = LmstudioBackend::new(
            format!("{}/v1", lmstudio_url.trim_end_matches('/')),
            model_name.clone(),
        );
        let llamacpp_backend = LlamaCppBackend::new(
            format!("{}/v1", llamacpp_url.trim_end_matches('/')),
            model_name.clone(),
        );
        let claude_backend = ClaudeBackend::new(model_name.clone());

        loop {
            iterations += 1;
            if iterations > MAX_TOOL_ITERATIONS {
                let _ = window.emit(
                    "llm:chunk",
                    StreamChunkPayload {
                        delta: format!("❌ Max tool iterations ({}) exceeded", MAX_TOOL_ITERATIONS),
                        done: true,
                    },
                );
                break;
            }

            // Check cancellation
            if *cancel_rx.borrow() {
                let _ = window.emit(
                    "llm:chunk",
                    StreamChunkPayload {
                        delta: String::new(),
                        done: true,
                    },
                );
                break;
            }

            // Build request with tools
            let request = ChatRequest::new(messages.clone()).with_tools(ollama_tools.clone());

            // Start streaming based on provider
            tracing::debug!("[STREAM] Starting request to provider: {}", provider_id);
            let stream_result: Result<BackendStream, String> = match provider_id.as_str() {
                "ollama" => ollama_backend
                    .chat_stream(request)
                    .await
                    .map(BackendStream::Ollama)
                    .map_err(|e| e.to_string()),
                "lmstudio" => lmstudio_backend
                    .chat_stream(request)
                    .await
                    .map(BackendStream::Lmstudio)
                    .map_err(|e| e.to_string()),
                "llamacpp" => {
                    tracing::debug!("[STREAM] Calling llamacpp_backend.chat_stream...");
                    let result = llamacpp_backend
                        .chat_stream(request)
                        .await
                        .map(BackendStream::LlamaCpp)
                        .map_err(|e| {
                            tracing::error!("[STREAM] llamacpp error: {}", e);
                            e.to_string()
                        });
                    tracing::debug!("[STREAM] llamacpp result: {:?}", result.is_ok());
                    result
                }
                "petals" => {
                    // Petals: call API and simulate streaming
                    petals_generate(&messages, &model_name).await
                }
                "claude" => claude_backend
                    .chat_stream(request)
                    .await
                    .map(BackendStream::Claude)
                    .map_err(|e| e.to_string()),
                _ => Err("Unknown provider".to_string()),
            };

            let mut rx = match stream_result {
                Ok(rx) => rx,
                Err(e) => {
                    let _ = window.emit(
                        "llm:chunk",
                        StreamChunkPayload {
                            delta: format!("❌ Error: {}", e),
                            done: true,
                        },
                    );
                    break;
                }
            };

            let mut accumulated_content = String::new();
            let mut final_tool_calls: Option<Vec<ToolCall>> = None;
            let mut in_thinking = false; // Track if we're currently receiving thinking

            // Stream response - filter out tool call XML from display
            // We use a simple state machine to detect and skip XML tool blocks
            let mut xml_depth: i32 = 0; // > 0 means we're inside tool XML
            let mut buffer = String::new();

            loop {
                tokio::select! {
                    _ = cancel_rx.changed() => {
                        if *cancel_rx.borrow() {
                            let _ = window.emit("llm:chunk", StreamChunkPayload {
                                delta: String::new(),
                                done: true,
                            });
                            return;
                        }
                    }
                    chunk = rx.recv() => {
                        match chunk {
                            Some(UnifiedStreamChunk::Delta(text)) => {
                                // If we were thinking, signal end of thinking
                                if in_thinking {
                                    let _ = window.emit("llm:thinking", ThinkingPayload {
                                        delta: String::new(),
                                        done: true,
                                    });
                                    in_thinking = false;
                                }

                                accumulated_content.push_str(&text);
                                buffer.push_str(&text);

                                // Use extracted filter function
                                let result = filter_tool_call_xml(&buffer, xml_depth);
                                xml_depth = result.new_depth;
                                buffer = result.new_buffer;

                                // Notify UI when entering a tool call
                                if result.entered_tool_call {
                                    let _ = window.emit("llm:status", StatusPayload {
                                        message: "Parsing tool call...".to_string(),
                                        icon: "🔍".to_string(),
                                    });
                                }

                                if !result.display_text.is_empty() {
                                    // Filter out narration patterns learned during fine-tuning
                                    let filtered = filter_narration_patterns(&result.display_text);
                                    if !filtered.is_empty() {
                                        let _ = window.emit("llm:chunk", StreamChunkPayload {
                                            delta: filtered,
                                            done: false,
                                        });
                                    }
                                }
                            }
                            Some(UnifiedStreamChunk::Thinking(text)) => {
                                // Stream thinking tokens live
                                in_thinking = true;
                                let _ = window.emit("llm:thinking", ThinkingPayload {
                                    delta: text,
                                    done: false,
                                });
                            }
                            Some(UnifiedStreamChunk::Done(response)) => {
                                // Signal end of thinking if we were still thinking
                                if in_thinking {
                                    let _ = window.emit("llm:thinking", ThinkingPayload {
                                        delta: String::new(),
                                        done: true,
                                    });
                                }

                                // Check if response was truncated due to length limit
                                if response.finish_reason == Some(devit_backend_core::FinishReason::Length) {
                                    tracing::warn!("Response truncated due to context length limit!");
                                    let _ = window.emit("llm:chunk", StreamChunkPayload {
                                        delta: "\n\n⚠️ **Response truncated** - Context limit reached. Try:\n- Clear chat history\n- Use `/compact` to compress context\n- Ask shorter questions".to_string(),
                                        done: false,
                                    });
                                }

                                // Emit any remaining buffered content (not in tool call)
                                if xml_depth == 0 && !buffer.is_empty() && !buffer.starts_with("<") {
                                    let _ = window.emit("llm:chunk", StreamChunkPayload {
                                        delta: buffer.clone(),
                                        done: false,
                                    });
                                }

                                // Emit stats if available (from llama.cpp timings)
                                tracing::debug!("Response timings: {:?}, usage: {:?}", response.timings, response.usage);
                                let stats = StatsPayload {
                                    tokens_per_second: response.timings.map(|t| t.tokens_per_second),
                                    total_tokens: response.usage.map(|u| u.total_tokens),
                                    prompt_tokens: response.usage.map(|u| u.prompt_tokens),
                                    completion_tokens: response.usage.map(|u| u.completion_tokens),
                                    total_ms: response.timings.map(|t| t.total_ms),
                                };
                                tracing::debug!("Stats payload: tps={:?}, tokens={:?}", stats.tokens_per_second, stats.total_tokens);
                                // Only emit if we have at least some stats
                                if stats.tokens_per_second.is_some() || stats.total_tokens.is_some() {
                                    tracing::info!("Emitting llm:stats event");
                                    let _ = window.emit("llm:stats", stats);
                                } else {
                                    tracing::warn!("No stats to emit (timings and usage both None)");
                                }

                                if let Some(calls) = response.tool_calls {
                                    final_tool_calls = Some(calls);
                                }
                                break;
                            }
                            Some(UnifiedStreamChunk::Error(err)) => {
                                let _ = window.emit("llm:chunk", StreamChunkPayload {
                                    delta: format!("❌ Error: {}", err),
                                    done: true,
                                });
                                return;
                            }
                            None => break,
                        }
                    }
                }
            }

            // Check for tool calls (native or parsed)
            let tool_calls = if let Some(calls) = final_tool_calls {
                calls
            } else {
                match DevItFormatParser::parse(&accumulated_content) {
                    Ok(calls) => calls,
                    Err(_) => vec![],
                }
            };

            if tool_calls.is_empty() {
                // No tool calls, we're done
                let clean_content = DevItFormatParser::strip_tool_calls(&accumulated_content);
                if !clean_content.is_empty() && clean_content != accumulated_content {
                    // Emit cleaned content if different
                }
                let _ = window.emit(
                    "llm:chunk",
                    StreamChunkPayload {
                        delta: String::new(),
                        done: true,
                    },
                );
                break;
            }

            // Execute tool calls
            for tool_call in tool_calls {
                let args_str = serde_json::to_string_pretty(&tool_call.arguments)
                    .unwrap_or_else(|_| "{}".to_string());

                // Notify UI of tool call
                let _ = window.emit(
                    "llm:tool_call",
                    ToolCallPayload {
                        name: tool_call.name.clone(),
                        args: args_str.clone(),
                    },
                );

                // Execute tool
                let result = match execute_tool(&tool_call, &tools).await {
                    Ok(r) => r,
                    Err(e) => format!("Error: {}", e),
                };

                // Notify UI of result
                let _ = window.emit(
                    "llm:tool_result",
                    ToolResultPayload {
                        name: tool_call.name.clone(),
                        result: result.clone(),
                    },
                );

                // Invalidate editor cache for file-modifying tools
                // This ensures the editor shows fresh content after LLM writes
                if matches!(tool_call.name.as_str(), "file_write" | "patch_apply") {
                    if let Some(path) = tool_call.arguments.get("path").and_then(|v| v.as_str()) {
                        let path_buf = std::path::PathBuf::from(path);
                        // Clear backend cache
                        {
                            let mut st = state_clone.write();
                            st.open_files.remove(&path_buf);
                        }
                        // Notify frontend to reload if file is open
                        let _ = window.emit(
                            "file:changed",
                            serde_json::json!({
                                "path": path,
                                "kind": "modified"
                            }),
                        );
                        tracing::debug!("Invalidated cache and notified UI for: {}", path);
                    }
                }

                // Check if using a custom devit model (uses XML format for tools)
                let is_custom_model = model_name.starts_with("devit-")
                    || model_name.contains("devit")
                    || model_name.contains("lora");

                if is_custom_model {
                    // For custom models: format tool call and result as XML in a user message
                    // This matches the training format the model understands
                    let args_json = serde_json::to_string_pretty(&tool_call.arguments)
                        .unwrap_or_else(|_| "{}".to_string());

                    let xml_message = format!(
                        "Tool execution completed:\n\n<tool_call>\n<name>{}</name>\n<arguments>\n{}\n</arguments>\n</tool_call>\n\n<tool_result name=\"{}\">\n{}\n</tool_result>\n\nPlease analyze the result and continue with your response.",
                        tool_call.name,
                        args_json,
                        tool_call.name,
                        result
                    );

                    messages.push(BackendMessage {
                        role: "user".to_string(),
                        content: xml_message,
                        tool_calls: None,
                        tool_name: None,
                        images: None,
                    });
                } else {
                    // For standard models: use assistant + tool role format
                    messages.push(BackendMessage {
                        role: "assistant".to_string(),
                        content: format!("[Executed {} - see result below]", tool_call.name),
                        tool_calls: None,
                        tool_name: Some(tool_call.name.clone()),
                        images: None,
                    });

                    messages.push(BackendMessage {
                        role: "tool".to_string(),
                        content: result,
                        tool_calls: None,
                        tool_name: Some(tool_call.name),
                        images: None,
                    });
                }
            }

            // Add a reminder to continue if there are more tasks
            // This helps models that tend to stop after one tool call
            // Skip for custom models since we already included continuation prompt in XML message
            let is_custom = model_name.starts_with("devit-")
                || model_name.contains("devit")
                || model_name.contains("lora");

            if !is_custom {
                messages.push(BackendMessage {
                    role: "user".to_string(),
                    content: "Continue with the remaining tasks. If all tasks are complete, provide a brief summary.".to_string(),
                    tool_calls: None,
                    tool_name: None,
                    images: None,
                });
            }

            // Continue loop to get next response with tool results
        }
    });

    Ok(())
}

/// Cancel ongoing stream
#[tauri::command]
pub async fn cancel_stream(_state: State<'_, Arc<RwLock<AppState>>>) -> Result<(), String> {
    let _ = get_cancel_channel().send(true);
    Ok(())
}

/// List available LLM providers
#[tauri::command]
pub async fn list_providers(
    state: State<'_, Arc<RwLock<AppState>>>,
) -> Result<Vec<Provider>, String> {
    // Get URLs from settings
    let (ollama_url, lmstudio_url, llamacpp_url) = {
        let st = state.read();
        (
            st.settings.ollama_url.clone(),
            st.settings.lmstudio_url.clone(),
            st.settings.llamacpp_url.clone(),
        )
    };

    // Probe local providers in parallel
    let (ollama_available, lmstudio_available, llamacpp_available, petals_available, claude_available) = tokio::join!(
        probe_ollama(&ollama_url),
        probe_lmstudio(&lmstudio_url),
        probe_llamacpp(&llamacpp_url),
        probe_petals(),
        devit_claude::probe()
    );

    Ok(vec![
        Provider {
            id: "ollama".into(),
            name: "Ollama".into(),
            kind: "local".into(),
            available: ollama_available,
        },
        Provider {
            id: "lmstudio".into(),
            name: "LM Studio".into(),
            kind: "local".into(),
            available: lmstudio_available,
        },
        Provider {
            id: "llamacpp".into(),
            name: "llama.cpp".into(),
            kind: "local".into(),
            available: llamacpp_available,
        },
        Provider {
            id: "petals".into(),
            name: "Petals (Distributed)".into(),
            kind: "distributed".into(),
            available: petals_available,
        },
        Provider {
            id: "claude".into(),
            name: "Claude (Anthropic)".into(),
            kind: "cloud".into(),
            available: claude_available,
        },
        Provider {
            id: "openai".into(),
            name: "OpenAI".into(),
            kind: "cloud".into(),
            available: false, // needs API key
        },
    ])
}

/// Probe if Ollama is running
async fn probe_ollama(ollama_url: &str) -> bool {
    let client = reqwest::Client::new();
    match client
        .get(format!("{}/api/version", ollama_url.trim_end_matches('/')))
        .timeout(std::time::Duration::from_secs(2))
        .send()
        .await
    {
        Ok(resp) => resp.status().is_success(),
        Err(_) => false,
    }
}

/// Probe if LM Studio is running (OpenAI-compatible API)
async fn probe_lmstudio(base_url: &str) -> bool {
    let client = reqwest::Client::new();
    let url = format!("{}/v1/models", base_url.trim_end_matches('/'));
    match client
        .get(&url)
        .timeout(std::time::Duration::from_secs(2))
        .send()
        .await
    {
        Ok(resp) => resp.status().is_success(),
        Err(_) => false,
    }
}

/// Probe if llama.cpp server is running (OpenAI-compatible API)
async fn probe_llamacpp(base_url: &str) -> bool {
    let client = reqwest::Client::new();
    let url = format!("{}/v1/models", base_url.trim_end_matches('/'));
    match client
        .get(&url)
        .timeout(std::time::Duration::from_secs(2))
        .send()
        .await
    {
        Ok(resp) => resp.status().is_success(),
        Err(_) => false,
    }
}

/// Probe if Petals API is reachable
async fn probe_petals() -> bool {
    let client = reqwest::Client::new();
    match client
        .get("https://chat.petals.dev/api/v1/models")
        .timeout(std::time::Duration::from_secs(3))
        .send()
        .await
    {
        Ok(resp) => resp.status().is_success(),
        Err(_) => false,
    }
}

/// Set active LLM provider
#[tauri::command]
pub async fn set_provider(
    provider_id: String,
    state: State<'_, Arc<RwLock<AppState>>>,
) -> Result<(), String> {
    tracing::info!("Setting provider to: {}", provider_id);
    let mut st = state.write();
    st.current_provider = Some(provider_id.clone());
    // Persist to settings
    st.settings.default_provider = provider_id;
    crate::ipc::settings::save_settings(&st.settings)?;
    Ok(())
}

/// Get default system prompt (for factory reset)
#[tauri::command]
pub async fn get_default_system_prompt() -> Result<String, String> {
    Ok(DEFAULT_SYSTEM_PROMPT.to_string())
}

/// Set active LLM model
#[tauri::command]
pub async fn set_model(
    model_name: String,
    state: State<'_, Arc<RwLock<AppState>>>,
) -> Result<(), String> {
    tracing::info!("Setting model to: {}", model_name);
    let mut st = state.write();
    st.current_model = Some(model_name.clone());
    // Persist to settings
    st.settings.default_model = Some(model_name);
    crate::ipc::settings::save_settings(&st.settings)?;
    Ok(())
}

#[derive(serde::Serialize)]
pub struct ModelInfo {
    pub name: String,
    pub size: String,
    pub modified: String,
}

/// List available models for the current provider
#[tauri::command]
pub async fn list_models(
    provider_id: Option<String>,
    state: State<'_, Arc<RwLock<AppState>>>,
) -> Result<Vec<ModelInfo>, String> {
    let (provider, ollama_url, lmstudio_url, llamacpp_url) = {
        let st = state.read();
        let p = provider_id.clone().unwrap_or_else(|| {
            st.current_provider
                .clone()
                .unwrap_or_else(|| "ollama".into())
        });
        (
            p,
            st.settings.ollama_url.clone(),
            st.settings.lmstudio_url.clone(),
            st.settings.llamacpp_url.clone(),
        )
    };

    match provider.as_str() {
        "ollama" => list_ollama_models(&ollama_url).await,
        "lmstudio" => list_lmstudio_models(&lmstudio_url).await,
        "llamacpp" => list_llamacpp_models(&llamacpp_url).await,
        "petals" => list_petals_models().await,
        "claude" => Ok(devit_claude::available_models()
            .into_iter()
            .map(|m| ModelInfo {
                name: m.id,
                size: m.name,
                modified: format!("{}k ctx", m.context / 1000),
            })
            .collect()),
        _ => Ok(vec![]),
    }
}

/// Fetch models from Ollama API
async fn list_ollama_models(ollama_url: &str) -> Result<Vec<ModelInfo>, String> {
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{}/api/tags", ollama_url.trim_end_matches('/')))
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await
        .map_err(|e| format!("Failed to connect to Ollama: {}", e))?;

    if !resp.status().is_success() {
        return Err("Ollama returned an error".into());
    }

    #[derive(serde::Deserialize)]
    struct OllamaResponse {
        models: Vec<OllamaModel>,
    }

    #[derive(serde::Deserialize)]
    struct OllamaModel {
        name: String,
        size: u64,
        modified_at: String,
    }

    let data: OllamaResponse = resp.json().await.map_err(|e| e.to_string())?;

    Ok(data
        .models
        .into_iter()
        .map(|m| ModelInfo {
            name: m.name,
            size: format_size(m.size),
            modified: m.modified_at.split('T').next().unwrap_or("").to_string(),
        })
        .collect())
}

fn format_size(bytes: u64) -> String {
    const GB: u64 = 1024 * 1024 * 1024;
    const MB: u64 = 1024 * 1024;
    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else {
        format!("{:.0} MB", bytes as f64 / MB as f64)
    }
}

/// Fetch models from LM Studio API (OpenAI-compatible)
async fn list_lmstudio_models(base_url: &str) -> Result<Vec<ModelInfo>, String> {
    let client = reqwest::Client::new();
    let url = format!("{}/v1/models", base_url.trim_end_matches('/'));
    let resp = client
        .get(&url)
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await
        .map_err(|e| format!("Failed to connect to LM Studio: {}", e))?;

    if !resp.status().is_success() {
        return Err("LM Studio returned an error".into());
    }

    #[derive(serde::Deserialize)]
    struct OpenAIModelsResponse {
        data: Vec<OpenAIModel>,
    }

    #[derive(serde::Deserialize)]
    struct OpenAIModel {
        id: String,
        #[serde(default)]
        owned_by: String,
    }

    let data: OpenAIModelsResponse = resp.json().await.map_err(|e| e.to_string())?;

    Ok(data
        .data
        .into_iter()
        .map(|m| ModelInfo {
            name: m.id,
            size: "-".to_string(), // LM Studio API doesn't provide size
            modified: m.owned_by,  // Use owned_by as info
        })
        .collect())
}

/// Fetch models from llama.cpp server (OpenAI-compatible API)
async fn list_llamacpp_models(base_url: &str) -> Result<Vec<ModelInfo>, String> {
    let client = reqwest::Client::new();
    let url = format!("{}/v1/models", base_url.trim_end_matches('/'));
    let resp = client
        .get(&url)
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await
        .map_err(|e| format!("Failed to connect to llama.cpp: {}", e))?;

    if !resp.status().is_success() {
        return Err("llama.cpp returned an error".into());
    }

    #[derive(serde::Deserialize)]
    struct OpenAIModelsResponse {
        data: Vec<OpenAIModel>,
    }

    #[derive(serde::Deserialize)]
    struct OpenAIModel {
        id: String,
    }

    let data: OpenAIModelsResponse = resp.json().await.map_err(|e| e.to_string())?;

    Ok(data
        .data
        .into_iter()
        .map(|m| ModelInfo {
            name: m.id,
            size: "-".to_string(),
            modified: "local".to_string(),
        })
        .collect())
}

/// Fetch available models from Petals
async fn list_petals_models() -> Result<Vec<ModelInfo>, String> {
    // Petals supports a fixed set of large models
    // These are the commonly available ones on the network
    Ok(vec![
        ModelInfo {
            name: "meta-llama/Meta-Llama-3.1-405B-Instruct".to_string(),
            size: "405B".to_string(),
            modified: "distributed".to_string(),
        },
        ModelInfo {
            name: "meta-llama/Llama-2-70b-chat-hf".to_string(),
            size: "70B".to_string(),
            modified: "distributed".to_string(),
        },
        ModelInfo {
            name: "mistralai/Mixtral-8x22B-Instruct-v0.1".to_string(),
            size: "8x22B".to_string(),
            modified: "distributed".to_string(),
        },
        ModelInfo {
            name: "bigscience/bloom".to_string(),
            size: "176B".to_string(),
            modified: "distributed".to_string(),
        },
    ])
}

/// Generate text using Petals API (simulates streaming by sending result at once)
async fn petals_generate(
    messages: &[BackendMessage],
    model: &str,
) -> Result<BackendStream, String> {
    let (tx, rx) = tokio::sync::mpsc::channel(10);

    // Build input from messages (concatenate user messages)
    let input: String = messages
        .iter()
        .filter(|m| m.role == "user" || m.role == "system")
        .map(|m| {
            if m.role == "system" {
                format!("System: {}\n", m.content)
            } else {
                m.content.clone()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");

    let model = model.to_string();

    tokio::spawn(async move {
        let client = reqwest::Client::new();

        #[derive(serde::Serialize)]
        struct PetalsRequest {
            model: String,
            inputs: String,
            max_new_tokens: u32,
            do_sample: u8,
            temperature: f32,
            top_p: f32,
        }

        #[derive(serde::Deserialize)]
        struct PetalsResponse {
            ok: bool,
            #[serde(default)]
            outputs: Option<String>,
            #[serde(default)]
            traceback: Option<String>,
        }

        let request = PetalsRequest {
            model,
            inputs: input,
            max_new_tokens: 512,
            do_sample: 1,
            temperature: 0.6,
            top_p: 0.9,
        };

        match client
            .post("https://chat.petals.dev/api/v1/generate")
            .form(&request)
            .timeout(std::time::Duration::from_secs(60))
            .send()
            .await
        {
            Ok(resp) => match resp.json::<PetalsResponse>().await {
                Ok(petals_resp) => {
                    if petals_resp.ok {
                        if let Some(output) = petals_resp.outputs {
                            // Send output as delta
                            let _ = tx.send(UnifiedStreamChunk::Delta(output.clone())).await;
                            // Send done
                            let response = devit_backend_core::RawChatResponse::new(output);
                            let _ = tx.send(UnifiedStreamChunk::Done(response)).await;
                        }
                    } else {
                        let error = petals_resp
                            .traceback
                            .unwrap_or_else(|| "Unknown error".into());
                        let _ = tx.send(UnifiedStreamChunk::Error(error)).await;
                    }
                }
                Err(e) => {
                    let _ = tx.send(UnifiedStreamChunk::Error(e.to_string())).await;
                }
            },
            Err(e) => {
                let _ = tx.send(UnifiedStreamChunk::Error(e.to_string())).await;
            }
        }
    });

    Ok(BackendStream::Petals(rx))
}

// ============================================================================
// Context Compression
// ============================================================================

/// Check if messages need compression and return compression info
fn check_compression_needed(
    messages: &[BackendMessage],
    max_context: usize,
) -> Option<(usize, Vec<BackendMessage>)> {
    let message_tokens: usize = messages.iter().map(estimate_message_tokens).sum();
    // Add system prompt overhead to get realistic total
    let total_tokens = message_tokens + SYSTEM_PROMPT_OVERHEAD;
    let threshold = (max_context as f32 * COMPRESSION_THRESHOLD) as usize;

    if total_tokens < threshold || messages.len() < 6 {
        return None;
    }

    // Find messages to compress (keep last ~60% by tokens)
    let target_keep_tokens = (max_context as f32 * 0.6) as usize;
    let mut tokens_from_end = 0;
    let mut keep_count = 0;

    for msg in messages.iter().rev() {
        let msg_tokens = estimate_message_tokens(msg);
        if tokens_from_end + msg_tokens > target_keep_tokens && keep_count > 1 {
            break;
        }
        tokens_from_end += msg_tokens;
        keep_count += 1;
    }

    let compress_count = messages.len().saturating_sub(keep_count);
    if compress_count < 2 {
        return None;
    }

    // Skip system message (index 0) from compression
    let start_idx = if messages
        .first()
        .map(|m| m.role == "system")
        .unwrap_or(false)
    {
        1
    } else {
        0
    };

    if compress_count <= start_idx {
        return None;
    }

    let to_compress: Vec<BackendMessage> = messages[start_idx..compress_count].to_vec();
    Some((compress_count - start_idx, to_compress))
}

/// Build a compression prompt from messages
fn build_compression_prompt(messages: &[BackendMessage]) -> String {
    let mut prompt = SUMMARIZE_PROMPT.to_string();

    for msg in messages {
        let role_label = match msg.role.as_str() {
            "user" => "User",
            "assistant" => "Assistant",
            "system" => "System",
            "tool" => "Tool Result",
            _ => &msg.role,
        };
        prompt.push_str(&format!("\n{}: {}\n", role_label, msg.content));
    }

    prompt
}

/// Compress context using LLM (blocking call)
async fn compress_context_with_llm(
    messages_to_compress: Vec<BackendMessage>,
    provider_id: &str,
    model_name: &str,
    ollama_url: &str,
    lmstudio_url: &str,
    llamacpp_url: &str,
) -> Result<TopicSummary, String> {
    let prompt = build_compression_prompt(&messages_to_compress);
    let message_count = messages_to_compress.len();

    // Create a simple request for summarization
    let request = ChatRequest::new(vec![BackendMessage {
        role: "user".to_string(),
        content: prompt,
        tool_calls: None,
        tool_name: None,
        images: None,
    }]);

    // Use the appropriate backend
    let response_text = match provider_id {
        "ollama" => {
            let backend = OllamaBackend::new(ollama_url.to_string(), model_name.to_string());
            let resp = backend.chat(request).await.map_err(|e| e.to_string())?;
            resp.content
        }
        "lmstudio" => {
            let backend = LmstudioBackend::new(
                format!("{}/v1", lmstudio_url.trim_end_matches('/')),
                model_name.to_string(),
            );
            let resp = backend.chat(request).await.map_err(|e| e.to_string())?;
            resp.content
        }
        "llamacpp" => {
            let backend = LlamaCppBackend::new(
                format!("{}/v1", llamacpp_url.trim_end_matches('/')),
                model_name.to_string(),
            );
            let resp = backend.chat(request).await.map_err(|e| e.to_string())?;
            resp.content
        }
        "claude" => {
            let backend = ClaudeBackend::new(model_name.to_string());
            let resp = backend.chat(request).await.map_err(|e| e.to_string())?;
            resp.content
        }
        _ => {
            return Err(format!(
                "Compression not supported for provider: {}",
                provider_id
            ))
        }
    };

    parse_summary_response(&response_text, message_count)
        .ok_or_else(|| "Failed to parse compression response".to_string())
}

/// Force context compression (for /compact command)
/// If force=true, compresses even if under threshold (useful after truncation)
#[tauri::command]
pub async fn compact_context(
    messages: Vec<ChatMessage>,
    force: Option<bool>,
    state: State<'_, Arc<RwLock<AppState>>>,
) -> Result<CompactResult, String> {
    let (provider_id, model_name, ollama_url, lmstudio_url, llamacpp_url) = {
        let st = state.read();
        let provider = st
            .current_provider
            .clone()
            .unwrap_or_else(|| "ollama".into());
        let model = st
            .current_model
            .clone()
            .unwrap_or_else(|| "qwen2.5-coder:7b".into());
        (
            provider,
            model,
            st.settings.ollama_url.clone(),
            st.settings.lmstudio_url.clone(),
            st.settings.llamacpp_url.clone(),
        )
    };

    // Convert to backend messages
    let backend_messages: Vec<BackendMessage> = messages
        .into_iter()
        .map(|msg| BackendMessage {
            role: msg.role,
            content: msg.content,
            tool_calls: None,
            tool_name: None,
            images: None,
        })
        .collect();

    // Check what can be compressed
    let message_count = backend_messages.len();
    let estimated_tokens: usize = backend_messages.iter().map(estimate_message_tokens).sum();
    let total_with_overhead = estimated_tokens + SYSTEM_PROMPT_OVERHEAD;
    let threshold = (DEFAULT_MAX_CONTEXT as f32 * COMPRESSION_THRESHOLD) as usize;
    let force = force.unwrap_or(false);

    tracing::info!(
        "compact_context: {} messages, ~{} estimated tokens, threshold={}, force={}",
        message_count,
        total_with_overhead,
        threshold,
        force
    );

    let (compress_count, to_compress) = if force && message_count >= 6 {
        // Force mode: compress regardless of token threshold
        // Compress first 60% of messages (keep last 40%)
        let compress_count = (message_count as f32 * 0.6) as usize;
        if compress_count < 2 {
            return Err(format!(
                "Not enough messages to compress - {} messages (need at least 6)",
                message_count
            ));
        }
        // Skip system message if present
        let start_idx = if backend_messages
            .first()
            .map(|m| m.role == "system")
            .unwrap_or(false)
        {
            1
        } else {
            0
        };
        let to_compress = backend_messages[start_idx..compress_count].to_vec();
        (compress_count - start_idx, to_compress)
    } else {
        check_compression_needed(&backend_messages, DEFAULT_MAX_CONTEXT)
            .ok_or_else(|| {
                format!(
                    "No compression needed - {} messages, ~{} tokens (threshold: {}). Use /compact force to override.",
                    message_count, total_with_overhead, threshold
                )
            })?
    };

    // Compress using LLM
    let topic = compress_context_with_llm(
        to_compress,
        &provider_id,
        &model_name,
        &ollama_url,
        &lmstudio_url,
        &llamacpp_url,
    )
    .await?;

    // Build result with remaining messages
    let remaining: Vec<CompactMessage> = backend_messages[compress_count..]
        .iter()
        .map(|m| CompactMessage {
            role: m.role.clone(),
            content: m.content.clone(),
        })
        .collect();

    Ok(CompactResult {
        topic,
        remaining_messages: remaining,
        compressed_count: compress_count,
    })
}

/// Result of context compaction
#[derive(serde::Serialize)]
pub struct CompactResult {
    pub topic: TopicSummary,
    pub remaining_messages: Vec<CompactMessage>,
    pub compressed_count: usize,
}

/// Compute a usage percentage clamped to 0..=100, returned in a single byte.
fn percent_u8(used: usize, max: usize) -> u8 {
    if max == 0 {
        return 0;
    }
    let pct = used.saturating_mul(100) / max;
    u8::try_from(pct.min(100)).unwrap_or(100)
}

/// Simplified message for compact result
#[derive(serde::Serialize, Clone)]
pub struct CompactMessage {
    pub role: String,
    pub content: String,
}

/// Get estimated token count for messages
#[tauri::command]
pub async fn estimate_context_tokens(
    messages: Vec<ChatMessage>,
) -> Result<ContextEstimate, String> {
    let total: usize = messages
        .iter()
        .map(|m| {
            let msg = BackendMessage {
                role: m.role.clone(),
                content: m.content.clone(),
                tool_calls: None,
                tool_name: None,
                images: None,
            };
            estimate_message_tokens(&msg)
        })
        .sum();

    // Add system prompt overhead for realistic estimate
    let total_with_overhead = total + SYSTEM_PROMPT_OVERHEAD;
    let needs_compression =
        total_with_overhead > (DEFAULT_MAX_CONTEXT as f32 * COMPRESSION_THRESHOLD) as usize;

    Ok(ContextEstimate {
        total_tokens: total_with_overhead,
        max_tokens: DEFAULT_MAX_CONTEXT,
        usage_percent: percent_u8(total_with_overhead, DEFAULT_MAX_CONTEXT),
        needs_compression,
    })
}

#[derive(serde::Serialize)]
pub struct ContextEstimate {
    pub total_tokens: usize,
    pub max_tokens: usize,
    pub usage_percent: u8,
    pub needs_compression: bool,
}

/// Server properties (context size, model info, etc.)
#[derive(serde::Serialize, serde::Deserialize, Default)]
pub struct ServerProps {
    pub n_ctx: usize,
    pub model_alias: Option<String>,
    pub total_slots: Option<u32>,
}

/// Get server properties (context size, etc.) from the active provider
#[tauri::command]
pub async fn get_server_props(
    state: State<'_, Arc<RwLock<AppState>>>,
) -> Result<ServerProps, String> {
    let (provider_id, llamacpp_url, lmstudio_url) = {
        let st = state.read();
        let provider = st
            .current_provider
            .clone()
            .unwrap_or_else(|| "ollama".into());
        (
            provider,
            st.settings.llamacpp_url.clone(),
            st.settings.lmstudio_url.clone(),
        )
    };

    let client = reqwest::Client::new();

    match provider_id.as_str() {
        "llamacpp" => {
            // llama.cpp /props endpoint
            let url = format!("{}/props", llamacpp_url.trim_end_matches('/'));
            match client
                .get(&url)
                .timeout(std::time::Duration::from_secs(3))
                .send()
                .await
            {
                Ok(resp) if resp.status().is_success() => {
                    #[derive(serde::Deserialize)]
                    struct LlamaCppProps {
                        default_generation_settings: Option<DefaultGenSettings>,
                        model_alias: Option<String>,
                        total_slots: Option<u32>,
                    }
                    #[derive(serde::Deserialize)]
                    struct DefaultGenSettings {
                        n_ctx: Option<usize>,
                    }

                    if let Ok(props) = resp.json::<LlamaCppProps>().await {
                        let n_ctx = props
                            .default_generation_settings
                            .and_then(|s| s.n_ctx)
                            .unwrap_or(DEFAULT_MAX_CONTEXT);
                        return Ok(ServerProps {
                            n_ctx,
                            model_alias: props.model_alias,
                            total_slots: props.total_slots,
                        });
                    }
                }
                _ => {}
            }
        }
        "lmstudio" => {
            // LM Studio doesn't have a props endpoint, use default
            // Could try to get from /v1/models but context size isn't exposed
        }
        "ollama" => {
            // Ollama: could get from /api/show but need model name
            // For now use default
        }
        "claude" => {
            return Ok(ServerProps {
                n_ctx: 200_000,
                model_alias: Some("claude".to_string()),
                total_slots: None,
            });
        }
        _ => {}
    }

    // Default fallback
    Ok(ServerProps {
        n_ctx: DEFAULT_MAX_CONTEXT,
        model_alias: None,
        total_slots: None,
    })
}

/// Get estimated token count with real server context size
#[tauri::command]
pub async fn estimate_context_tokens_v2(
    messages: Vec<ChatMessage>,
    state: State<'_, Arc<RwLock<AppState>>>,
) -> Result<ContextEstimate, String> {
    // Get real context size from server
    let server_props = get_server_props(state).await.unwrap_or_default();
    let max_context = server_props.n_ctx;

    let total: usize = messages
        .iter()
        .map(|m| {
            let msg = BackendMessage {
                role: m.role.clone(),
                content: m.content.clone(),
                tool_calls: None,
                tool_name: None,
                images: None,
            };
            estimate_message_tokens(&msg)
        })
        .sum();

    // Add system prompt overhead for realistic estimate
    let total_with_overhead = total + SYSTEM_PROMPT_OVERHEAD;
    let needs_compression =
        total_with_overhead > (max_context as f32 * COMPRESSION_THRESHOLD) as usize;

    Ok(ContextEstimate {
        total_tokens: total_with_overhead,
        max_tokens: max_context,
        usage_percent: percent_u8(total_with_overhead, max_context),
        needs_compression,
    })
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_filter_plain_text() {
        let result = filter_tool_call_xml("Hello world!", 0);
        assert_eq!(result.display_text, "Hello world!");
        assert_eq!(result.new_buffer, "");
        assert_eq!(result.new_depth, 0);
        assert!(!result.entered_tool_call);
    }

    #[test]
    fn test_filter_complete_tool_call() {
        let input = "Before <tool_call><tool_name>shell</tool_name></tool_call> After";
        let result = filter_tool_call_xml(input, 0);
        assert_eq!(result.display_text, "Before  After");
        assert_eq!(result.new_depth, 0);
        assert!(result.entered_tool_call);
    }

    #[test]
    fn test_filter_tool_call_hides_content() {
        let input = "<tool_call>hidden content</tool_call>";
        let result = filter_tool_call_xml(input, 0);
        assert_eq!(result.display_text, "");
        assert_eq!(result.new_depth, 0);
    }

    #[test]
    fn test_filter_partial_open_tag() {
        // Simulates streaming where "<tool" arrives first
        let result = filter_tool_call_xml("Hello <tool", 0);
        assert_eq!(result.display_text, "Hello ");
        assert_eq!(result.new_buffer, "<tool");
        assert_eq!(result.new_depth, 0);
    }

    #[test]
    fn test_filter_partial_tag_completes() {
        // First chunk: partial tag buffered
        let r1 = filter_tool_call_xml("Hello <tool", 0);
        assert_eq!(r1.new_buffer, "<tool");

        // Second chunk: completes the tag
        let combined = format!("{}_call>content</tool_call> done", r1.new_buffer);
        let r2 = filter_tool_call_xml(&combined, r1.new_depth);
        assert_eq!(r2.display_text, " done");
        assert_eq!(r2.new_depth, 0);
    }

    #[test]
    fn test_filter_streaming_chunks() {
        // Simulate real streaming: chunks arrive piece by piece
        let chunks = vec![
            "Je commence ",
            "maintenant !\n\n",
            "<",
            "tool",
            "_call",
            ">\n<tool_name>",
            "shell",
            "</tool_name>\n",
            "<arguments>",
            "{\"command\": \"ls\"}",
            "</arguments>\n",
            "</tool_call>",
            "\nDone!",
        ];

        let mut buffer = String::new();
        let mut depth = 0;
        let mut all_display = String::new();

        for chunk in chunks {
            buffer.push_str(chunk);
            let result = filter_tool_call_xml(&buffer, depth);
            all_display.push_str(&result.display_text);
            buffer = result.new_buffer;
            depth = result.new_depth;
        }

        assert_eq!(all_display, "Je commence maintenant !\n\n\nDone!");
        assert_eq!(depth, 0);
    }

    #[test]
    fn test_filter_utf8_french() {
        let input = "Création du fichier réussi ! 🎉";
        let result = filter_tool_call_xml(input, 0);
        assert_eq!(result.display_text, input);
    }

    #[test]
    fn test_filter_nested_angle_brackets() {
        // Other < > should not be filtered
        let input = "if x < 10 && y > 5";
        let result = filter_tool_call_xml(input, 0);
        assert_eq!(result.display_text, input);
    }

    #[test]
    fn test_filter_inside_tool_call() {
        // Start already inside a tool_call
        let input = "more content</tool_call>visible";
        let result = filter_tool_call_xml(input, 1);
        assert_eq!(result.display_text, "visible");
        assert_eq!(result.new_depth, 0);
    }

    #[test]
    fn test_filter_multiple_tool_calls() {
        let input = "A<tool_call>x</tool_call>B<tool_call>y</tool_call>C";
        let result = filter_tool_call_xml(input, 0);
        assert_eq!(result.display_text, "ABC");
    }
}
