// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2025-2026 naskel.com

//! Context Manager - Sliding window with topic summarization
//!
//! Manages LLM context by compressing old messages into topic summaries
//! when approaching the context limit, similar to Claude Code's approach.

use crate::state::ChatState;
use devit_backend_core::ChatMessage as BackendMessage;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::State;

/// A compressed summary of a conversation segment
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TopicSummary {
    /// Short title describing the topic
    pub title: String,
    /// Compressed summary of the conversation segment
    pub summary: String,
    /// Estimated token count for this summary
    pub token_count: usize,
    /// Number of original messages that were compressed
    pub original_message_count: usize,
    /// Timestamp when this topic was created
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Configuration for context management
#[derive(Clone, Debug)]
pub struct ContextConfig {
    /// Maximum context window size in tokens
    pub max_tokens: usize,
    /// Threshold (0.0-1.0) at which to trigger compression
    pub compression_threshold: f32,
    /// Target ratio of context to keep for recent messages (0.0-1.0)
    pub recent_messages_ratio: f32,
    /// Maximum number of topic summaries to keep
    pub max_topics: usize,
    /// Minimum messages before considering compression
    pub min_messages_for_compression: usize,
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self {
            max_tokens: 8192,                // Default 8k context
            compression_threshold: 0.7,      // Compress at 70% capacity
            recent_messages_ratio: 0.8,      // Keep 80% for recent messages
            max_topics: 10,                  // Max 10 topic summaries
            min_messages_for_compression: 6, // At least 6 messages before compressing
        }
    }
}

impl ContextConfig {
    /// Create config for a specific context size
    pub fn for_context_size(max_tokens: usize) -> Self {
        Self {
            max_tokens,
            ..Default::default()
        }
    }
}

/// Manages the conversation context with automatic compression
#[derive(Clone, Debug)]
pub struct ContextManager {
    /// Configuration
    pub config: ContextConfig,
    /// Topic summaries (oldest first)
    pub topics: Vec<TopicSummary>,
    /// Recent messages (not yet compressed)
    pub recent_messages: Vec<BackendMessage>,
    /// System prompt (always kept)
    pub system_prompt: Option<String>,
    /// Current estimated token count
    current_tokens: usize,
}

impl ContextManager {
    /// Create a new context manager
    pub fn new(config: ContextConfig) -> Self {
        Self {
            config,
            topics: Vec::new(),
            recent_messages: Vec::new(),
            system_prompt: None,
            current_tokens: 0,
        }
    }

    /// Create with default config for a context size
    pub fn with_context_size(max_tokens: usize) -> Self {
        Self::new(ContextConfig::for_context_size(max_tokens))
    }

    /// Set the system prompt
    pub fn set_system_prompt(&mut self, prompt: String) {
        let old_tokens = self
            .system_prompt
            .as_ref()
            .map(|p| estimate_tokens(p))
            .unwrap_or(0);
        let new_tokens = estimate_tokens(&prompt);
        self.current_tokens = self.current_tokens.saturating_sub(old_tokens) + new_tokens;
        self.system_prompt = Some(prompt);
    }

    /// Add a message to the context
    pub fn add_message(&mut self, message: BackendMessage) {
        let tokens = estimate_message_tokens(&message);
        self.current_tokens += tokens;
        self.recent_messages.push(message);
    }

    /// Check if compression is needed
    pub fn needs_compression(&self) -> bool {
        let threshold =
            (self.config.max_tokens as f32 * self.config.compression_threshold) as usize;
        self.current_tokens >= threshold
            && self.recent_messages.len() >= self.config.min_messages_for_compression
    }

    /// Get current token usage ratio
    pub fn usage_ratio(&self) -> f32 {
        self.current_tokens as f32 / self.config.max_tokens as f32
    }

    /// Get messages that should be compressed (older messages)
    /// Returns None if compression is not needed
    pub fn get_messages_to_compress(&self) -> Option<Vec<BackendMessage>> {
        if !self.needs_compression() {
            return None;
        }

        // Keep recent_messages_ratio of context for recent messages
        let target_recent_tokens =
            (self.config.max_tokens as f32 * self.config.recent_messages_ratio) as usize;

        // Find how many messages to keep as recent
        let mut tokens_from_end = 0;
        let mut keep_count = 0;

        for msg in self.recent_messages.iter().rev() {
            let msg_tokens = estimate_message_tokens(msg);
            if tokens_from_end + msg_tokens > target_recent_tokens && keep_count > 0 {
                break;
            }
            tokens_from_end += msg_tokens;
            keep_count += 1;
        }

        // Messages to compress = total - keep_count
        let compress_count = self.recent_messages.len().saturating_sub(keep_count);

        if compress_count < 2 {
            // Not enough messages to compress
            return None;
        }

        Some(self.recent_messages[..compress_count].to_vec())
    }

    /// Apply a topic summary (after LLM generates it)
    pub fn apply_compression(&mut self, summary: TopicSummary, compressed_message_count: usize) {
        // Remove compressed messages
        let old_tokens: usize = self.recent_messages[..compressed_message_count]
            .iter()
            .map(estimate_message_tokens)
            .sum();

        self.recent_messages.drain(..compressed_message_count);

        // Update token count
        self.current_tokens = self.current_tokens.saturating_sub(old_tokens) + summary.token_count;

        // Add topic
        self.topics.push(summary);

        // Enforce max topics limit (FIFO)
        while self.topics.len() > self.config.max_topics {
            if let Some(removed) = self.topics.first() {
                self.current_tokens = self.current_tokens.saturating_sub(removed.token_count);
            }
            self.topics.remove(0);
        }
    }

    /// Build the full message list for LLM (system + topics + recent)
    pub fn build_messages(&self) -> Vec<BackendMessage> {
        let mut messages = Vec::new();

        // System prompt with topic summaries embedded
        if let Some(ref system) = self.system_prompt {
            let mut system_content = system.clone();

            if !self.topics.is_empty() {
                system_content.push_str("\n\n## Previous Conversation Context\n");
                for topic in &self.topics {
                    system_content.push_str(&format!("\n### {}\n{}\n", topic.title, topic.summary));
                }
                system_content.push_str("\n---\n(Recent conversation follows)\n");
            }

            messages.push(BackendMessage {
                role: "system".to_string(),
                content: system_content,
                tool_calls: None,
                tool_name: None,
                images: None,
            });
        }

        // Recent messages
        messages.extend(self.recent_messages.clone());

        messages
    }

    /// Get statistics about current context usage
    pub fn stats(&self) -> ContextStats {
        ContextStats {
            total_tokens: self.current_tokens,
            max_tokens: self.config.max_tokens,
            usage_percent: ((self.usage_ratio() * 100.0).round().min(255.0).max(0.0)) as u8,
            topic_count: self.topics.len(),
            recent_message_count: self.recent_messages.len(),
            needs_compression: self.needs_compression(),
        }
    }

    /// Clear all context (reset)
    pub fn clear(&mut self) {
        self.topics.clear();
        self.recent_messages.clear();
        self.current_tokens = self
            .system_prompt
            .as_ref()
            .map(|p| estimate_tokens(p))
            .unwrap_or(0);
    }
}

/// Statistics about context usage
#[derive(Clone, Debug, Serialize)]
pub struct ContextStats {
    pub total_tokens: usize,
    pub max_tokens: usize,
    pub usage_percent: u8,
    pub topic_count: usize,
    pub recent_message_count: usize,
    pub needs_compression: bool,
}

/// Estimate token count for a string (conservative approximation)
/// Uses ~3 characters per token to be safe (accounts for code, non-English, etc.)
pub fn estimate_tokens(text: &str) -> usize {
    // Conservative estimate: ~3 chars per token
    // This is safer because:
    // - Code and special characters often tokenize to more tokens
    // - Non-English text (French, etc.) uses more tokens
    // - Better to over-estimate and trigger compression earlier than truncate
    let char_count = text.chars().count();
    (char_count + 2) / 3 // Round up division by 3
}

/// Estimate tokens for a message (including role overhead)
pub fn estimate_message_tokens(msg: &BackendMessage) -> usize {
    let content_tokens = estimate_tokens(&msg.content);
    let role_overhead = 4; // ~4 tokens for role formatting
    let image_tokens = msg
        .images
        .as_ref()
        .map(|imgs| imgs.len() * 256)
        .unwrap_or(0); // Rough estimate for images

    content_tokens + role_overhead + image_tokens
}

/// Prompt template for generating topic summaries
pub const SUMMARIZE_PROMPT: &str = r#"Summarize the following conversation segment into a concise topic summary.

Format your response as:
TITLE: [2-5 word topic title]
SUMMARY: [2-4 bullet points capturing key information, decisions, and context needed for future reference]

Focus on:
- Key decisions made
- Important information shared
- Context needed to continue the conversation
- Any unresolved questions or tasks

Conversation to summarize:
"#;

/// Parse a summary response from the LLM
pub fn parse_summary_response(
    response: &str,
    original_message_count: usize,
) -> Option<TopicSummary> {
    let mut title = String::new();
    let mut summary = String::new();
    let mut in_summary = false;

    for line in response.lines() {
        let line = line.trim();
        if line.starts_with("TITLE:") {
            title = line.strip_prefix("TITLE:").unwrap_or("").trim().to_string();
        } else if line.starts_with("SUMMARY:") {
            in_summary = true;
            let rest = line.strip_prefix("SUMMARY:").unwrap_or("").trim();
            if !rest.is_empty() {
                summary.push_str(rest);
                summary.push('\n');
            }
        } else if in_summary && !line.is_empty() {
            summary.push_str(line);
            summary.push('\n');
        }
    }

    if title.is_empty() {
        title = "Conversation segment".to_string();
    }

    if summary.is_empty() {
        // Fallback: use the whole response as summary
        summary = response.trim().to_string();
    }

    let token_count = estimate_tokens(&title) + estimate_tokens(&summary) + 10; // +10 for formatting

    Some(TopicSummary {
        title,
        summary: summary.trim().to_string(),
        token_count,
        original_message_count,
        created_at: chrono::Utc::now(),
    })
}

// ============================================================================
// IPC Commands
// ============================================================================

/// Type alias for managed chat state
pub type ManagedChatState = Arc<RwLock<ChatState>>;

/// Get context statistics
#[tauri::command]
pub async fn get_context_stats(state: State<'_, ManagedChatState>) -> Result<ContextStats, String> {
    let chat = state.read();
    Ok(chat.context_manager.stats())
}

/// Configure context window size
#[tauri::command]
pub async fn configure_context(
    max_tokens: usize,
    state: State<'_, ManagedChatState>,
) -> Result<(), String> {
    let mut chat = state.write();
    chat.context_manager.config.max_tokens = max_tokens;
    Ok(())
}

/// Clear chat context (reset)
#[tauri::command]
pub async fn clear_context(state: State<'_, ManagedChatState>) -> Result<(), String> {
    let mut chat = state.write();
    chat.context_manager.clear();
    Ok(())
}

/// Get current topic summaries
#[tauri::command]
pub async fn get_topics(state: State<'_, ManagedChatState>) -> Result<Vec<TopicSummary>, String> {
    let chat = state.read();
    Ok(chat.context_manager.topics.clone())
}

/// Apply a topic summary (called after LLM generates summary)
#[tauri::command]
pub async fn apply_topic_summary(
    title: String,
    summary: String,
    compressed_count: usize,
    state: State<'_, ManagedChatState>,
) -> Result<ContextStats, String> {
    let mut chat = state.write();

    let topic = TopicSummary {
        title,
        summary: summary.clone(),
        token_count: estimate_tokens(&summary) + 20,
        original_message_count: compressed_count,
        created_at: chrono::Utc::now(),
    };

    chat.context_manager
        .apply_compression(topic, compressed_count);
    Ok(chat.context_manager.stats())
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_estimation() {
        // ~4 chars per token
        assert_eq!(estimate_tokens("hello"), 2); // 5 chars -> 2 tokens
        assert_eq!(estimate_tokens("hello world"), 3); // 11 chars -> 3 tokens
        assert_eq!(estimate_tokens(""), 0);
    }

    #[test]
    fn test_context_manager_basic() {
        let mut cm = ContextManager::with_context_size(1000);
        cm.set_system_prompt("You are a helpful assistant.".to_string());

        assert!(!cm.needs_compression());

        cm.add_message(BackendMessage {
            role: "user".to_string(),
            content: "Hello".to_string(),
            tool_calls: None,
            tool_name: None,
            images: None,
        });

        let messages = cm.build_messages();
        assert_eq!(messages.len(), 2); // system + user
    }

    #[test]
    fn test_parse_summary() {
        let response = "TITLE: Project Setup Discussion\nSUMMARY:\n- Decided to use Rust\n- Created initial structure";
        let summary = parse_summary_response(response, 5).unwrap();

        assert_eq!(summary.title, "Project Setup Discussion");
        assert!(summary.summary.contains("Decided to use Rust"));
        assert_eq!(summary.original_message_count, 5);
    }
}
