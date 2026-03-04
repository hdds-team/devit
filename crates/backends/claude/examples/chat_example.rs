// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2025-2026 naskel.com

//! Quick test: does claude backend stream correctly?
//! Run from a terminal WITHOUT Claude Code active:
//!   cargo run -p devit-claude --example test_chat

use devit_backend_core::{ChatMessage, ChatRequest};
use devit_claude::ClaudeBackend;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter("debug")
        .init();

    println!("=== Claude Backend Test ===");
    println!("Checking availability...");

    if !devit_claude::is_available() {
        eprintln!("ERROR: `claude` not found in PATH");
        std::process::exit(1);
    }
    println!("claude CLI found\n");

    let backend = ClaudeBackend::new("haiku".to_string());

    let request = ChatRequest::new(vec![ChatMessage {
        role: "user".to_string(),
        content: "Say hello in exactly 3 words.".to_string(),
        tool_calls: None,
        tool_name: None,
        images: None,
    }]);

    println!("Sending request...");
    match backend.chat_stream(request).await {
        Ok(mut rx) => {
            println!("Stream opened, waiting for chunks...");
            while let Some(chunk) = rx.recv().await {
                match chunk {
                    devit_claude::StreamChunk::Delta(text) => {
                        print!("{}", text);
                    }
                    devit_claude::StreamChunk::Thinking(text) => {
                        print!("[think: {}]", text);
                    }
                    devit_claude::StreamChunk::Done(resp) => {
                        println!("\n\n=== DONE ===");
                        println!("Content: {}", resp.content);
                        if let Some(usage) = resp.usage {
                            println!(
                                "Usage: {} in / {} out / {} total",
                                usage.prompt_tokens,
                                usage.completion_tokens,
                                usage.total_tokens
                            );
                        }
                        break;
                    }
                    devit_claude::StreamChunk::Error(e) => {
                        eprintln!("\nERROR: {}", e);
                        break;
                    }
                }
            }
        }
        Err(e) => {
            eprintln!("Failed to start stream: {}", e);
            std::process::exit(1);
        }
    }
}
