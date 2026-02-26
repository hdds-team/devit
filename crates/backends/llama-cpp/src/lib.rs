// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2025-2026 naskel.com

//! llama.cpp backend - compatible with OpenAI-like API (localhost:8000)
//!
//! llama.cpp provides a local chat API compatible with OpenAI format.
//! Default: http://localhost:8000/v1/chat/completions

use anyhow::Result;
use async_trait::async_trait;
use devit_backend_core::{
    ChatRequest, FinishReason, LlmBackend, MetricsProvider, ModelInfo, RawChatResponse, Timings,
    Usage,
};
use futures_util::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::mpsc;

/// llama.cpp backend using OpenAI-compatible API
#[derive(Clone)]
pub struct LlamaCppBackend {
    base_url: String,
    model: String,
    http: Client,
}

impl LlamaCppBackend {
    pub fn new(base_url: String, model: String) -> Self {
        Self {
            base_url,
            model,
            http: Client::new(),
        }
    }

    /// Create with default llama.cpp endpoint (localhost:8000)
    pub fn with_default() -> Self {
        Self::new(
            "http://localhost:8000/v1".to_string(),
            "local-model".to_string(),
        )
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

    /// Stream chat response - returns a receiver for streaming chunks
    pub async fn chat_stream(&self, request: ChatRequest) -> Result<mpsc::Receiver<StreamChunk>> {
        let url = format!("{}/chat/completions", self.base_url);
        eprintln!("[DEBUG] llama.cpp chat_stream: url={}", url);

        // Convert ChatRequest to OpenAI format
        let openai_messages: Vec<OpenAIMessage> = request
            .messages
            .iter()
            .map(|msg| OpenAIMessage {
                role: msg.role.clone(),
                content: msg.content.clone(),
            })
            .collect();

        // Pass tools to API for native tool calling (OpenAI format)
        // GLM and other models use native tool_calls instead of XML format
        let has_tools = request.tools.is_some();
        let tools_count = request
            .tools
            .as_ref()
            .and_then(|t| t.as_array())
            .map(|a| a.len())
            .unwrap_or(0);

        eprintln!(
            "[DEBUG] llama.cpp sending request with {} tools...",
            tools_count
        );

        let req = OpenAIChatRequest {
            model: &self.model,
            messages: openai_messages.clone(),
            stream: true,
            temperature: request.temperature,
            max_tokens: request.max_tokens,
            tools: request.tools,
        };

        // Try with tools first
        let response = self.http.post(&url).json(&req).send().await?;
        eprintln!(
            "[DEBUG] llama.cpp got response: status={}",
            response.status()
        );

        // Fallback to without tools on 400 error (server doesn't support tool calling)
        let response = if response.status() == reqwest::StatusCode::BAD_REQUEST && has_tools {
            eprintln!("[DEBUG] llama.cpp 400 error with tools, retrying without tools...");
            let req_no_tools = OpenAIChatRequest {
                model: &self.model,
                messages: openai_messages,
                stream: true,
                temperature: request.temperature,
                max_tokens: request.max_tokens,
                tools: None,
            };
            self.http
                .post(&url)
                .json(&req_no_tools)
                .send()
                .await?
                .error_for_status()?
        } else {
            response.error_for_status()?
        };
        eprintln!("[DEBUG] llama.cpp response OK, starting stream");

        let (tx, rx) = mpsc::channel(100);
        let mut stream = response.bytes_stream();

        tokio::spawn(async move {
            let mut accumulated_content = String::new();
            let mut final_usage: Option<OpenAIUsage> = None;
            let mut final_timings: Option<LlamaCppTimings> = None;
            let mut tool_calls: std::collections::HashMap<usize, AccumulatedToolCall> =
                std::collections::HashMap::new();

            while let Some(chunk_result) = stream.next().await {
                match chunk_result {
                    Ok(bytes) => {
                        let text = String::from_utf8_lossy(&bytes);
                        for line in text.lines() {
                            let line = line.trim();
                            if line.is_empty() || line == "[DONE]" {
                                continue;
                            }

                            let data_line = line.strip_prefix("data: ").unwrap_or(line);

                            let chunk = match serde_json::from_str::<OpenAIStreamChunk>(data_line) {
                                Ok(c) => c,
                                Err(_) => continue,
                            };

                            if chunk.usage.is_some() {
                                eprintln!("[DEBUG] llama.cpp usage: {:?}", chunk.usage);
                                final_usage = chunk.usage;
                            }
                            if chunk.timings.is_some() {
                                eprintln!("[DEBUG] llama.cpp timings: {:?}", chunk.timings);
                                final_timings = chunk.timings;
                            }

                            // Process delta fields from first choice
                            if let Some(delta) =
                                chunk.choices.first().and_then(|c| c.delta.as_ref())
                            {
                                if let Some(reasoning) =
                                    delta.reasoning_content.as_ref().filter(|r| !r.is_empty())
                                {
                                    let _ = tx.send(StreamChunk::Thinking(reasoning.clone())).await;
                                }
                                if let Some(content) =
                                    delta.content.as_ref().filter(|c| !c.is_empty())
                                {
                                    accumulated_content.push_str(content);
                                    let _ = tx.send(StreamChunk::Delta(content.clone())).await;
                                }
                                // Accumulate tool call deltas
                                if let Some(tcs) = &delta.tool_calls {
                                    for tc in tcs {
                                        let entry = tool_calls.entry(tc.index).or_default();
                                        if let Some(id) = &tc.id {
                                            entry.id = id.clone();
                                        }
                                        if let Some(func) = &tc.function {
                                            if let Some(name) = &func.name {
                                                entry.name = name.clone();
                                            }
                                            if let Some(args) = &func.arguments {
                                                entry.arguments.push_str(args);
                                            }
                                        }
                                    }
                                }
                            }

                            // Check if stream finished
                            if let Some(reason) = chunk
                                .choices
                                .first()
                                .and_then(|c| c.finish_reason.as_ref())
                                .filter(|r| r.as_str() != "null" && !r.is_empty())
                            {
                                eprintln!("[DEBUG] llama.cpp final chunk - finish_reason: {}, usage: {:?}, timings: {:?}",
                                    reason, final_usage, final_timings);

                                // Convert tool calls to XML if present
                                if reason == "tool_calls" && !tool_calls.is_empty() {
                                    let xml = tool_calls_to_xml(&tool_calls);
                                    let _ = tx.send(StreamChunk::Delta(xml.clone())).await;
                                    accumulated_content.push_str(&xml);
                                }

                                let resp = build_llamacpp_response(
                                    &accumulated_content,
                                    reason,
                                    final_usage.as_ref(),
                                    final_timings.as_ref(),
                                );
                                let _ = tx.send(StreamChunk::Done(resp)).await;
                                return;
                            }
                        }
                    }
                    Err(e) => {
                        let _ = tx.send(StreamChunk::Error(e.to_string())).await;
                        return;
                    }
                }
            }

            // Stream ended without explicit finish reason
            let resp = build_llamacpp_response(
                &accumulated_content,
                "stop",
                final_usage.as_ref(),
                final_timings.as_ref(),
            );
            let _ = tx.send(StreamChunk::Done(resp)).await;
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

/// Convert accumulated tool calls to XML format that Studio can parse.
fn tool_calls_to_xml(tool_calls: &std::collections::HashMap<usize, AccumulatedToolCall>) -> String {
    let mut sorted: Vec<_> = tool_calls.iter().collect();
    sorted.sort_by_key(|(idx, _)| *idx);
    let mut xml = String::new();
    for (_, tc) in sorted {
        xml.push_str(&format!(
            "\n<tool_call>\n<tool_name>{}</tool_name>\n<arguments>\n{}\n</arguments>\n</tool_call>",
            tc.name, tc.arguments
        ));
    }
    xml
}

/// Build a final RawChatResponse with optional usage/timings from llama.cpp API stats.
fn build_llamacpp_response(
    content: &str,
    finish_reason_str: &str,
    usage: Option<&OpenAIUsage>,
    timings: Option<&LlamaCppTimings>,
) -> RawChatResponse {
    let reason = match finish_reason_str {
        "stop" | "tool_calls" => FinishReason::Stop,
        "length" => FinishReason::Length,
        _ => FinishReason::Other,
    };
    let mut resp = RawChatResponse::new(content.to_string()).with_finish_reason(reason);

    if let Some(u) = usage {
        resp = resp.with_usage(Usage {
            prompt_tokens: u.prompt_tokens,
            completion_tokens: u.completion_tokens,
            total_tokens: u.total_tokens,
        });
    }
    if let Some(t) = timings {
        resp = resp.with_timings(Timings {
            tokens_per_second: t.predicted_per_second,
            prompt_ms: t.prompt_ms,
            generation_ms: t.predicted_ms,
            total_ms: t.prompt_ms + t.predicted_ms,
        });
    }
    resp
}

#[async_trait]
impl LlmBackend for LlamaCppBackend {
    async fn chat(&self, request: ChatRequest) -> Result<RawChatResponse> {
        let url = format!("{}/chat/completions", self.base_url);

        // Convert ChatRequest to OpenAI format
        let openai_messages: Vec<OpenAIMessage> = request
            .messages
            .iter()
            .map(|msg| OpenAIMessage {
                role: msg.role.clone(),
                content: msg.content.clone(),
            })
            .collect();

        // Pass tools for native tool calling
        let req = OpenAIChatRequest {
            model: &self.model,
            messages: openai_messages,
            stream: false,
            temperature: request.temperature,
            max_tokens: request.max_tokens,
            tools: request.tools,
        };

        let resp: OpenAIChatResponse = self
            .http
            .post(&url)
            .json(&req)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        // Build RawChatResponse from first choice
        let content = resp
            .choices
            .first()
            .map(|c| c.message.content.clone())
            .unwrap_or_default();

        let finish_reason = resp
            .choices
            .first()
            .and_then(|c| c.finish_reason.as_ref())
            .map(|r| match r.as_str() {
                "stop" => FinishReason::Stop,
                "length" => FinishReason::Length,
                _ => FinishReason::Other,
            })
            .unwrap_or(FinishReason::Stop);

        let usage = resp.usage.map(|u| Usage {
            prompt_tokens: u.prompt_tokens,
            completion_tokens: u.completion_tokens,
            total_tokens: u.total_tokens,
        });

        let mut response = RawChatResponse::new(content).with_finish_reason(finish_reason);

        if let Some(u) = usage {
            response = response.with_usage(u);
        }

        Ok(response)
    }

    async fn get_model_info(&self) -> Result<ModelInfo> {
        // Try to get model list from /models endpoint
        let url = format!("{}/models", self.base_url);

        let model_info = match self.http.get(&url).send().await {
            Ok(resp) => match resp.json::<ModelsResponse>().await {
                Ok(models_resp) => {
                    let model_name = models_resp
                        .data
                        .first()
                        .map(|m| m.id.clone())
                        .unwrap_or_else(|| self.model.clone());

                    ModelInfo {
                        name: model_name.clone(),
                        family: Self::detect_model_family(&model_name),
                        context_window: Some(8192), // llama.cpp default
                        supports_native_tools: false,
                        max_output_tokens: None,
                    }
                }
                Err(_) => {
                    // Fallback: just use the model name provided
                    ModelInfo {
                        name: self.model.clone(),
                        family: Self::detect_model_family(&self.model),
                        context_window: Some(8192),
                        supports_native_tools: false,
                        max_output_tokens: None,
                    }
                }
            },
            Err(_) => {
                // Service unavailable, return default
                ModelInfo {
                    name: self.model.clone(),
                    family: Self::detect_model_family(&self.model),
                    context_window: Some(8192),
                    supports_native_tools: false,
                    max_output_tokens: None,
                }
            }
        };

        Ok(model_info)
    }
}

// Note: llama.cpp doesn't provide metrics like Ollama does
#[async_trait]
impl MetricsProvider for LlamaCppBackend {
    async fn get_metrics(&self) -> Result<Value> {
        // llama.cpp doesn't have a metrics endpoint like Ollama
        // Return a placeholder response
        Ok(serde_json::json!({
            "status": "No metrics available for llama.cpp",
            "note": "Use llama.cpp server UI to monitor resource usage"
        }))
    }
}

// OpenAI-compatible types
#[derive(Debug, Clone, Serialize, Deserialize)]
struct OpenAIMessage {
    role: String,
    content: String,
}

#[derive(Serialize)]
struct OpenAIChatRequest<'a> {
    model: &'a str,
    messages: Vec<OpenAIMessage>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    /// Tools for native tool calling (OpenAI format)
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Value>,
}

#[derive(Debug, Serialize, Deserialize)]
struct OpenAIChatResponse {
    choices: Vec<OpenAIChoice>,
    #[serde(default)]
    usage: Option<OpenAIUsage>,
}

#[derive(Debug, Serialize, Deserialize)]
struct OpenAIChoice {
    message: OpenAIMessage,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct OpenAIUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
    total_tokens: u32,
}

/// Streaming response chunk from OpenAI-compatible API
#[derive(Debug, Deserialize)]
struct OpenAIStreamChunk {
    choices: Vec<OpenAIStreamChoice>,
    /// Usage stats (included in final chunk by llama.cpp)
    #[serde(default)]
    usage: Option<OpenAIUsage>,
    /// Timing stats from llama.cpp (tokens/s, etc.)
    #[serde(default)]
    timings: Option<LlamaCppTimings>,
}

#[derive(Debug, Deserialize)]
struct OpenAIStreamChoice {
    delta: Option<OpenAIStreamDelta>,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenAIStreamDelta {
    content: Option<String>,
    /// Reasoning/thinking content (used by GLM and other reasoning models)
    reasoning_content: Option<String>,
    /// Tool calls in OpenAI format (used by GLM, etc.)
    tool_calls: Option<Vec<OpenAIToolCallDelta>>,
}

/// Streaming tool call delta from OpenAI-compatible API
#[derive(Debug, Deserialize)]
struct OpenAIToolCallDelta {
    index: usize,
    id: Option<String>,
    #[serde(rename = "type")]
    call_type: Option<String>,
    function: Option<OpenAIFunctionDelta>,
}

#[derive(Debug, Deserialize)]
struct OpenAIFunctionDelta {
    name: Option<String>,
    arguments: Option<String>,
}

/// Accumulated tool call during streaming
#[derive(Debug, Default, Clone)]
struct AccumulatedToolCall {
    id: String,
    name: String,
    arguments: String,
}

/// llama.cpp specific timing information
#[derive(Debug, Clone, Deserialize)]
pub struct LlamaCppTimings {
    /// Tokens generated per second
    #[serde(default)]
    pub predicted_per_second: f64,
    /// Number of prompt tokens
    #[serde(default)]
    pub prompt_n: u32,
    /// Number of predicted/generated tokens
    #[serde(default)]
    pub predicted_n: u32,
    /// Prompt processing time in ms
    #[serde(default)]
    pub prompt_ms: f64,
    /// Generation time in ms
    #[serde(default)]
    pub predicted_ms: f64,
}

#[derive(Debug, Deserialize)]
struct ModelsResponse {
    data: Vec<ModelData>,
}

#[derive(Debug, Deserialize)]
struct ModelData {
    id: String,
}
