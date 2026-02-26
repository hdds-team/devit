// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2025-2026 naskel.com

//! LMStudio backend - compatible with OpenAI-like API (localhost:1234)
//!
//! LMStudio provides a local chat API compatible with OpenAI format.
//! Default: http://localhost:1234/v1/chat/completions

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

/// LMStudio backend using OpenAI-compatible API
#[derive(Clone)]
pub struct LmstudioBackend {
    base_url: String,
    model: String,
    http: Client,
}

impl LmstudioBackend {
    pub fn new(base_url: String, model: String) -> Self {
        Self {
            base_url,
            model,
            http: Client::new(),
        }
    }

    /// Create with default LMStudio endpoint (localhost:1234)
    pub fn with_default() -> Self {
        Self::new(
            "http://localhost:1234/v1".to_string(),
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

        // Convert ChatRequest to OpenAI format
        let openai_messages: Vec<OpenAIMessage> = request
            .messages
            .iter()
            .map(|msg| OpenAIMessage {
                role: msg.role.clone(),
                content: msg.content.clone(),
            })
            .collect();

        let req = OpenAIChatRequest {
            model: &self.model,
            messages: openai_messages,
            stream: true,
            temperature: request.temperature,
            max_tokens: request.max_tokens,
        };

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
            let mut parser = ThinkingTagParser::new();
            let mut final_usage: Option<OpenAIUsage> = None;
            let mut final_timings: Option<LmstudioTimings> = None;
            let start_time = std::time::Instant::now();

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

                            if let Ok(chunk) = serde_json::from_str::<OpenAIStreamChunk>(data_line)
                            {
                                if let Some(usage) = chunk.usage {
                                    final_usage = Some(usage);
                                }
                                if let Some(timings) = chunk.timings {
                                    final_timings = Some(timings);
                                }

                                // Extract and parse content through thinking tag parser
                                if let Some(content) = chunk
                                    .choices
                                    .first()
                                    .and_then(|c| c.delta.as_ref())
                                    .and_then(|d| d.content.as_ref())
                                    .filter(|c| !c.is_empty())
                                {
                                    accumulated_content.push_str(content);
                                    for seg in parser.feed(content) {
                                        let _ = match seg {
                                            ParsedSegment::Thinking(t) => {
                                                tx.send(StreamChunk::Thinking(t)).await
                                            }
                                            ParsedSegment::Content(c) => {
                                                tx.send(StreamChunk::Delta(c)).await
                                            }
                                        };
                                    }
                                }

                                // Check if stream finished
                                if let Some(reason) = chunk
                                    .choices
                                    .first()
                                    .and_then(|c| c.finish_reason.as_ref())
                                    .filter(|r| r.as_str() != "null" && !r.is_empty())
                                {
                                    // Flush parser buffer
                                    if let Some(seg) = parser.flush() {
                                        let _ = match seg {
                                            ParsedSegment::Thinking(t) => {
                                                tx.send(StreamChunk::Thinking(t)).await
                                            }
                                            ParsedSegment::Content(c) => {
                                                tx.send(StreamChunk::Delta(c)).await
                                            }
                                        };
                                    }
                                    let resp = build_lmstudio_response(
                                        &accumulated_content,
                                        reason,
                                        final_usage.as_ref(),
                                        final_timings.as_ref(),
                                        start_time.elapsed(),
                                    );
                                    let _ = tx.send(StreamChunk::Done(resp)).await;
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

            // Stream ended without explicit finish reason
            if let Some(seg) = parser.flush() {
                let _ = match seg {
                    ParsedSegment::Thinking(t) => tx.send(StreamChunk::Thinking(t)).await,
                    ParsedSegment::Content(c) => tx.send(StreamChunk::Delta(c)).await,
                };
            }
            let resp = build_lmstudio_response(
                &accumulated_content,
                "stop",
                final_usage.as_ref(),
                final_timings.as_ref(),
                start_time.elapsed(),
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

/// Parsed output from the thinking tag parser
enum ParsedSegment {
    Thinking(String),
    Content(String),
}

/// State machine for parsing <think>/<reasoning>/<thought> tags in streaming content.
/// LMStudio (and similar OpenAI-compat servers) embed thinking in the content
/// using XML tags rather than a dedicated API field.
struct ThinkingTagParser {
    in_thinking: bool,
    seen_any_tag: bool,
    buffer: String,
}

const THINKING_PATTERNS: &[(&str, &str, usize, usize)] = &[
    ("<think>", "</think>", 7, 8),
    ("<seed:think>", "</seed:think>", 12, 13),
    ("<reasoning>", "</reasoning>", 11, 12),
    ("<thought>", "</thought>", 9, 10),
];

const MAX_TAG_LEN: usize = 13; // </seed:think>

impl ThinkingTagParser {
    fn new() -> Self {
        Self {
            in_thinking: false,
            seen_any_tag: false,
            buffer: String::new(),
        }
    }

    /// Feed new streaming content, returns segments to emit.
    fn feed(&mut self, content: &str) -> Vec<ParsedSegment> {
        self.buffer.push_str(content);
        let mut out = Vec::new();
        loop {
            if self.in_thinking {
                if let Some((pos, close_len)) = Self::find_close(&self.buffer) {
                    let thinking = self.buffer[..pos].to_string();
                    if !thinking.is_empty() {
                        out.push(ParsedSegment::Thinking(thinking));
                    }
                    self.buffer = self.buffer[pos + close_len..].to_string();
                    self.in_thinking = false;
                    self.seen_any_tag = true;
                } else if self.can_flush_partial() {
                    if let Some(s) = self.flush_safe_prefix() {
                        out.push(ParsedSegment::Thinking(s));
                    }
                    break;
                } else {
                    break;
                }
            } else if let Some((pos, open_len)) = Self::find_open(&self.buffer) {
                if self.seen_any_tag {
                    let regular = self.buffer[..pos].to_string();
                    if !regular.is_empty() {
                        out.push(ParsedSegment::Content(regular));
                    }
                }
                self.buffer = self.buffer[pos + open_len..].to_string();
                self.in_thinking = true;
                self.seen_any_tag = true;
            } else if let Some((pos, close_len)) = Self::find_close(&self.buffer) {
                // Closing tag without opening -- content before it is thinking
                let thinking = self.buffer[..pos].to_string();
                if !thinking.is_empty() {
                    out.push(ParsedSegment::Thinking(thinking));
                }
                self.buffer = self.buffer[pos + close_len..].to_string();
                self.seen_any_tag = true;
            } else if self.seen_any_tag && self.can_flush_partial() {
                if let Some(s) = self.flush_safe_prefix() {
                    out.push(ParsedSegment::Content(s));
                }
                break;
            } else {
                break;
            }
        }
        out
    }

    /// Flush remaining buffer at stream end.
    fn flush(&mut self) -> Option<ParsedSegment> {
        if self.buffer.is_empty() {
            return None;
        }
        let text = std::mem::take(&mut self.buffer);
        if self.in_thinking {
            Some(ParsedSegment::Thinking(text))
        } else {
            Some(ParsedSegment::Content(text))
        }
    }

    fn find_open(buf: &str) -> Option<(usize, usize)> {
        for &(open, _, open_len, _) in THINKING_PATTERNS {
            if let Some(pos) = buf.find(open) {
                return Some((pos, open_len));
            }
        }
        None
    }

    fn find_close(buf: &str) -> Option<(usize, usize)> {
        for &(_, close, _, close_len) in THINKING_PATTERNS {
            if let Some(pos) = buf.find(close) {
                return Some((pos, close_len));
            }
        }
        None
    }

    fn can_flush_partial(&self) -> bool {
        self.buffer.len() > MAX_TAG_LEN && !Self::ends_with_partial_tag(&self.buffer)
    }

    fn flush_safe_prefix(&mut self) -> Option<String> {
        let safe_len =
            Self::floor_char_boundary(&self.buffer, self.buffer.len().saturating_sub(MAX_TAG_LEN));
        if safe_len == 0 {
            return None;
        }
        let out = self.buffer[..safe_len].to_string();
        self.buffer = self.buffer[safe_len..].to_string();
        Some(out)
    }

    fn floor_char_boundary(s: &str, index: usize) -> usize {
        if index >= s.len() {
            s.len()
        } else {
            let mut i = index;
            while i > 0 && !s.is_char_boundary(i) {
                i -= 1;
            }
            i
        }
    }

    fn ends_with_partial_tag(buffer: &str) -> bool {
        const PARTIALS: &[&str] = &[
            "<",
            "<t",
            "<th",
            "<thi",
            "<thin",
            "<think",
            "</",
            "</t",
            "</th",
            "</thi",
            "</thin",
            "</think",
            "<s",
            "<se",
            "<see",
            "<seed",
            "<seed:",
            "<seed:t",
            "<seed:th",
            "<seed:thi",
            "<seed:thin",
            "<seed:think",
            "</s",
            "</se",
            "</see",
            "</seed",
            "</seed:",
            "</seed:t",
            "</seed:th",
            "</seed:thi",
            "</seed:thin",
            "</seed:think",
            "<r",
            "<re",
            "<rea",
            "<reas",
            "<reaso",
            "<reason",
            "<reasoni",
            "<reasonin",
            "<reasoning",
            "</r",
            "</re",
            "</rea",
            "</reas",
            "</reaso",
            "</reason",
            "</reasoni",
            "</reasonin",
            "</reasoning",
        ];
        PARTIALS.iter().any(|p| buffer.ends_with(p))
    }
}

/// Build a final RawChatResponse with optional usage/timings from LMStudio API stats.
fn build_lmstudio_response(
    content: &str,
    finish_reason_str: &str,
    usage: Option<&OpenAIUsage>,
    timings: Option<&LmstudioTimings>,
    elapsed: std::time::Duration,
) -> RawChatResponse {
    let mut resp =
        RawChatResponse::new(content.to_string()).with_finish_reason(match finish_reason_str {
            "stop" => FinishReason::Stop,
            "length" => FinishReason::Length,
            _ => FinishReason::Other,
        });

    if let Some(u) = usage {
        resp = resp.with_usage(Usage {
            prompt_tokens: u.prompt_tokens,
            completion_tokens: u.completion_tokens,
            total_tokens: u.total_tokens,
        });
    }

    let elapsed_secs = elapsed.as_secs_f64();
    let estimated_tokens = (content.len() / 4).max(1) as f64;

    if let Some(t) = timings {
        resp = resp.with_timings(Timings {
            tokens_per_second: t.tokens_per_second,
            prompt_ms: t.prompt_ms,
            generation_ms: t.generation_ms,
            total_ms: elapsed.as_millis() as f64,
        });
    } else if elapsed_secs > 0.0 {
        resp = resp.with_timings(Timings {
            tokens_per_second: estimated_tokens / elapsed_secs,
            prompt_ms: 0.0,
            generation_ms: elapsed.as_millis() as f64,
            total_ms: elapsed.as_millis() as f64,
        });
    }

    resp
}

#[async_trait]
impl LlmBackend for LmstudioBackend {
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

        let req = OpenAIChatRequest {
            model: &self.model,
            messages: openai_messages,
            stream: false,
            temperature: request.temperature,
            max_tokens: request.max_tokens,
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
                        context_window: Some(8192), // LMStudio default
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

// Note: LMStudio doesn't provide metrics like Ollama does
#[async_trait]
impl MetricsProvider for LmstudioBackend {
    async fn get_metrics(&self) -> Result<Value> {
        // LMStudio doesn't have a metrics endpoint like Ollama
        // Return a placeholder response
        Ok(serde_json::json!({
            "status": "No metrics available for LMStudio",
            "note": "Use LMStudio UI to monitor resource usage"
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
    /// Usage stats (included in final chunk by some providers like LMStudio)
    #[serde(default)]
    usage: Option<OpenAIUsage>,
    /// Timing stats (LMStudio may include this)
    #[serde(default)]
    timings: Option<LmstudioTimings>,
}

/// LMStudio timing information (if available)
#[derive(Debug, Clone, Deserialize)]
struct LmstudioTimings {
    /// Tokens generated per second
    #[serde(default, alias = "predicted_per_second", alias = "tokens_per_second")]
    tokens_per_second: f64,
    /// Prompt processing time in ms
    #[serde(default, alias = "prompt_ms")]
    prompt_ms: f64,
    /// Generation time in ms
    #[serde(default, alias = "predicted_ms", alias = "generation_ms")]
    generation_ms: f64,
}

#[derive(Debug, Deserialize)]
struct OpenAIStreamChoice {
    delta: Option<OpenAIStreamDelta>,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenAIStreamDelta {
    content: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ModelsResponse {
    data: Vec<ModelData>,
}

#[derive(Debug, Deserialize)]
struct ModelData {
    id: String,
}
