// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2025-2026 naskel.com

//! devit_ports -- Show listening ports (ss wrapper)
//!
//! Quick view of what's listening on the machine.
//! Structured output for easy parsing by LLMs.

use async_trait::async_trait;
use mcp_core::{McpError, McpResult, McpTool};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::process::Command;
use tracing::info;

#[derive(Debug, Deserialize)]
struct PortsParams {
    /// Protocol filter: tcp (default), udp, all
    #[serde(default = "default_proto")]
    proto: String,
    /// Filter by port number
    #[serde(default)]
    port: Option<u16>,
}

fn default_proto() -> String {
    "tcp".into()
}

#[derive(Debug)]
struct ListeningPort {
    proto: String,
    local_addr: String,
    port: String,
    process: String,
    state: String,
}

pub struct PortsTool;

impl PortsTool {
    pub fn new() -> Self {
        Self
    }

    async fn get_ports(&self, proto: &str) -> Result<Vec<ListeningPort>, McpError> {
        let flag = match proto {
            "tcp" => "-tln",
            "udp" => "-uln",
            "all" => "-tuln",
            _ => "-tln",
        };

        let output = Command::new("ss")
            .args([flag, "-p"])
            .output()
            .await
            .map_err(|e| McpError::ExecutionFailed(format!("ss command failed: {e}")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(McpError::ExecutionFailed(format!(
                "ss failed: {}",
                stderr.trim()
            )));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut ports = Vec::new();

        for line in stdout.lines().skip(1) {
            // State Recv-Q Send-Q Local_Address:Port Peer_Address:Port Process
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 5 {
                continue;
            }

            let state = parts[0].to_string();
            let local = parts[4];

            // Parse address:port
            let (addr, port_str) = if let Some(idx) = local.rfind(':') {
                (&local[..idx], &local[idx + 1..])
            } else {
                (local, "*")
            };

            // Extract process name from "users:((\"name\",pid=N,fd=N))"
            let process = parts
                .get(6)
                .map(|p| p.split('"').nth(1).unwrap_or("").to_string())
                .unwrap_or_default();

            let proto_str = if state == "UNCONN" || parts.len() > 0 && line.contains("udp") {
                "udp"
            } else {
                "tcp"
            };

            ports.push(ListeningPort {
                proto: proto_str.to_string(),
                local_addr: addr.to_string(),
                port: port_str.to_string(),
                process,
                state,
            });
        }

        Ok(ports)
    }
}

#[async_trait]
impl McpTool for PortsTool {
    fn name(&self) -> &str {
        "devit_ports"
    }

    fn description(&self) -> &str {
        "Show listening ports on the machine. Quick debug tool to see what's running. \
         Filter by protocol (tcp/udp) or specific port number."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "proto": {
                    "type": "string",
                    "enum": ["tcp", "udp", "all"],
                    "description": "Protocol filter (default: tcp)",
                    "default": "tcp"
                },
                "port": {
                    "type": "integer",
                    "description": "Filter by specific port number"
                }
            }
        })
    }

    async fn execute(&self, params: Value) -> McpResult<Value> {
        let params: PortsParams = serde_json::from_value(params)
            .map_err(|e| McpError::InvalidRequest(format!("Invalid params: {e}")))?;

        info!(
            target: "devit_mcp_tools",
            "devit_ports | proto={} | port={:?}",
            params.proto, params.port
        );

        let mut ports = self.get_ports(&params.proto).await?;

        // Filter by port if specified
        if let Some(filter_port) = params.port {
            let filter_str = filter_port.to_string();
            ports.retain(|p| p.port == filter_str);
        }

        // Build text output
        let mut text = format!("Listening ports ({}):\n\n", params.proto);
        text.push_str(&format!(
            "{:<6} {:<25} {:<8} {}\n",
            "PROTO", "ADDRESS", "PORT", "PROCESS"
        ));
        text.push_str(&"-".repeat(60));
        text.push('\n');

        for p in &ports {
            text.push_str(&format!(
                "{:<6} {:<25} {:<8} {}\n",
                p.proto, p.local_addr, p.port, p.process
            ));
        }

        text.push_str(&format!("\nTotal: {} listening", ports.len()));

        // Structured
        let entries: Vec<Value> = ports
            .iter()
            .map(|p| {
                json!({
                    "proto": p.proto,
                    "address": p.local_addr,
                    "port": p.port,
                    "process": p.process,
                    "state": p.state,
                })
            })
            .collect();

        Ok(json!({
            "content": [{"type": "text", "text": text}],
            "structuredContent": {
                "ports": {
                    "count": ports.len(),
                    "proto_filter": params.proto,
                    "entries": entries
                }
            }
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_params_defaults() {
        let v = json!({});
        let p: PortsParams = serde_json::from_value(v).unwrap();
        assert_eq!(p.proto, "tcp");
        assert!(p.port.is_none());
    }

    #[test]
    fn test_params_custom() {
        let v = json!({"proto": "udp", "port": 5555});
        let p: PortsParams = serde_json::from_value(v).unwrap();
        assert_eq!(p.proto, "udp");
        assert_eq!(p.port.unwrap(), 5555);
    }
}
