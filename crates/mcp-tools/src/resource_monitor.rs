// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2025-2026 naskel.com

use async_trait::async_trait;
use mcp_core::{McpResult, McpTool};
use serde_json::{json, Value};
use std::process::Command;
use sysinfo::System;

use crate::validation_error;

pub struct ResourceMonitor;

impl ResourceMonitor {
    pub fn new() -> Self {
        Self
    }

    fn get_cpu_info() -> Value {
        let mut sys = System::new_all();
        sys.refresh_all();

        // In sysinfo 0.31, use cpus() method
        let cpus = sys.cpus();
        let global_usage: f64 =
            cpus.iter().map(|cpu| cpu.cpu_usage() as f64).sum::<f64>() / cpus.len() as f64;

        // Collect per-core usage
        let cores: Vec<Value> = cpus
            .iter()
            .enumerate()
            .map(|(idx, cpu)| {
                json!({
                    "core": idx,
                    "usage_percent": cpu.cpu_usage() as f64,
                    "frequency_mhz": cpu.frequency(),
                    "name": cpu.name(),
                })
            })
            .collect();

        // Try to get CPU temperature
        let temp_celsius = get_cpu_temperature();

        json!({
            "global_usage_percent": global_usage,
            "num_cores": cpus.len(),
            "cores": cores,
            "temperature_celsius": temp_celsius,
        })
    }

    fn get_memory_info() -> Value {
        let mut sys = System::new_all();
        sys.refresh_all();

        let total_memory = sys.total_memory();
        let used_memory = sys.used_memory();
        let available_memory = sys.available_memory();
        let swap_total = sys.total_swap();
        let swap_used = sys.used_swap();

        let memory_usage_percent = if total_memory > 0 {
            ((used_memory as f64 / total_memory as f64) * 100.0) as f64
        } else {
            0.0
        };

        let swap_usage_percent = if swap_total > 0 {
            ((swap_used as f64 / swap_total as f64) * 100.0) as f64
        } else {
            0.0
        };

        json!({
            "total_mb": total_memory / 1024,
            "used_mb": used_memory / 1024,
            "available_mb": available_memory / 1024,
            "usage_percent": memory_usage_percent,
            "swap": {
                "total_mb": swap_total / 1024,
                "used_mb": swap_used / 1024,
                "usage_percent": swap_usage_percent,
            }
        })
    }

    fn get_disk_info() -> Value {
        let disks = sysinfo::Disks::new_with_refreshed_list();

        let disk_info: Vec<Value> = disks
            .list()
            .iter()
            .map(|disk| {
                let total_space = disk.total_space();
                let available_space = disk.available_space();
                let used_space = total_space.saturating_sub(available_space);
                let usage_percent = if total_space > 0 {
                    ((used_space as f64 / total_space as f64) * 100.0) as f64
                } else {
                    0.0
                };

                json!({
                    "mount_point": disk.mount_point().display().to_string(),
                    "file_system": disk.file_system().to_string_lossy().to_string(),
                    "total_gb": total_space / (1024 * 1024 * 1024),
                    "used_gb": used_space / (1024 * 1024 * 1024),
                    "available_gb": available_space / (1024 * 1024 * 1024),
                    "usage_percent": usage_percent,
                })
            })
            .collect();

        json!({
            "disks": disk_info,
        })
    }

    fn get_network_info() -> Value {
        let networks = sysinfo::Networks::new_with_refreshed_list();

        let interfaces: Vec<Value> = networks
            .list()
            .iter()
            .map(|(interface_name, data)| {
                json!({
                    "name": interface_name,
                    "received_bytes": data.received(),
                    "transmitted_bytes": data.transmitted(),
                    "received_packets": data.packets_received(),
                    "transmitted_packets": data.packets_transmitted(),
                    "errors_in": data.errors_on_received(),
                    "errors_out": data.errors_on_transmitted(),
                })
            })
            .collect();

        json!({
            "interfaces": interfaces,
        })
    }

    fn get_gpu_info() -> Value {
        // Try to get NVIDIA GPU info using nvidia-smi
        match Command::new("nvidia-smi")
            .args(&[
                "--query-gpu=index,name,memory.total,memory.used,memory.free,utilization.gpu,utilization.memory,temperature.gpu",
                "--format=csv,noheader,nounits"
            ])
            .output()
        {
            Ok(output) => {
                if output.status.success() {
                    let output_str = String::from_utf8_lossy(&output.stdout);
                    let gpus: Vec<Value> = output_str
                        .lines()
                        .filter_map(|line| {
                            let parts: Vec<&str> = line.split(',').map(|s| s.trim()).collect();
                            if parts.len() >= 8 {
                                Some(json!({
                                    "index": parts[0].parse::<u32>().unwrap_or(0),
                                    "name": parts[1],
                                    "memory_total_mb": parts[2].parse::<u64>().unwrap_or(0),
                                    "memory_used_mb": parts[3].parse::<u64>().unwrap_or(0),
                                    "memory_free_mb": parts[4].parse::<u64>().unwrap_or(0),
                                    "utilization_gpu_percent": parts[5].parse::<f64>().unwrap_or(0.0),
                                    "utilization_memory_percent": parts[6].parse::<f64>().unwrap_or(0.0),
                                    "temperature_celsius": parts[7].parse::<f64>().unwrap_or(0.0),
                                }))
                            } else {
                                None
                            }
                        })
                        .collect();

                    if !gpus.is_empty() {
                        return json!({
                            "available": true,
                            "gpus": gpus,
                        });
                    }
                }
                json!({
                    "available": false,
                    "reason": "nvidia-smi query failed",
                })
            }
            Err(_) => {
                json!({
                    "available": false,
                    "reason": "nvidia-smi not found",
                })
            }
        }
    }
}

// Helper: Try to read CPU temperature from /sys/class/thermal
fn get_cpu_temperature() -> Option<f64> {
    for i in 0..10 {
        let path = format!("/sys/class/thermal/thermal_zone{}/temp", i);
        if let Ok(content) = std::fs::read_to_string(&path) {
            if let Ok(temp_millidegrees) = content.trim().parse::<f64>() {
                return Some(temp_millidegrees / 1000.0); // Convert millidegrees to degrees
            }
        }
    }
    None
}

#[async_trait]
impl McpTool for ResourceMonitor {
    fn name(&self) -> &str {
        "devit_resource_monitor"
    }

    fn description(&self) -> &str {
        "Monitor system resources: CPU, RAM, disk, network, and GPU usage with real-time metrics."
    }

    async fn execute(&self, params: Value) -> McpResult<Value> {
        if !params.is_null() && !params.is_object() {
            return Err(validation_error(
                "Parameters must be a JSON object (or omitted).",
            ));
        }

        // Get which metrics to include (default: all)
        let include_cpu = params
            .get("include_cpu")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let include_memory = params
            .get("include_memory")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let include_disk = params
            .get("include_disk")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let include_network = params
            .get("include_network")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let include_gpu = params
            .get("include_gpu")
            .and_then(Value::as_bool)
            .unwrap_or(true);

        let mut metrics = json!({});

        if include_cpu {
            metrics["cpu"] = Self::get_cpu_info();
        }
        if include_memory {
            metrics["memory"] = Self::get_memory_info();
        }
        if include_disk {
            metrics["disk"] = Self::get_disk_info();
        }
        if include_network {
            metrics["network"] = Self::get_network_info();
        }
        if include_gpu {
            metrics["gpu"] = Self::get_gpu_info();
        }

        // Add metadata
        let mut result = json!({
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "hostname": get_hostname(),
            "uptime_seconds": get_uptime(),
            "metrics": metrics,
        });

        // Add summary
        if include_memory && include_cpu {
            if let (Some(cpu_usage), Some(mem_usage)) = (
                metrics
                    .get("cpu")
                    .and_then(|c| c.get("global_usage_percent"))
                    .and_then(Value::as_f64),
                metrics
                    .get("memory")
                    .and_then(|m| m.get("usage_percent"))
                    .and_then(Value::as_f64),
            ) {
                result["summary"] = json!({
                    "cpu_usage_percent": cpu_usage,
                    "memory_usage_percent": mem_usage,
                    "critical_alert": cpu_usage > 90.0 || mem_usage > 90.0,
                });
            }
        }

        Ok(json!({
            "content": [{
                "type": "text",
                "text": format!("📊 System Resources ({})", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"))
            }],
            "structuredContent": result
        }))
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "include_cpu": {
                    "type": "boolean",
                    "description": "Include CPU metrics (usage, cores, temperature)",
                    "default": true
                },
                "include_memory": {
                    "type": "boolean",
                    "description": "Include RAM and swap metrics",
                    "default": true
                },
                "include_disk": {
                    "type": "boolean",
                    "description": "Include disk space metrics for all partitions",
                    "default": true
                },
                "include_network": {
                    "type": "boolean",
                    "description": "Include network interface statistics",
                    "default": true
                },
                "include_gpu": {
                    "type": "boolean",
                    "description": "Include NVIDIA GPU metrics (if available)",
                    "default": true
                }
            },
            "additionalProperties": false
        })
    }
}

fn get_hostname() -> String {
    match std::fs::read_to_string("/etc/hostname") {
        Ok(content) => content.trim().to_string(),
        Err(_) => "unknown".to_string(),
    }
}

fn get_uptime() -> u64 {
    match std::fs::read_to_string("/proc/uptime") {
        Ok(content) => {
            if let Some(uptime_str) = content.split_whitespace().next() {
                uptime_str.parse::<f64>().unwrap_or(0.0) as u64
            } else {
                0
            }
        }
        Err(_) => 0,
    }
}
