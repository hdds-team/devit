// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2025-2026 naskel.com

pub mod tool_calling;

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Core trait for LLM backends - NO Clone, use Arc<dyn LlmBackend>
#[async_trait]
pub trait LlmBackend: Send + Sync {
    /// Send a chat request and get a response
    async fn chat(&self, request: ChatRequest) -> Result<RawChatResponse>;

    /// Get model information and capabilities
    async fn get_model_info(&self) -> Result<ModelInfo>;
}

/// Optional trait for backends that provide metrics
#[async_trait]
pub trait MetricsProvider: Send + Sync {
    /// Get current metrics (cache usage, performance, etc.)
    async fn get_metrics(&self) -> Result<Value>;
}

/// Chat request with messages
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatRequest {
    pub messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    /// Optional tools to enable native tool calling (backend-specific format)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Value>,
}

/// Individual chat message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    /// Tool calls (for assistant messages with native tool calling)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<tool_calling::ToolCall>>,
    /// Tool name (for tool result messages)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    /// Images for vision models (base64 encoded, no data: prefix)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub images: Option<Vec<String>>,
}

/// Raw response from backend (before tool calling layer processing)
#[derive(Debug, Clone)]
pub struct RawChatResponse {
    /// The text content of the response
    pub content: String,

    /// Finish reason (if available)
    pub finish_reason: Option<FinishReason>,

    /// Token usage stats (if available)
    pub usage: Option<Usage>,

    /// Performance timing stats (tokens/s, latency, etc.)
    pub timings: Option<Timings>,

    /// Tool calls parsed by backend (if backend supports native tool calling)
    pub tool_calls: Option<Vec<tool_calling::ToolCall>>,

    /// Raw backend-specific data for debugging/advanced use
    pub raw_data: Option<Value>,
}

/// Why the model stopped generating
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    /// Natural stop
    Stop,
    /// Hit token limit
    Length,
    /// Hit content filter
    ContentFilter,
    /// Tool call requested (if backend supports native tool calling)
    ToolCalls,
    /// Other/unknown reason
    Other,
}

/// Token usage statistics
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

/// Performance timing statistics
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Timings {
    /// Tokens generated per second
    pub tokens_per_second: f64,
    /// Prompt processing time in milliseconds
    pub prompt_ms: f64,
    /// Generation time in milliseconds
    pub generation_ms: f64,
    /// Total time in milliseconds
    pub total_ms: f64,
}

/// Model information and capabilities
#[derive(Debug, Clone)]
pub struct ModelInfo {
    /// Model name/ID
    pub name: String,

    /// Model family (qwen, llama3, mistral, etc.) for format detection
    pub family: Option<String>,

    /// Context window size
    pub context_window: Option<u32>,

    /// Does the backend natively support tool calling?
    pub supports_native_tools: bool,

    /// Maximum tokens the model can generate
    pub max_output_tokens: Option<u32>,
}

impl ChatRequest {
    pub fn new(messages: Vec<ChatMessage>) -> Self {
        Self {
            messages,
            temperature: None,
            max_tokens: None,
            tools: None,
        }
    }

    pub fn with_temperature(mut self, temp: f32) -> Self {
        self.temperature = Some(temp);
        self
    }

    pub fn with_max_tokens(mut self, max: u32) -> Self {
        self.max_tokens = Some(max);
        self
    }

    pub fn with_tools(mut self, tools: Value) -> Self {
        self.tools = Some(tools);
        self
    }
}

impl RawChatResponse {
    pub fn new(content: String) -> Self {
        Self {
            content,
            finish_reason: None,
            usage: None,
            timings: None,
            tool_calls: None,
            raw_data: None,
        }
    }

    pub fn with_finish_reason(mut self, reason: FinishReason) -> Self {
        self.finish_reason = Some(reason);
        self
    }

    pub fn with_usage(mut self, usage: Usage) -> Self {
        self.usage = Some(usage);
        self
    }

    pub fn with_timings(mut self, timings: Timings) -> Self {
        self.timings = Some(timings);
        self
    }

    pub fn with_tool_calls(mut self, calls: Vec<tool_calling::ToolCall>) -> Self {
        self.tool_calls = Some(calls);
        self
    }

    pub fn with_raw_data(mut self, data: Value) -> Self {
        self.raw_data = Some(data);
        self
    }
}
