// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2025-2026 naskel.com

//! AIRCP Unified Tool - Single entry point for all AIRCP operations.
//!
//! Commands:
//! - send, history, join, status (core)
//! - claim, lock, presence (daemon)
//! - task/list, task/create, task/activity, task/complete
//! - brainstorm/create, brainstorm/vote, brainstorm/status, brainstorm/list
//! - mode/status, mode/set, mode/history, ask, stop, handover
//! - workflow/status, workflow/config, workflow/start, workflow/next, workflow/extend, workflow/skip, workflow/abort, workflow/history
//! - memory/search, memory/get, memory/stats

use async_trait::async_trait;
use mcp_core::{McpResult, McpTool};
use serde_json::{json, Value};
use std::process::Command;

use crate::errors::{internal_error, missing_param, validation_error};

const DEFAULT_AIRCP_CLI: &str = "aircp_cli.py";
const DEFAULT_HDDS_LIB_PATH: &str = "";
const DEFAULT_AIRCP_DAEMON_URL: &str = "http://localhost:5555";

fn aircp_cli() -> String {
    std::env::var("DEVIT_AIRCP_CLI").unwrap_or_else(|_| DEFAULT_AIRCP_CLI.to_string())
}

fn hdds_lib_path() -> String {
    std::env::var("DEVIT_HDDS_LIB_PATH").unwrap_or_else(|_| DEFAULT_HDDS_LIB_PATH.to_string())
}

fn aircp_daemon_url() -> String {
    std::env::var("DEVIT_AIRCP_DAEMON_URL").unwrap_or_else(|_| DEFAULT_AIRCP_DAEMON_URL.to_string())
}

// ============================================================================
// CLI helpers (for HDDS-based commands)
// ============================================================================

fn run_aircp_cli(args: &[&str], agent_id: &str) -> McpResult<Value> {
    let cli = aircp_cli();
    let lib_path = hdds_lib_path();
    let output = Command::new("python3")
        .arg(&cli)
        .arg("--agent-id")
        .arg(agent_id)
        .args(args)
        .env("HDDS_LIB_PATH", &lib_path)
        .env("LD_LIBRARY_PATH", &lib_path)
        .env("HDDS_REUSEPORT", "1")
        .output()
        .map_err(|e| internal_error(format!("Failed to run AIRCP CLI: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(internal_error(format!("AIRCP CLI failed: {}", stderr)));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(&stdout)
        .map_err(|e| internal_error(format!("Invalid JSON from AIRCP CLI: {} - {}", e, stdout)))
}

// ============================================================================
// Encoding helpers
// ============================================================================

fn encode_param(s: &str) -> String {
    s.replace('%', "%25")
        .replace(' ', "%20")
        .replace('#', "%23")
        .replace('&', "%26")
        .replace('=', "%3D")
        .replace('+', "%2B")
        .replace('@', "%40")
}

// ============================================================================
// HTTP helpers (for daemon-based commands)
// ============================================================================

async fn call_daemon(method: &str, endpoint: &str, body: Option<Value>) -> McpResult<Value> {
    let client = reqwest::Client::new();
    let url = format!("{}{}", aircp_daemon_url(), endpoint);

    let response = match method {
        "GET" => client.get(&url).send().await,
        "POST" => {
            let req = client.post(&url).header("Content-Type", "application/json");
            if let Some(b) = body {
                req.json(&b).send().await
            } else {
                req.send().await
            }
        }
        _ => return Err(internal_error("Invalid HTTP method")),
    };

    let resp = response.map_err(|e| internal_error(format!("Daemon request failed: {}", e)))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(internal_error(format!("Daemon error {}: {}", status, text)));
    }

    resp.json()
        .await
        .map_err(|e| internal_error(format!("Invalid JSON from daemon: {}", e)))
}

// ============================================================================
// Unified AIRCP Tool
// ============================================================================

pub struct AircpTool;

impl AircpTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for AircpTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl McpTool for AircpTool {
    fn name(&self) -> &str {
        "devit_aircp"
    }

    fn description(&self) -> &str {
        "AIRCP multi-agent coordination. Commands: send, history, status, join, claim, lock, presence, task/*, brainstorm/*, mode/*, workflow/*, review/*, memory/*"
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "Command to execute. Use 'help' for full documentation. Options: help, send, history, status, join, claim, lock, presence, task/list, task/create, task/activity, task/complete, brainstorm/create, brainstorm/vote, brainstorm/status, brainstorm/list, mode/status, mode/set, mode/history, ask, stop, handover, workflow/status, workflow/config, workflow/start, workflow/next, workflow/extend, workflow/skip, workflow/abort, workflow/history, review/request, review/approve, review/comment, review/changes, review/status, review/list, review/history, memory/search, memory/get, memory/stats"
                },
                // Common params
                "agent_id": { "type": "string", "description": "Your agent ID (default: from DEVIT_IDENT)" },
                "room": { "type": "string", "description": "Room name for send/history/join (e.g., '#general')" },
                "message": { "type": "string", "description": "Message content for send" },
                "limit": { "type": "integer", "description": "Limit for history/list commands" },
                // Claim/Lock params
                "action": { "type": "string", "description": "Action for claim/lock: request, release, query" },
                "resource": { "type": "string", "description": "Resource identifier for claim" },
                "path": { "type": "string", "description": "File path for lock" },
                "capabilities": { "type": "array", "items": { "type": "string" }, "description": "Capabilities for claim" },
                "ttl_minutes": { "type": "integer", "description": "TTL in minutes for claim/lock" },
                "mode": { "type": "string", "description": "Lock mode: read or write" },
                // Task params
                "task_id": { "type": "integer", "description": "Task ID for task commands" },
                "agent": { "type": "string", "description": "Agent filter for task/list" },
                "task_status": { "type": "string", "description": "Status filter for task/list" },
                "description": { "type": "string", "description": "Description for task/create" },
                "priority": { "type": "string", "description": "Priority for task/create" },
                "progress": { "type": "string", "description": "Progress for task/activity" },
                "result": { "type": "string", "description": "Result for task/complete" },
                // Brainstorm params
                "session_id": { "type": "integer", "description": "Session ID for brainstorm commands" },
                "topic": { "type": "string", "description": "Topic for brainstorm/create" },
                "vote": { "type": "string", "description": "Vote for brainstorm/vote (✅ or ❌)" },
                "comment": { "type": "string", "description": "Comment for brainstorm/vote" },
                // Mode params
                "new_mode": { "type": "string", "description": "Mode for mode/set: neutral, focus, review, build" },
                "lead": { "type": "string", "description": "Lead agent for mode/set or workflow/start" },
                "to": { "type": "string", "description": "Target agent for ask/handover" },
                "question": { "type": "string", "description": "Question for ask" },
                // Workflow params
                "feature": { "type": "string", "description": "Feature name for workflow/start" },
                "phase": { "type": "string", "description": "Target phase for workflow/skip" },
                "minutes": { "type": "integer", "description": "Minutes for workflow/extend" },
                "reason": { "type": "string", "description": "Reason for workflow/abort" },
                // Review params
                "file": { "type": "string", "description": "File path for review/request" },
                "reviewers": { "type": "array", "items": { "type": "string" }, "description": "Reviewers list for review/request (e.g., ['@beta', '@sonnet'])" },
                "type": { "type": "string", "description": "Review type: 'code' (needs 2 approvals) or 'doc' (needs 1 approval)" },
                "request_id": { "type": "integer", "description": "Review request ID for review commands" },
                "status": { "type": "string", "description": "Status filter for review/list" },
                // Memory params
                "query": { "type": "string", "description": "Search query for memory/search (required)" },
                "id": { "type": "string", "description": "Message ID for memory/get" },
                "day": { "type": "string", "description": "Date filter YYYY-MM-DD for memory/get or memory/search" },
                "hour": { "type": "integer", "description": "Hour filter 0-23 for memory/get", "minimum": 0, "maximum": 23 }
            },
            "required": ["command"],
            "additionalProperties": false
        })
    }

    async fn execute(&self, params: Value) -> McpResult<Value> {
        let command = params
            .get("command")
            .and_then(Value::as_str)
            .ok_or_else(|| missing_param("command", "string"))?;

        let agent_id = params
            .get("agent_id")
            .and_then(Value::as_str)
            .or_else(|| std::env::var("DEVIT_IDENT").ok().as_deref().map(|_| ""))
            .unwrap_or("@claude-desktop");

        // Get agent_id properly
        let agent_id = params
            .get("agent_id")
            .and_then(Value::as_str)
            .map(String::from)
            .or_else(|| std::env::var("DEVIT_IDENT").ok())
            .unwrap_or_else(|| "@claude-desktop".to_string());

        match command {
            // ========== Help ==========
            "help" => cmd_help().await,

            // ========== Core commands (CLI-based) ==========
            "status" => cmd_status(&agent_id).await,
            "send" => cmd_send(&params, &agent_id).await,
            "history" => cmd_history(&params, &agent_id).await,
            "join" => cmd_join(&params, &agent_id).await,

            // ========== Daemon commands ==========
            "claim" => cmd_claim(&params, &agent_id).await,
            "lock" => cmd_lock(&params, &agent_id).await,
            "presence" => cmd_presence(&params, &agent_id).await,

            // ========== Task commands ==========
            "task/list" => cmd_task_list(&params).await,
            "task/create" => cmd_task_create(&params, &agent_id).await,
            "task/activity" => cmd_task_activity(&params).await,
            "task/complete" => cmd_task_complete(&params).await,

            // ========== Brainstorm commands ==========
            "brainstorm/create" => cmd_brainstorm_create(&params, &agent_id).await,
            "brainstorm/vote" => cmd_brainstorm_vote(&params, &agent_id).await,
            "brainstorm/status" => cmd_brainstorm_status(&params).await,
            "brainstorm/list" => cmd_brainstorm_list(&params).await,

            // ========== Mode commands ==========
            "mode/status" => cmd_mode_status().await,
            "mode/set" => cmd_mode_set(&params, &agent_id).await,
            "mode/history" => cmd_mode_history(&params).await,
            "ask" => cmd_ask(&params, &agent_id).await,
            "stop" => cmd_stop().await,
            "handover" => cmd_handover(&params).await,

            // ========== Workflow commands ==========
            "workflow/status" => cmd_workflow_status().await,
            "workflow/config" => cmd_workflow_config().await,
            "workflow/start" => cmd_workflow_start(&params).await,
            "workflow/next" => cmd_workflow_next().await,
            "workflow/extend" => cmd_workflow_extend(&params).await,
            "workflow/skip" => cmd_workflow_skip(&params).await,
            "workflow/abort" => cmd_workflow_abort(&params).await,
            "workflow/history" => cmd_workflow_history(&params).await,

            // ========== Review commands ==========
            "review/request" => cmd_review_request(&params, &agent_id).await,
            "review/approve" => cmd_review_approve(&params, &agent_id).await,
            "review/comment" => cmd_review_comment(&params, &agent_id).await,
            "review/changes" => cmd_review_changes(&params, &agent_id).await,
            "review/status" => cmd_review_status(&params).await,
            "review/list" => cmd_review_list(&params).await,
            "review/history" => cmd_review_history(&params).await,

            // ========== Memory commands ==========
            "memory/search" => cmd_memory_search(&params).await,
            "memory/get" => cmd_memory_get(&params).await,
            "memory/stats" => cmd_memory_stats().await,

            _ => Err(internal_error(format!("Unknown command: {}", command))),
        }
    }
}

// ============================================================================
// Help command
// ============================================================================

async fn cmd_help() -> McpResult<Value> {
    let help_text = r##"# AIRCP - Multi-Agent Coordination Tool

## 🔑 Basics
- **Channel principal:** `#general`
- **Tags:** `@all` (tous), `@agent` (ciblé) - REQUIS pour obtenir une réponse!
- **Agents:** @alpha (lead dev), @sonnet (analyse), @beta (QA), @haiku (triage), @mascotte (fun)

## 📨 Communication
| Commande | Usage |
|----------|-------|
| `send` | `command="send" room="#general" message="@all Hello!"` |
| `history` | `command="history" room="#general" limit=10` |
| `status` | `command="status"` — check connexion |

## 📋 Tasks
| Commande | Usage |
|----------|-------|
| `task/list` | `command="task/list"` ou `agent="@alpha"` |
| `task/create` | `command="task/create" description="Fix bug" agent="@alpha"` |
| `task/activity` | `command="task/activity" task_id=1 progress="50%"` |
| `task/complete` | `command="task/complete" task_id=1` |

## 🧠 Brainstorm (évite le spam!)
| Commande | Usage |
|----------|-------|
| `brainstorm/create` | `command="brainstorm/create" topic="Quelle archi?"` |
| `brainstorm/vote` | `command="brainstorm/vote" session_id=1 vote="✅"` |
| `brainstorm/status` | `command="brainstorm/status" session_id=1` |

## 🔄 Workflow
| Commande | Usage |
|----------|-------|
| `workflow/status` | Status du workflow actif |
| `workflow/start` | `command="workflow/start" feature="Dark mode" lead="@alpha"` |
| `workflow/next` | Passer à la phase suivante |
| `workflow/abort` | `command="workflow/abort" reason="Annulé"` |

## ⚙️ Modes
| Commande | Usage |
|----------|-------|
| `mode/status` | Mode actuel (neutral/focus/review/build) |
| `mode/set` | `command="mode/set" new_mode="focus" lead="@alpha"` |

## 🔍 Code Review
| Commande | Usage |
|----------|-------|
| `review/request` | `command="review/request" file="src/main.rs" reviewers=["@beta"] type="code"` |
| `review/approve` | `command="review/approve" request_id=1 comment="LGTM!"` |
| `review/comment` | `command="review/comment" request_id=1 comment="Typo ligne 42"` |
| `review/changes` | `command="review/changes" request_id=1 comment="Manque section X"` (bloquant) |
| `review/status` | `command="review/status" request_id=1` |
| `review/list` | `command="review/list"` ou `status="pending"` |

**Règles:** Doc=1 approval, Code=2 approvals. Timeout: 30min→rappel, 1h→auto-close.

## 🧠 Memory
| Commande | Usage |
|----------|-------|
| `memory/search` | `command="memory/search" query="bug fix" room="#general" day="2026-02-07"` |
| `memory/get` | `command="memory/get" id="msg-123"` ou `day="2026-02-07" hour=14 room="#general"` |
| `memory/stats` | `command="memory/stats"` — stats globales mémoire |

## 💡 Tips
1. **Toujours @tagger** pour obtenir une réponse: `@all question?` ou `@alpha peux-tu...`
2. **Utiliser brainstorm** au lieu de flood #general avec des idées
3. **history** montre les derniers messages avec contenu
"##;
    Ok(json!({ "content": [{ "type": "text", "text": help_text }] }))
}

// ============================================================================
// Core commands
// ============================================================================

async fn cmd_status(agent_id: &str) -> McpResult<Value> {
    let result = run_aircp_cli(&["status"], agent_id)?;
    let status = result
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let text = if status == "ok" {
        "✅ AIRCP network connected."
    } else {
        "❌ AIRCP network not available."
    };
    Ok(json!({ "content": [{ "type": "text", "text": text }], "structuredContent": result }))
}

async fn cmd_send(params: &Value, agent_id: &str) -> McpResult<Value> {
    let room = params
        .get("room")
        .and_then(Value::as_str)
        .ok_or_else(|| missing_param("room", "string"))?;
    let message = params
        .get("message")
        .and_then(Value::as_str)
        .ok_or_else(|| missing_param("message", "string"))?;

    // CLI: send <room> <message> (positional args)
    let result = run_aircp_cli(&["send", room, message], agent_id)?;
    let success = result
        .get("success")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let text = if success {
        format!("✅ Message sent to {}", room)
    } else {
        "❌ Failed to send".to_string()
    };
    Ok(json!({ "content": [{ "type": "text", "text": text }], "structuredContent": result }))
}

async fn cmd_history(params: &Value, agent_id: &str) -> McpResult<Value> {
    let room = params
        .get("room")
        .and_then(Value::as_str)
        .ok_or_else(|| missing_param("room", "string"))?;
    let limit = params.get("limit").and_then(Value::as_u64).unwrap_or(20);

    // CLI: history <room> --limit N (room is positional)
    let result = run_aircp_cli(&["history", room, "--limit", &limit.to_string()], agent_id)?;
    let count = result.get("count").and_then(Value::as_u64).unwrap_or(0);

    // Build text with actual messages for better visibility
    let mut text = format!("📜 **{}** - {} messages:\n\n", room, count);
    if let Some(messages) = result.get("messages").and_then(Value::as_array) {
        for msg in messages.iter().rev().take(20) {
            // Show oldest first, limit 20
            let from = msg.get("from").and_then(Value::as_str).unwrap_or("?");
            let content = msg.get("content").and_then(Value::as_str).unwrap_or("");
            // Truncate long messages
            let content_short = if content.len() > 200 {
                format!("{}...", &content[..200])
            } else {
                content.to_string()
            };
            text.push_str(&format!("**[{}]**: {}\n\n", from, content_short));
        }
    }
    Ok(json!({ "content": [{ "type": "text", "text": text }], "structuredContent": result }))
}

async fn cmd_join(params: &Value, agent_id: &str) -> McpResult<Value> {
    let room = params
        .get("room")
        .and_then(Value::as_str)
        .ok_or_else(|| missing_param("room", "string"))?;

    // CLI: join <room> (positional)
    let result = run_aircp_cli(&["join", room], agent_id)?;
    let text = format!("✅ Joined {}", room);
    Ok(json!({ "content": [{ "type": "text", "text": text }], "structuredContent": result }))
}

// ============================================================================
// Daemon commands
// ============================================================================

async fn cmd_claim(params: &Value, agent_id: &str) -> McpResult<Value> {
    let action = params
        .get("action")
        .and_then(Value::as_str)
        .ok_or_else(|| missing_param("action", "string"))?;

    if action == "query" && params.get("resource").is_none() {
        let result = call_daemon("GET", "/claims", None).await?;
        return Ok(
            json!({ "content": [{ "type": "text", "text": "Claims retrieved" }], "structuredContent": result }),
        );
    }

    let resource = params
        .get("resource")
        .and_then(Value::as_str)
        .ok_or_else(|| missing_param("resource", "string"))?;

    let endpoint = format!("/claim/{}", resource);
    let body = json!({
        "action": action,
        "agent_id": agent_id,
        "capabilities": params.get("capabilities"),
        "description": params.get("description"),
        "ttl_minutes": params.get("ttl_minutes")
    });

    let result = call_daemon("POST", &endpoint, Some(body)).await?;
    let text = format!("Claim {}: {}", action, resource);
    Ok(json!({ "content": [{ "type": "text", "text": text }], "structuredContent": result }))
}

async fn cmd_lock(params: &Value, agent_id: &str) -> McpResult<Value> {
    let action = params
        .get("action")
        .and_then(Value::as_str)
        .ok_or_else(|| missing_param("action", "string"))?;

    if action == "query" && params.get("path").is_none() {
        let result = call_daemon("GET", "/locks", None).await?;
        return Ok(
            json!({ "content": [{ "type": "text", "text": "Locks retrieved" }], "structuredContent": result }),
        );
    }

    let body = json!({
        "action": action,
        "agent_id": agent_id,
        "path": params.get("path"),
        "mode": params.get("mode").and_then(Value::as_str).unwrap_or("write"),
        "ttl_minutes": params.get("ttl_minutes")
    });

    let result = call_daemon("POST", "/lock", Some(body)).await?;
    let text = format!("Lock {}", action);
    Ok(json!({ "content": [{ "type": "text", "text": text }], "structuredContent": result }))
}

async fn cmd_presence(params: &Value, agent_id: &str) -> McpResult<Value> {
    let action = params
        .get("action")
        .and_then(Value::as_str)
        .unwrap_or("query");

    if action == "query" {
        let result = call_daemon("GET", "/presence", None).await?;
        return Ok(
            json!({ "content": [{ "type": "text", "text": "Presence retrieved" }], "structuredContent": result }),
        );
    }

    let body = json!({
        "agent_id": agent_id,
        "status": params.get("status").and_then(Value::as_str).unwrap_or("working"),
        "current_task": params.get("current_task")
    });

    let result = call_daemon("POST", "/agent/heartbeat", Some(body)).await?;
    Ok(
        json!({ "content": [{ "type": "text", "text": "Heartbeat sent" }], "structuredContent": result }),
    )
}

// ============================================================================
// Task commands
// ============================================================================

async fn cmd_task_list(params: &Value) -> McpResult<Value> {
    let mut endpoint = "/tasks".to_string();
    let mut query_parts = vec![];

    if let Some(agent) = params.get("agent").and_then(Value::as_str) {
        query_parts.push(format!("agent={}", agent));
    }
    if let Some(status) = params.get("task_status").and_then(Value::as_str) {
        query_parts.push(format!("status={}", status));
    }
    if !query_parts.is_empty() {
        endpoint = format!("{}?{}", endpoint, query_parts.join("&"));
    }

    let result = call_daemon("GET", &endpoint, None).await?;
    let count = result
        .get("tasks")
        .and_then(Value::as_array)
        .map(|a| a.len())
        .unwrap_or(0);
    let text = format!("📋 {} task(s)", count);
    Ok(json!({ "content": [{ "type": "text", "text": text }], "structuredContent": result }))
}

async fn cmd_task_create(params: &Value, agent_id: &str) -> McpResult<Value> {
    let description = params
        .get("description")
        .and_then(Value::as_str)
        .ok_or_else(|| missing_param("description", "string"))?;

    let body = json!({
        "description": description,
        "agent_id": params.get("agent").and_then(Value::as_str).unwrap_or(agent_id),
        "priority": params.get("priority").and_then(Value::as_str).unwrap_or("normal"),
        "created_by": agent_id
    });

    let result = call_daemon("POST", "/task", Some(body)).await?;
    let task_id = result.get("task_id").and_then(Value::as_u64).unwrap_or(0);
    let text = format!("✅ Task #{} created", task_id);
    Ok(json!({ "content": [{ "type": "text", "text": text }], "structuredContent": result }))
}

async fn cmd_task_activity(params: &Value) -> McpResult<Value> {
    let task_id = params
        .get("task_id")
        .and_then(Value::as_u64)
        .ok_or_else(|| missing_param("task_id", "integer"))?;

    let body = json!({
        "task_id": task_id,
        "progress": params.get("progress"),
        "status": params.get("status")
    });

    let result = call_daemon("POST", "/task/activity", Some(body)).await?;
    let text = format!("✅ Activity reported for task #{}", task_id);
    Ok(json!({ "content": [{ "type": "text", "text": text }], "structuredContent": result }))
}

async fn cmd_task_complete(params: &Value) -> McpResult<Value> {
    let task_id = params
        .get("task_id")
        .and_then(Value::as_u64)
        .ok_or_else(|| missing_param("task_id", "integer"))?;

    let body = json!({
        "task_id": task_id,
        "status": params.get("task_status").and_then(Value::as_str).unwrap_or("done"),
        "result": params.get("result")
    });

    let result = call_daemon("POST", "/task/complete", Some(body)).await?;
    let text = format!("✅ Task #{} completed", task_id);
    Ok(json!({ "content": [{ "type": "text", "text": text }], "structuredContent": result }))
}

// ============================================================================
// Brainstorm commands
// ============================================================================

async fn cmd_brainstorm_create(params: &Value, agent_id: &str) -> McpResult<Value> {
    let topic = params
        .get("topic")
        .and_then(Value::as_str)
        .ok_or_else(|| missing_param("topic", "string"))?;

    let body = json!({
        "topic": topic,
        "created_by": agent_id,
        "participants": params.get("participants")
    });

    let result = call_daemon("POST", "/brainstorm/create", Some(body)).await?;
    let session_id = result
        .get("session_id")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let text = format!("🧠 Brainstorm #{} created: {}", session_id, topic);
    Ok(json!({ "content": [{ "type": "text", "text": text }], "structuredContent": result }))
}

async fn cmd_brainstorm_vote(params: &Value, agent_id: &str) -> McpResult<Value> {
    let session_id = params
        .get("session_id")
        .and_then(Value::as_u64)
        .ok_or_else(|| missing_param("session_id", "integer"))?;
    let vote = params
        .get("vote")
        .and_then(Value::as_str)
        .ok_or_else(|| missing_param("vote", "string"))?;

    let body = json!({
        "session_id": session_id,
        "agent_id": agent_id,
        "vote": vote,
        "comment": params.get("comment")
    });

    let result = call_daemon("POST", "/brainstorm/vote", Some(body)).await?;
    let text = format!("✅ Vote {} recorded on brainstorm #{}", vote, session_id);
    Ok(json!({ "content": [{ "type": "text", "text": text }], "structuredContent": result }))
}

async fn cmd_brainstorm_status(params: &Value) -> McpResult<Value> {
    let session_id = params
        .get("session_id")
        .and_then(Value::as_u64)
        .ok_or_else(|| missing_param("session_id", "integer"))?;

    let endpoint = format!("/brainstorm/{}", session_id);
    let result = call_daemon("GET", &endpoint, None).await?;
    Ok(
        json!({ "content": [{ "type": "text", "text": format!("Brainstorm #{} status", session_id) }], "structuredContent": result }),
    )
}

async fn cmd_brainstorm_list(params: &Value) -> McpResult<Value> {
    let active_only = params
        .get("active_only")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let endpoint = if active_only {
        "/brainstorm/active"
    } else {
        "/brainstorm/history"
    };
    let result = call_daemon("GET", endpoint, None).await?;
    let count = result
        .get("sessions")
        .and_then(Value::as_array)
        .map(|a| a.len())
        .unwrap_or(0);
    let text = format!("🧠 {} brainstorm session(s)", count);
    Ok(json!({ "content": [{ "type": "text", "text": text }], "structuredContent": result }))
}

// ============================================================================
// Mode commands
// ============================================================================

async fn cmd_mode_status() -> McpResult<Value> {
    let result = call_daemon("GET", "/mode", None).await?;
    let mode = result
        .get("mode")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let lead = result.get("lead").and_then(Value::as_str).unwrap_or("none");
    let text = format!("🎯 Mode: {} | Lead: {}", mode, lead);
    Ok(json!({ "content": [{ "type": "text", "text": text }], "structuredContent": result }))
}

async fn cmd_mode_set(params: &Value, agent_id: &str) -> McpResult<Value> {
    let mode = params
        .get("new_mode")
        .and_then(Value::as_str)
        .ok_or_else(|| missing_param("new_mode", "string"))?;

    let body = json!({
        "mode": mode,
        "lead": params.get("lead").and_then(Value::as_str).unwrap_or(agent_id),
        "timeout_minutes": params.get("timeout_minutes")
    });

    let result = call_daemon("POST", "/mode", Some(body)).await?;
    let text = format!("✅ Mode set to: {}", mode);
    Ok(json!({ "content": [{ "type": "text", "text": text }], "structuredContent": result }))
}

async fn cmd_mode_history(params: &Value) -> McpResult<Value> {
    let limit = params.get("limit").and_then(Value::as_u64).unwrap_or(10);
    let endpoint = format!("/mode/history?limit={}", limit);
    let result = call_daemon("GET", &endpoint, None).await?;
    Ok(
        json!({ "content": [{ "type": "text", "text": "Mode history retrieved" }], "structuredContent": result }),
    )
}

async fn cmd_ask(params: &Value, agent_id: &str) -> McpResult<Value> {
    let to = params
        .get("to")
        .and_then(Value::as_str)
        .ok_or_else(|| missing_param("to", "string"))?;
    let question = params
        .get("question")
        .and_then(Value::as_str)
        .ok_or_else(|| missing_param("question", "string"))?;

    let body = json!({
        "to": to,
        "question": question,
        "from": agent_id
    });

    let result = call_daemon("POST", "/ask", Some(body)).await?;
    let text = format!("❓ Asked {}: {}", to, question);
    Ok(json!({ "content": [{ "type": "text", "text": text }], "structuredContent": result }))
}

async fn cmd_stop() -> McpResult<Value> {
    let result = call_daemon("POST", "/stop", None).await?;
    Ok(
        json!({ "content": [{ "type": "text", "text": "🛑 Emergency stop executed" }], "structuredContent": result }),
    )
}

async fn cmd_handover(params: &Value) -> McpResult<Value> {
    let to = params
        .get("to")
        .and_then(Value::as_str)
        .ok_or_else(|| missing_param("to", "string"))?;

    let body = json!({ "to": to });
    let result = call_daemon("POST", "/handover", Some(body)).await?;
    let text = format!("🤝 Leadership transferred to {}", to);
    Ok(json!({ "content": [{ "type": "text", "text": text }], "structuredContent": result }))
}

// ============================================================================
// Workflow commands
// ============================================================================

async fn cmd_workflow_status() -> McpResult<Value> {
    let result = call_daemon("GET", "/workflow", None).await?;
    let active = result
        .get("active")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let text = if active {
        let name = result
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let phase = result
            .get("phase")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        format!("🚀 Workflow actif: {} (phase: {})", name, phase)
    } else {
        "Aucun workflow actif".to_string()
    };
    Ok(json!({ "content": [{ "type": "text", "text": text }], "structuredContent": result }))
}

async fn cmd_workflow_config() -> McpResult<Value> {
    let result = call_daemon("GET", "/workflow/config", None).await?;
    Ok(
        json!({ "content": [{ "type": "text", "text": "Workflow config retrieved" }], "structuredContent": result }),
    )
}

async fn cmd_workflow_start(params: &Value) -> McpResult<Value> {
    let feature = params
        .get("feature")
        .and_then(Value::as_str)
        .ok_or_else(|| missing_param("feature", "string"))?;

    let body = json!({
        "name": feature,  // daemon expects "name", not "feature"
        "lead": params.get("lead").and_then(Value::as_str).unwrap_or("me")
    });

    let result = call_daemon("POST", "/workflow/start", Some(body)).await?;
    let workflow_id = result
        .get("workflow_id")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let text = format!("🚀 Workflow #{} démarré: {}", workflow_id, feature);
    Ok(json!({ "content": [{ "type": "text", "text": text }], "structuredContent": result }))
}

async fn cmd_workflow_next() -> McpResult<Value> {
    let result = call_daemon("POST", "/workflow/next", None).await?;
    let from = result.get("from").and_then(Value::as_str).unwrap_or("?");
    let to = result.get("to").and_then(Value::as_str).unwrap_or("?");
    let text = format!("➡️ Phase: {} → {}", from, to);
    Ok(json!({ "content": [{ "type": "text", "text": text }], "structuredContent": result }))
}

async fn cmd_workflow_extend(params: &Value) -> McpResult<Value> {
    let minutes = params.get("minutes").and_then(Value::as_u64).unwrap_or(10);
    let body = json!({ "minutes": minutes });
    let result = call_daemon("POST", "/workflow/extend", Some(body)).await?;
    let total = result
        .get("total_minutes")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let text = format!("⏰ Phase étendue de {}min (total: {}min)", minutes, total);
    Ok(json!({ "content": [{ "type": "text", "text": text }], "structuredContent": result }))
}

async fn cmd_workflow_skip(params: &Value) -> McpResult<Value> {
    let phase = params
        .get("phase")
        .and_then(Value::as_str)
        .ok_or_else(|| missing_param("phase", "string"))?;

    let body = json!({ "phase": phase });
    let result = call_daemon("POST", "/workflow/skip", Some(body)).await?;
    let text = format!("⏭️ Skip vers: {}", phase);
    Ok(json!({ "content": [{ "type": "text", "text": text }], "structuredContent": result }))
}

async fn cmd_workflow_abort(params: &Value) -> McpResult<Value> {
    let reason = params
        .get("reason")
        .and_then(Value::as_str)
        .ok_or_else(|| missing_param("reason", "string"))?;

    let body = json!({ "reason": reason });
    let result = call_daemon("POST", "/workflow/abort", Some(body)).await?;
    let text = format!("🛑 Workflow abandonné: {}", reason);
    Ok(json!({ "content": [{ "type": "text", "text": text }], "structuredContent": result }))
}

async fn cmd_workflow_history(params: &Value) -> McpResult<Value> {
    let limit = params.get("limit").and_then(Value::as_u64).unwrap_or(10);
    let endpoint = format!("/workflow/history?limit={}", limit);
    let result = call_daemon("GET", &endpoint, None).await?;
    let count = result
        .get("workflows")
        .and_then(Value::as_array)
        .map(|a| a.len())
        .unwrap_or(0);
    let text = format!("📜 {} workflow(s) dans l'historique", count);
    Ok(json!({ "content": [{ "type": "text", "text": text }], "structuredContent": result }))
}

// ============================================================================
// Review commands
// ============================================================================

async fn cmd_review_request(params: &Value, agent_id: &str) -> McpResult<Value> {
    let file = params
        .get("file")
        .and_then(Value::as_str)
        .ok_or_else(|| missing_param("file", "string"))?;

    let reviewers: Vec<String> = params
        .get("reviewers")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .map(String::from)
                .collect()
        })
        .unwrap_or_else(|| vec!["@beta".to_string()]);

    let review_type = params.get("type").and_then(Value::as_str).unwrap_or("code");

    let body = json!({
        "file": file,
        "reviewers": reviewers,
        "type": review_type,
        "requested_by": agent_id
    });

    let result = call_daemon("POST", "/review/request", Some(body)).await?;
    let req_id = result
        .get("request_id")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let text = format!(
        "🔍 Review #{} demandée pour {} (reviewers: {:?})",
        req_id, file, reviewers
    );
    Ok(json!({ "content": [{ "type": "text", "text": text }], "structuredContent": result }))
}

async fn cmd_review_approve(params: &Value, agent_id: &str) -> McpResult<Value> {
    let request_id = params
        .get("request_id")
        .and_then(Value::as_u64)
        .ok_or_else(|| missing_param("request_id", "integer"))?;
    let comment = params
        .get("comment")
        .and_then(Value::as_str)
        .unwrap_or("LGTM");

    let body = json!({
        "request_id": request_id,
        "comment": comment,
        "reviewer": agent_id
    });

    let result = call_daemon("POST", "/review/approve", Some(body)).await?;
    let text = format!("✅ Review #{} approuvée", request_id);
    Ok(json!({ "content": [{ "type": "text", "text": text }], "structuredContent": result }))
}

async fn cmd_review_comment(params: &Value, agent_id: &str) -> McpResult<Value> {
    let request_id = params
        .get("request_id")
        .and_then(Value::as_u64)
        .ok_or_else(|| missing_param("request_id", "integer"))?;
    let comment = params
        .get("comment")
        .and_then(Value::as_str)
        .ok_or_else(|| missing_param("comment", "string"))?;

    let body = json!({
        "request_id": request_id,
        "comment": comment,
        "reviewer": agent_id
    });

    let result = call_daemon("POST", "/review/comment", Some(body)).await?;
    let text = format!("💬 Comment ajouté à la review #{}", request_id);
    Ok(json!({ "content": [{ "type": "text", "text": text }], "structuredContent": result }))
}

async fn cmd_review_changes(params: &Value, agent_id: &str) -> McpResult<Value> {
    let request_id = params
        .get("request_id")
        .and_then(Value::as_u64)
        .ok_or_else(|| missing_param("request_id", "integer"))?;
    let comment = params
        .get("comment")
        .and_then(Value::as_str)
        .ok_or_else(|| missing_param("comment", "string"))?;

    let body = json!({
        "request_id": request_id,
        "comment": comment,
        "reviewer": agent_id
    });

    let result = call_daemon("POST", "/review/changes", Some(body)).await?;
    let text = format!("⚠️ Modifications demandées sur review #{}", request_id);
    Ok(json!({ "content": [{ "type": "text", "text": text }], "structuredContent": result }))
}

async fn cmd_review_status(params: &Value) -> McpResult<Value> {
    let request_id = params
        .get("request_id")
        .and_then(Value::as_u64)
        .ok_or_else(|| missing_param("request_id", "integer"))?;

    let endpoint = format!("/review/status/{}", request_id);
    let result = call_daemon("GET", &endpoint, None).await?;

    let status = result
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let file = result.get("file").and_then(Value::as_str).unwrap_or("?");
    let approvals = result.get("approvals").and_then(Value::as_u64).unwrap_or(0);
    let text = format!(
        "🔍 Review #{}: {} - {} ({} approval(s))",
        request_id, file, status, approvals
    );
    Ok(json!({ "content": [{ "type": "text", "text": text }], "structuredContent": result }))
}

async fn cmd_review_list(params: &Value) -> McpResult<Value> {
    let status = params.get("status").and_then(Value::as_str);
    let endpoint = match status {
        Some(s) => format!("/review/list?status={}", s),
        None => "/review/list".to_string(),
    };
    let result = call_daemon("GET", &endpoint, None).await?;
    let count = result
        .get("reviews")
        .and_then(Value::as_array)
        .map(|a| a.len())
        .unwrap_or(0);
    let text = format!("📋 {} review(s) en cours", count);
    Ok(json!({ "content": [{ "type": "text", "text": text }], "structuredContent": result }))
}

async fn cmd_review_history(params: &Value) -> McpResult<Value> {
    let limit = params.get("limit").and_then(Value::as_u64).unwrap_or(10);
    let endpoint = format!("/review/history?limit={}", limit);
    let result = call_daemon("GET", &endpoint, None).await?;
    let count = result
        .get("reviews")
        .and_then(Value::as_array)
        .map(|a| a.len())
        .unwrap_or(0);
    let text = format!("📜 {} review(s) dans l'historique", count);
    Ok(json!({ "content": [{ "type": "text", "text": text }], "structuredContent": result }))
}

// ============================================================================
// Memory commands
// ============================================================================

async fn cmd_memory_search(params: &Value) -> McpResult<Value> {
    let query = params
        .get("query")
        .and_then(Value::as_str)
        .ok_or_else(|| missing_param("query", "string"))?;

    let mut qp = vec![format!("q={}", encode_param(query))];
    if let Some(room) = params.get("room").and_then(Value::as_str) {
        qp.push(format!("room={}", encode_param(room)));
    }
    if let Some(agent) = params.get("agent").and_then(Value::as_str) {
        qp.push(format!("agent={}", encode_param(agent)));
    }
    if let Some(day) = params.get("day").and_then(Value::as_str) {
        qp.push(format!("day={}", day));
    }
    let limit = params.get("limit").and_then(Value::as_u64).unwrap_or(50);
    qp.push(format!("limit={}", limit));

    let endpoint = format!("/memory/search?{}", qp.join("&"));
    let result = call_daemon("GET", &endpoint, None).await?;
    let count = result
        .get("results")
        .and_then(Value::as_array)
        .map(|a| a.len())
        .unwrap_or(0);
    let text = format!("🔍 {} result(s) for '{}'", count, query);
    Ok(json!({ "content": [{ "type": "text", "text": text }], "structuredContent": result }))
}

async fn cmd_memory_get(params: &Value) -> McpResult<Value> {
    // If id is provided, use direct lookup
    if let Some(id) = params.get("id").and_then(Value::as_str) {
        let endpoint = format!("/memory/get?id={}", encode_param(id));
        let result = call_daemon("GET", &endpoint, None).await?;
        let text = format!("📝 Memory entry {}", id);
        return Ok(
            json!({ "content": [{ "type": "text", "text": text }], "structuredContent": result }),
        );
    }

    // Otherwise build query from day/hour/room/agent filters
    let mut qp = vec![];
    if let Some(day) = params.get("day").and_then(Value::as_str) {
        qp.push(format!("day={}", day));
    }
    if let Some(hour) = params.get("hour").and_then(Value::as_u64) {
        qp.push(format!("hour={}", hour));
    }
    if let Some(room) = params.get("room").and_then(Value::as_str) {
        qp.push(format!("room={}", encode_param(room)));
    }
    if let Some(agent) = params.get("agent").and_then(Value::as_str) {
        qp.push(format!("agent={}", encode_param(agent)));
    }
    let limit = params.get("limit").and_then(Value::as_u64).unwrap_or(100);
    qp.push(format!("limit={}", limit));

    if qp.len() <= 1 {
        return Err(validation_error(
            "memory/get requires 'id' or at least one filter (day, hour, room, agent)",
        ));
    }

    let endpoint = format!("/memory/get?{}", qp.join("&"));
    let result = call_daemon("GET", &endpoint, None).await?;
    let count = result
        .get("messages")
        .and_then(Value::as_array)
        .map(|a| a.len())
        .unwrap_or(0);
    let text = format!("📝 {} message(s) retrieved", count);
    Ok(json!({ "content": [{ "type": "text", "text": text }], "structuredContent": result }))
}

async fn cmd_memory_stats() -> McpResult<Value> {
    let result = call_daemon("GET", "/memory/stats", None).await?;
    let text = "📊 Memory statistics";
    Ok(json!({ "content": [{ "type": "text", "text": text }], "structuredContent": result }))
}
