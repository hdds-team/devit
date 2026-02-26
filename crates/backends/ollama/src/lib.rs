// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2025-2026 naskel.com

// # -----------------------------
// # crates/backends/ollama/src/lib.rs
// # -----------------------------
use anyhow::Result;
use async_trait::async_trait;
use devit_backend_core::{
    tool_calling::ToolCall, ChatRequest, FinishReason, LlmBackend, MetricsProvider, ModelInfo,
    RawChatResponse, Timings, Usage,
};
use futures_util::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::mpsc;

/// Ollama native API client using /api/chat (non-streaming)
#[derive(Clone)]
pub struct OllamaBackend {
    base_url: String,
    model: String,
    http: Client,
}

impl OllamaBackend {
    pub fn new(base_url: String, model: String) -> Self {
        Self {
            base_url,
            model,
            http: Client::new(),
        }
    }

    /// Extract model family from model name for format detection
    fn detect_model_family(model_name: &str) -> Option<String> {
        let lower = model_name.to_lowercase();
        if lower.contains("qwen") {
            Some("qwen".to_string())
        } else if lower.contains("llama") {
            Some("llama3".to_string())
        } else if lower.contains("mistral") {
            Some("mistral".to_string())
        } else if lower.contains("codellama") {
            Some("llama3".to_string())
        } else {
            None
        }
    }

    /// Check if model supports native Ollama tool calling
    /// Custom/fine-tuned models (like devit-*) use XML format instead
    /// Vision models don't support tool calling
    fn supports_native_tools(model_name: &str) -> bool {
        let lower = model_name.to_lowercase();
        // Custom devit models use XML tool format, not native Ollama tools
        if lower.starts_with("devit") {
            return false;
        }
        // Vision models don't support tool calling
        if lower.contains("vision") || lower.contains("llava") || lower.contains("moondream") {
            return false;
        }
        // Known models that support native tool calling in Ollama
        // Most base models from major providers support it
        lower.contains("qwen")
            || lower.contains("llama")
            || lower.contains("mistral")
            || lower.contains("gemma")
            || lower.contains("phi")
    }

    /// Stream chat response - returns a receiver for streaming chunks
    pub async fn chat_stream(&self, request: ChatRequest) -> Result<mpsc::Receiver<StreamChunk>> {
        let url = format!("{}/api/chat", self.base_url);

        // Convert ChatRequest to Ollama format
        let ollama_messages: Vec<OllamaMessage> = request
            .messages
            .iter()
            .map(|msg| OllamaMessage {
                role: msg.role.clone(),
                content: msg.content.clone(),
                tool_calls: msg.tool_calls.as_ref().map(|calls| {
                    calls
                        .iter()
                        .map(|call| OllamaToolCall {
                            function: OllamaToolCallFunction {
                                name: call.name.clone(),
                                arguments: call.arguments.clone(),
                            },
                        })
                        .collect()
                }),
                tool_name: msg.tool_name.clone(),
                images: msg.images.clone(), // Pass images for vision models
            })
            .collect();

        // Only send tools if model supports native Ollama tool calling
        let tools = if Self::supports_native_tools(&self.model) {
            request
                .tools
                .as_ref()
                .and_then(|t| serde_json::from_value(t.clone()).ok())
        } else {
            None // Custom models (devit-*) use XML format instead
        };

        let req = OllamaChatRequest {
            model: &self.model,
            messages: ollama_messages,
            stream: true, // Enable streaming
            options: Some(OllamaOptions {
                temperature: request.temperature,
                num_predict: request
                    .max_tokens
                    .map(|t| i32::try_from(t).unwrap_or(i32::MAX)),
            }),
            tools,
        };

        // Debug: log if any messages have images
        for (i, msg) in req.messages.iter().enumerate() {
            if let Some(images) = &msg.images {
                eprintln!(
                    "[DEBUG] Message {} has {} image(s), first image len: {} bytes",
                    i,
                    images.len(),
                    images.first().map(|s| s.len()).unwrap_or(0)
                );
            }
        }

        let response = self
            .http
            .post(&url)
            .json(&req)
            .send()
            .await?
            .error_for_status()?;

        let (tx, rx) = mpsc::channel(100);
        let mut stream = response.bytes_stream();

        tokio::spawn(async move {
            let mut accumulated_content = String::new();
            let mut tool_calls: Option<Vec<ToolCall>> = None;

            while let Some(chunk_result) = stream.next().await {
                match chunk_result {
                    Ok(bytes) => {
                        // Ollama sends NDJSON (newline-delimited JSON)
                        let text = String::from_utf8_lossy(&bytes);
                        for line in text.lines() {
                            if line.trim().is_empty() {
                                continue;
                            }
                            if let Ok(chunk) = serde_json::from_str::<OllamaStreamChunk>(line) {
                                // Send thinking delta
                                if let Some(thinking) = &chunk.message.thinking {
                                    if !thinking.is_empty() {
                                        let _ =
                                            tx.send(StreamChunk::Thinking(thinking.clone())).await;
                                    }
                                }

                                // Send content delta
                                if !chunk.message.content.is_empty() {
                                    accumulated_content.push_str(&chunk.message.content);
                                    let _ = tx
                                        .send(StreamChunk::Delta(chunk.message.content.clone()))
                                        .await;
                                }

                                // Accumulate tool calls if present
                                if let Some(calls) = &chunk.message.tool_calls {
                                    let converted: Vec<ToolCall> = calls
                                        .iter()
                                        .map(|c| ToolCall {
                                            name: c.function.name.clone(),
                                            arguments: normalize_arguments(&c.function.arguments),
                                        })
                                        .collect();
                                    tool_calls = Some(converted);
                                }

                                // Check if done
                                if chunk.done {
                                    let mut final_response =
                                        RawChatResponse::new(accumulated_content.clone())
                                            .with_finish_reason(
                                                match chunk.done_reason.as_deref() {
                                                    Some("stop") => FinishReason::Stop,
                                                    Some("length") => FinishReason::Length,
                                                    _ => FinishReason::Other,
                                                },
                                            )
                                            .with_usage(Usage {
                                                prompt_tokens: u32::try_from(
                                                    chunk.prompt_eval_count.unwrap_or(0),
                                                )
                                                .unwrap_or(0),
                                                completion_tokens: u32::try_from(
                                                    chunk.eval_count.unwrap_or(0),
                                                )
                                                .unwrap_or(0),
                                                total_tokens: u32::try_from(
                                                    chunk
                                                        .prompt_eval_count
                                                        .unwrap_or(0)
                                                        .saturating_add(
                                                            chunk.eval_count.unwrap_or(0),
                                                        ),
                                                )
                                                .unwrap_or(0),
                                            });

                                    // Add timing stats if available
                                    if let (Some(eval_count), Some(eval_duration)) =
                                        (chunk.eval_count, chunk.eval_duration)
                                    {
                                        if eval_duration > 0 {
                                            // eval_duration is in nanoseconds, convert to ms and calculate tok/s
                                            let eval_ms = eval_duration as f64 / 1_000_000.0;
                                            let prompt_ms = chunk.prompt_eval_duration.unwrap_or(0)
                                                as f64
                                                / 1_000_000.0;
                                            let total_ms = chunk.total_duration.unwrap_or(0) as f64
                                                / 1_000_000.0;
                                            let tokens_per_second = eval_count as f64
                                                / (eval_duration as f64 / 1_000_000_000.0);

                                            final_response = final_response.with_timings(Timings {
                                                tokens_per_second,
                                                prompt_ms,
                                                generation_ms: eval_ms,
                                                total_ms,
                                            });
                                        }
                                    }

                                    let final_response = if let Some(calls) = tool_calls.take() {
                                        final_response.with_tool_calls(calls)
                                    } else {
                                        final_response
                                    };

                                    let _ = tx.send(StreamChunk::Done(final_response)).await;
                                    return;
                                }
                            }
                        }
                    }
                    Err(e) => {
                        let _ = tx.send(StreamChunk::Error(e.to_string())).await;
                        return;
                    }
                }
            }
        });

        Ok(rx)
    }
}

/// Chunk types for streaming responses
#[derive(Debug, Clone)]
pub enum StreamChunk {
    /// Text delta (partial content)
    Delta(String),
    /// Thinking/reasoning content (for models with chain-of-thought)
    Thinking(String),
    /// Final response with all metadata
    Done(RawChatResponse),
    /// Error occurred
    Error(String),
}

#[async_trait]
impl LlmBackend for OllamaBackend {
    async fn chat(&self, request: ChatRequest) -> Result<RawChatResponse> {
        let url = format!("{}/api/chat", self.base_url);

        // Convert ChatRequest to Ollama format
        let ollama_messages: Vec<OllamaMessage> = request
            .messages
            .iter()
            .map(|msg| OllamaMessage {
                role: msg.role.clone(),
                content: msg.content.clone(),
                tool_calls: msg.tool_calls.as_ref().map(|calls| {
                    calls
                        .iter()
                        .map(|call| OllamaToolCall {
                            function: OllamaToolCallFunction {
                                name: call.name.clone(),
                                arguments: call.arguments.clone(),
                            },
                        })
                        .collect()
                }),
                tool_name: msg.tool_name.clone(),
                images: msg.images.clone(), // Pass images for vision models
            })
            .collect();

        // Only send tools if model supports native Ollama tool calling
        let tools = if Self::supports_native_tools(&self.model) {
            request
                .tools
                .as_ref()
                .and_then(|t| serde_json::from_value(t.clone()).ok())
        } else {
            None // Custom models (devit-*) use XML format instead
        };

        let req = OllamaChatRequest {
            model: &self.model,
            messages: ollama_messages,
            stream: false,
            options: Some(OllamaOptions {
                temperature: request.temperature,
                num_predict: request
                    .max_tokens
                    .map(|t| i32::try_from(t).unwrap_or(i32::MAX)),
            }),
            tools,
        };

        let resp: OllamaChatResponse = self
            .http
            .post(&url)
            .json(&req)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        // Convert Ollama tool calls to devit ToolCall format
        let tool_calls = resp.message.tool_calls.as_ref().map(|calls| {
            calls
                .iter()
                .map(|call| ToolCall {
                    name: call.function.name.clone(),
                    arguments: normalize_arguments(&call.function.arguments),
                })
                .collect()
        });

        // Build RawChatResponse
        let mut raw_response = RawChatResponse::new(resp.message.content.clone())
            .with_finish_reason(match resp.done_reason.as_deref() {
                Some("stop") => FinishReason::Stop,
                Some("length") => FinishReason::Length,
                _ => FinishReason::Other,
            })
            .with_usage(Usage {
                prompt_tokens: u32::try_from(resp.prompt_eval_count.unwrap_or(0)).unwrap_or(0),
                completion_tokens: u32::try_from(resp.eval_count.unwrap_or(0)).unwrap_or(0),
                total_tokens: u32::try_from(
                    resp.prompt_eval_count
                        .unwrap_or(0)
                        .saturating_add(resp.eval_count.unwrap_or(0)),
                )
                .unwrap_or(0),
            })
            .with_raw_data(serde_json::to_value(&resp)?);

        if let Some(calls) = tool_calls {
            raw_response = raw_response.with_tool_calls(calls);
        }

        Ok(raw_response)
    }

    async fn get_model_info(&self) -> Result<ModelInfo> {
        // Try to get model details from /api/show
        let url = format!("{}/api/show", self.base_url);
        let show_req = serde_json::json!({"name": self.model});

        let model_info = match self
            .http
            .post(&url)
            .json(&show_req)
            .send()
            .await?
            .error_for_status()
        {
            Ok(resp) => {
                let show_resp: OllamaShowResponse = resp.json().await?;
                ModelInfo {
                    name: self.model.clone(),
                    family: Self::detect_model_family(&self.model),
                    context_window: show_resp.model_info.and_then(|info| {
                        info.get("context_length")
                            .and_then(|v| v.as_u64())
                            .and_then(|v| u32::try_from(v).ok())
                    }),
                    supports_native_tools: false, // Ollama doesn't support native tool calling yet
                    max_output_tokens: None,
                }
            }
            Err(_) => {
                // Fallback if /api/show fails
                ModelInfo {
                    name: self.model.clone(),
                    family: Self::detect_model_family(&self.model),
                    context_window: Some(32768), // Reasonable default
                    supports_native_tools: false,
                    max_output_tokens: None,
                }
            }
        };

        Ok(model_info)
    }
}

#[async_trait]
impl MetricsProvider for OllamaBackend {
    async fn get_metrics(&self) -> Result<Value> {
        let url = format!("{}/api/ps", self.base_url);
        let resp: ProcessListResponse = self
            .http
            .get(&url)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        Ok(serde_json::to_value(resp)?)
    }
}

// Ollama-specific types
#[derive(Debug, Clone, Serialize, Deserialize)]
struct OllamaMessage {
    role: String,
    content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<OllamaToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_name: Option<String>,
    /// Images for vision models (base64 encoded)
    #[serde(skip_serializing_if = "Option::is_none")]
    images: Option<Vec<String>>,
}

#[derive(Serialize)]
struct OllamaChatRequest<'a> {
    model: &'a str,
    messages: Vec<OllamaMessage>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    options: Option<OllamaOptions>,
    /// Empty tools array disables Ollama's native tool calling parser
    /// This allows us to use our own XML-based tool format
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<serde_json::Value>>,
}

#[derive(Serialize)]
struct OllamaOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    num_predict: Option<i32>,
}

#[derive(Debug, Serialize, Deserialize)]
struct OllamaChatResponse {
    message: OllamaMessageResponse,
    #[serde(default)]
    done_reason: Option<String>,
    #[serde(default)]
    prompt_eval_count: Option<i64>,
    #[serde(default)]
    eval_count: Option<i64>,
}

/// Streaming response chunk from Ollama
#[derive(Debug, Deserialize)]
struct OllamaStreamChunk {
    message: OllamaMessageResponse,
    #[serde(default)]
    done: bool,
    #[serde(default)]
    done_reason: Option<String>,
    #[serde(default)]
    prompt_eval_count: Option<i64>,
    #[serde(default)]
    eval_count: Option<i64>,
    /// Prompt evaluation duration in nanoseconds
    #[serde(default)]
    prompt_eval_duration: Option<i64>,
    /// Generation duration in nanoseconds
    #[serde(default)]
    eval_duration: Option<i64>,
    /// Total duration in nanoseconds
    #[serde(default)]
    total_duration: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize)]
struct OllamaMessageResponse {
    content: String,
    #[serde(default)]
    tool_calls: Option<Vec<OllamaToolCall>>,
    #[serde(default)]
    thinking: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OllamaToolCall {
    function: OllamaToolCallFunction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OllamaToolCallFunction {
    name: String,
    arguments: Value,
}

/// Normalize tool call arguments: some LLMs return arguments as a JSON string
/// instead of a JSON object. This function parses the string if needed.
fn normalize_arguments(args: &Value) -> Value {
    match args {
        Value::String(s) => {
            // Try to parse the string as JSON object
            serde_json::from_str(s).unwrap_or_else(|_| args.clone())
        }
        _ => args.clone(),
    }
}

#[derive(Debug, Deserialize)]
struct OllamaShowResponse {
    #[serde(default)]
    model_info: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessListResponse {
    pub models: Vec<ProcessMetrics>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessMetrics {
    pub name: String,
    pub model: String,
    pub size: u64,
    #[serde(default)]
    pub size_vram: u64,
    #[serde(default)]
    pub context_length: u32,
    #[serde(default)]
    pub context_metrics: Option<ContextMetrics>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextMetrics {
    pub context_used: i64,
    pub context_max: i64,
    pub context_percent: f32,
}

impl ProcessMetrics {
    /// Format cache metrics for display
    pub fn format_cache(&self) -> String {
        if self.size_vram > 0 && self.context_length > 0 {
            let vram_gb = self.size_vram as f64 / 1_000_000_000.0;
            format!(
                "Model: {:.1}GB VRAM | Context: {}/32k",
                vram_gb,
                self.context_length / 1024
            )
        } else if let Some(metrics) = &self.context_metrics {
            format!(
                "Cache: {}/{} ({:.1}%)",
                metrics.context_used, metrics.context_max, metrics.context_percent
            )
        } else {
            "Model: Ready".to_string()
        }
    }
}
