// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2025-2026 naskel.com

use std::net::IpAddr;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::journal_best_effort as jbe;
use crate::net_utils::{
    detect_injection_text, detect_paywall_hint, extract_article_content, robots_policy_for,
    sanitize_html_to_text, ExtractMode, RobotsPolicy,
};
use async_trait::async_trait;
use chrono::Utc;
use devit_common::cache::cache_key;
use devit_common::limits::{resolve_fetch_limits, EffectiveLimits, LimitSources};
use mcp_core::{McpError, McpResult, McpTool};
use reqwest::header::{ACCEPT, ACCEPT_LANGUAGE, CACHE_CONTROL, PRAGMA, USER_AGENT};
use reqwest::redirect::Policy as RedirectPolicy;
use reqwest::Client;
use serde_json::{json, Value};
use tracing::info;
use url::Url;
use uuid::Uuid;

/// MCP tool: devit_fetch_url — safe HTML/text fetch with robots + sanitizer
pub struct FetchUrlTool;

impl FetchUrlTool {
    pub fn new() -> Arc<Self> {
        Arc::new(Self)
    }

    fn validate_url(url: &str) -> Result<Url, McpError> {
        let u = url.trim();
        if !(u.starts_with("http://") || u.starts_with("https://")) {
            return Err(McpError::InvalidRequest(
                "'url' must start with http:// or https://".into(),
            ));
        }
        if u.len() > 2048 {
            return Err(McpError::InvalidRequest("url too long".into()));
        }
        Url::parse(u).map_err(|e| McpError::InvalidRequest(format!("invalid url: {e}")))
    }

    fn env_bool(name: &str, default_true: bool) -> bool {
        let val = std::env::var(name).ok().unwrap_or_default().to_lowercase();
        match val.as_str() {
            "0" | "false" | "no" | "off" => !default_true,
            "1" | "true" | "yes" | "on" => true,
            _ => default_true,
        }
    }

    fn user_agent(params: &Value) -> String {
        params
            .get("user_agent")
            .and_then(Value::as_str)
            .map(|s| s.to_string())
            .or_else(|| std::env::var("DEVIT_HTTP_USER_AGENT").ok())
            .unwrap_or_else(|| "DevItBot/1.0".to_string())
    }

    fn is_private_host(host: &str) -> bool {
        if let Ok(ip) = IpAddr::from_str(host) {
            return match ip {
                IpAddr::V4(v4) => {
                    let o = v4.octets();
                    // 10.0.0.0/8
                    (o[0] == 10)
                        // 172.16.0.0/12
                        || (o[0] == 172 && (16..=31).contains(&o[1]))
                        // 192.168.0.0/16
                        || (o[0] == 192 && o[1] == 168)
                        // 127.0.0.0/8
                        || (o[0] == 127)
                }
                IpAddr::V6(v6) => v6.is_loopback() || v6.is_unique_local(),
            };
        }
        // Heuristic: block obvious internal hostnames
        let low = host.to_lowercase();
        low.ends_with(".local") || low.ends_with(".corp") || low == "localhost"
    }

    async fn fetch_robots(client: &Client, url: &Url, timeout_ms: u64) -> Option<String> {
        let mut robots = url.clone();
        robots.set_path("/robots.txt");
        robots.set_query(None);
        let req = client
            .get(robots)
            .header(USER_AGENT, Self::user_agent(&json!({})))
            .timeout(Duration::from_millis(timeout_ms));
        match req.send().await {
            Ok(r) if r.status().is_success() => r.text().await.ok(),
            _ => None,
        }
    }
}

#[async_trait]
impl McpTool for FetchUrlTool {
    fn name(&self) -> &str {
        "devit_fetch_url"
    }

    fn description(&self) -> &str {
        "Fetch HTML/text content with safety policies, timeouts and size budgets"
    }

    async fn execute(&self, params: Value) -> McpResult<Value> {
        let url_raw = params
            .get("url")
            .and_then(Value::as_str)
            .ok_or_else(|| McpError::InvalidRequest("'url' is required".into()))?;

        let url = Self::validate_url(url_raw)?;

        // Resolve normalized limits from params/env/defaults
        let (effective_limits, limit_sources): (EffectiveLimits, LimitSources) =
            resolve_fetch_limits(
                params.get("timeout_ms").and_then(Value::as_u64),
                params.get("max_bytes").and_then(Value::as_u64),
                params.get("follow_redirects").and_then(Value::as_bool),
            );
        let timeout_ms = effective_limits.timeout_ms;
        let max_bytes = effective_limits.max_bytes.unwrap_or(500_000);
        let follow_redirects = effective_limits.follow_redirects;
        let _safe_mode = params
            .get("safe_mode")
            .and_then(Value::as_str)
            .unwrap_or("moderate");
        let extract_mode = params
            .get("extract_mode")
            .and_then(Value::as_str)
            .map(ExtractMode::from_str)
            .unwrap_or(ExtractMode::Raw);
        let respect_robots = Self::env_bool("DEVIT_RESPECT_ROBOTS", true);
        let block_private = Self::env_bool("DEVIT_BLOCK_PRIVATE_CIDRS", true);
        let agent = Self::user_agent(&params);

        if block_private {
            if let Some(host) = url.host_str() {
                if Self::is_private_host(host) {
                    return Err(McpError::rpc(
                        -32600,
                        "DISALLOWED_DOMAIN",
                        Some(json!({
                            "message": "Private or internal host is not allowed",
                            "host": host
                        })),
                    ));
                }
            }
        }

        let client = reqwest::Client::builder()
            .user_agent(&agent)
            .redirect(if follow_redirects {
                RedirectPolicy::limited(effective_limits.max_redirects as usize)
            } else {
                RedirectPolicy::none()
            })
            .timeout(Duration::from_millis(timeout_ms))
            .gzip(true)
            .brotli(true)
            .deflate(true)
            .build()
            .map_err(|e| McpError::Internal(format!("http client error: {e}")))?;

        // Robots check
        let accept_hdr = "text/html, text/plain;q=0.9, */*;q=0.1";
        let cache_key_val = cache_key(url.as_str(), accept_hdr, &agent, _safe_mode, true);
        let mut robots_policy_str = "unknown".to_string();

        if respect_robots {
            if let Some(host) = url.host_str() {
                if let Some(text) = Self::fetch_robots(&client, &url, timeout_ms)
                    .await
                    .as_deref()
                {
                    let policy = robots_policy_for(url.path(), text);
                    robots_policy_str = match policy {
                        RobotsPolicy::Allow => "allow".to_string(),
                        RobotsPolicy::Disallow => "disallow".to_string(),
                    };
                    if let RobotsPolicy::Disallow = policy {
                        let tid = Uuid::new_v4().to_string();
                        info!(target: "mcp.fetch", trace_id=%tid, op="fetch", host=%host, policy=%robots_policy_str, cache_key=%cache_key_val, "blocked by robots");
                        let meta = build_fetch_meta(
                            &tid,
                            &robots_policy_str,
                            false,
                            0,
                            &effective_limits,
                            &limit_sources,
                            &cache_key_val,
                        );
                        return Ok(fetch_error_response(
                            url.as_str(),
                            url.as_str(),
                            0,
                            "",
                            0,
                            json!({}),
                            meta,
                            "ROBOTS_DISALLOW",
                            "Robots.txt disallows this path",
                            &format!("Robots policy disallows fetching: {}", url),
                        ));
                    }
                }
            }
        }

        // Fetch
        let start = Instant::now();
        let req = client
            .get(url.clone())
            .header(USER_AGENT, agent.as_str())
            .header(ACCEPT, accept_hdr)
            .header(ACCEPT_LANGUAGE, "en-US,en;q=0.9,fr;q=0.8")
            .header(CACHE_CONTROL, "no-cache")
            .header(PRAGMA, "no-cache");

        let trace_id = Uuid::new_v4().to_string();
        match req.send().await {
            Ok(mut r) => {
                let status = r.status().as_u16() as i64;
                let final_url = r.url().to_string();
                let content_type = r
                    .headers()
                    .get(reqwest::header::CONTENT_TYPE)
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("")
                    .to_lowercase();
                let hdrs = header_map_to_json(r.headers());

                // MIME whitelist
                let allowed = content_type.starts_with("text/html")
                    || content_type.starts_with("text/plain")
                    || content_type.starts_with("application/xhtml+xml")
                    || content_type.starts_with("application/json");
                if !allowed {
                    info!(target: "mcp.fetch", %trace_id, op="fetch", url=%final_url, status=%status, ct=%content_type, cache_key=%cache_key_val, "unsupported mime");
                    let meta = build_fetch_meta(
                        &trace_id,
                        &robots_policy_str,
                        false,
                        start.elapsed().as_millis() as u64,
                        &effective_limits,
                        &limit_sources,
                        &cache_key_val,
                    );
                    return Ok(fetch_error_response(
                        url.as_str(),
                        &final_url,
                        status,
                        &content_type,
                        0,
                        hdrs,
                        meta,
                        "UNSUPPORTED_MIME",
                        "Only text/html, text/plain, application/xhtml+xml are allowed",
                        &format!("Unsupported MIME type: {}", content_type),
                    ));
                }

                // Size guard (Content-Length)
                if let Some(len) = r.content_length() {
                    if len > max_bytes {
                        info!(target: "mcp.fetch", %trace_id, op="fetch", url=%final_url, status=%status, content_length=%len, cache_key=%cache_key_val, "too large");
                        let meta = build_fetch_meta(
                            &trace_id,
                            &robots_policy_str,
                            false,
                            start.elapsed().as_millis() as u64,
                            &effective_limits,
                            &limit_sources,
                            &cache_key_val,
                        );
                        return Ok(fetch_error_response(
                            url.as_str(),
                            &final_url,
                            status,
                            &content_type,
                            len as usize,
                            hdrs,
                            meta,
                            "TOO_LARGE",
                            "Content-Length exceeds limit",
                            &format!("Response too large ({} bytes > limit {})", len, max_bytes),
                        ));
                    }
                }

                // Stream body with limit
                let mut bytes: Vec<u8> = Vec::new();
                while let Ok(Some(chunk)) = r.chunk().await {
                    bytes.extend_from_slice(&chunk);
                    if (bytes.len() as u64) > max_bytes {
                        info!(target: "mcp.fetch", %trace_id, op="fetch", url=%final_url, status=%status, downloaded=%bytes.len(), cache_key=%cache_key_val, "stream too large");
                        let meta = build_fetch_meta(
                            &trace_id,
                            &robots_policy_str,
                            false,
                            start.elapsed().as_millis() as u64,
                            &effective_limits,
                            &limit_sources,
                            &cache_key_val,
                        );
                        return Ok(fetch_error_response(
                            url.as_str(),
                            &final_url,
                            status,
                            &content_type,
                            bytes.len(),
                            hdrs,
                            meta,
                            "TOO_LARGE",
                            "Streamed body exceeded limit",
                            &format!("Response exceeded max_bytes limit ({} bytes)", bytes.len()),
                        ));
                    }
                }

                // Extract and process content
                let body_str = String::from_utf8_lossy(&bytes).to_string();
                let paywall_detected = detect_paywall_hint(&body_str);
                let is_html = content_type.starts_with("text/html")
                    || content_type.starts_with("application/xhtml+xml");
                let effective_mode = match extract_mode {
                    ExtractMode::Auto if is_html => ExtractMode::Article,
                    other => other,
                };

                let (text, article_meta) = if is_html {
                    match effective_mode {
                        ExtractMode::Article | ExtractMode::Auto => {
                            let article = extract_article_content(&body_str);
                            let meta = json!({ "title": article.title, "quality_score": article.quality_score, "has_code": article.has_code, "paragraph_count": article.paragraph_count });
                            (article.text, Some(meta))
                        }
                        ExtractMode::Raw => (sanitize_html_to_text(&body_str), None),
                    }
                } else {
                    (body_str, None)
                };

                let mut errors: Vec<Value> = Vec::new();
                let content_text = if detect_injection_text(&text) {
                    errors.push(json!({"code": "SANITIZER_BLOCKED", "message": "Content flagged as potentially prompt-injection"}));
                    None
                } else {
                    Some(format!(
                        "[UNTRUSTED_CONTENT_START]\n{}\n[UNTRUSTED_CONTENT_END]",
                        text
                    ))
                };

                let elapsed_ms = start.elapsed().as_millis() as u64;
                info!(target: "mcp.fetch", %trace_id, op="fetch", url=%final_url, status=%status, bytes=%bytes.len(), elapsed_ms=%elapsed_ms, robots=%robots_policy_str, paywall=%paywall_detected, effective_limits=?effective_limits, limit_sources=?limit_sources, delegation_context=?None::<()>, cache_key=%cache_key_val, "fetch done");

                let extract_mode_str = match effective_mode {
                    ExtractMode::Article => "article",
                    ExtractMode::Auto => "auto",
                    ExtractMode::Raw => "raw",
                };
                let mut meta = build_fetch_meta(
                    &trace_id,
                    &robots_policy_str,
                    paywall_detected,
                    elapsed_ms,
                    &effective_limits,
                    &limit_sources,
                    &cache_key_val,
                );
                if let Some(obj) = meta.as_object_mut() {
                    obj.insert("extract_mode".into(), json!(extract_mode_str));
                    obj.insert(
                        "extraction".into(),
                        article_meta.clone().unwrap_or(Value::Null),
                    );
                }
                jbe::append(
                    "fetch",
                    &json!({ "url": url.as_str(), "final_url": final_url, "status": status, "meta": meta }),
                );

                // Build summary
                let mut summary = format!(
                    "Fetched {} ({} bytes, {} ms)",
                    final_url,
                    bytes.len(),
                    elapsed_ms
                );
                if let Some(ref ext) = article_meta {
                    if let Some(title) = ext.get("title").and_then(|v| v.as_str()) {
                        summary = format!("{}\nTitle: {}", summary, title);
                    }
                    if let Some(quality) = ext.get("quality_score").and_then(|v| v.as_f64()) {
                        summary = format!("{} | Quality: {:.0}%", summary, quality * 100.0);
                    }
                }

                let full_text = match &content_text {
                    Some(ct) => format!("{}\n\n{}", summary, ct),
                    None => format!("{}\n\n[Content blocked or empty]", summary),
                };

                Ok(json!({
                    "content": [{"type": "text", "text": full_text}],
                    "metadata": {
                        "url": url.as_str(),
                        "final_url": final_url,
                        "status": status,
                        "retrieved_at": Utc::now().to_rfc3339(),
                        "content_type": content_type,
                        "content_text": content_text,
                        "content_bytes": bytes.len(),
                        "headers": hdrs,
                        "meta": meta,
                        "errors": errors
                    }
                }))
            }
            Err(e) => {
                let es = e.to_string().to_lowercase();
                let is_redirect =
                    es.contains("redirect") && (es.contains("too many") || es.contains("loop"));
                let code = if is_redirect {
                    "REDIRECT_LOOP"
                } else {
                    "NETWORK_ERROR"
                };
                info!(target: "mcp.fetch", op="fetch", err=%e.to_string(), redirect_loop=%is_redirect, cache_key=%cache_key_val, "fetch failed");
                let meta = build_fetch_meta(
                    &Uuid::new_v4().to_string(),
                    &robots_policy_str,
                    false,
                    0,
                    &effective_limits,
                    &limit_sources,
                    &cache_key_val,
                );
                Ok(fetch_error_response(
                    url.as_str(),
                    url.as_str(),
                    0,
                    "",
                    0,
                    json!({}),
                    meta,
                    code,
                    &e.to_string(),
                    &format!("Fetch failed: {}", e),
                ))
            }
        }
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "minLength": 1,
                    "description": "URL to fetch"
                },
                "max_bytes": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 1000000,
                    "description": "Maximum response size in bytes"
                },
                "timeout_ms": {
                    "type": "integer",
                    "minimum": 100,
                    "maximum": 10000,
                    "description": "Request timeout in milliseconds"
                },
                "follow_redirects": {
                    "type": "boolean",
                    "description": "Whether to follow HTTP redirects"
                },
                "user_agent": {
                    "type": "string",
                    "description": "Custom User-Agent header"
                },
                "safe_mode": {
                    "type": "string",
                    "enum": ["strict", "moderate", "off"],
                    "description": "Safety mode for content filtering"
                },
                "extract_mode": {
                    "type": "string",
                    "enum": ["raw", "article", "auto"],
                    "default": "raw",
                    "description": "Content extraction mode: 'raw' returns all text, 'article' extracts main content using Readability-like heuristics, 'auto' uses article mode for HTML"
                }
            },
            "required": ["url"]
        })
    }
}

fn header_map_to_json(map: &reqwest::header::HeaderMap) -> Value {
    let mut out = serde_json::Map::new();
    for (k, v) in map.iter() {
        if let Ok(val) = v.to_str() {
            out.insert(k.as_str().to_string(), Value::String(val.to_string()));
        }
    }
    Value::Object(out)
}

/// Build the standard metadata object for fetch responses.
fn build_fetch_meta(
    trace_id: &str,
    robots_policy: &str,
    paywall: bool,
    elapsed_ms: u64,
    limits: &EffectiveLimits,
    sources: &LimitSources,
    cache_key: &str,
) -> Value {
    json!({
        "trace_id": trace_id,
        "robots_policy": robots_policy,
        "paywall_detected": paywall,
        "from_cache": false,
        "elapsed_ms": elapsed_ms,
        "effective_limits": limits,
        "limit_sources": sources,
        "delegation_context": serde_json::Value::Null,
        "cache_key": cache_key
    })
}

/// Build a standard error/early-return response for fetch.
fn fetch_error_response(
    url: &str,
    final_url: &str,
    status: i64,
    content_type: &str,
    content_bytes: usize,
    headers: Value,
    meta: Value,
    error_code: &str,
    error_message: &str,
    display_message: &str,
) -> Value {
    jbe::append(
        "fetch",
        &json!({ "url": url, "final_url": final_url, "status": status, "meta": meta }),
    );
    json!({
        "content": [{"type": "text", "text": display_message}],
        "metadata": {
            "url": url,
            "final_url": final_url,
            "status": status,
            "retrieved_at": Utc::now().to_rfc3339(),
            "content_type": content_type,
            "content_text": null,
            "content_bytes": content_bytes,
            "headers": headers,
            "meta": meta,
            "errors": [{"code": error_code, "message": error_message}]
        }
    })
}
