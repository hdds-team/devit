// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2025-2026 naskel.com

//! Multi-engine web search with fallback and deduplication
//!
//! Supported engines:
//! - DuckDuckGo (default, no API key needed)
//! - Brave Search (requires API key via BRAVE_API_KEY)
//! - SearXNG (self-hosted, configurable via SEARXNG_URL)

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::journal_best_effort as jbe;
use async_trait::async_trait;
use chrono::Utc;
use devit_common::cache::cache_key;
use devit_common::limits::{resolve_search_limits, EffectiveLimits, LimitSources};
use mcp_core::{McpError, McpResult, McpTool};
use parking_lot::RwLock;
use regex::Regex;
use reqwest::redirect::Policy as RedirectPolicy;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tracing::{info, warn};
use url::Url;
use uuid::Uuid;

// ============================================================================
// Types
// ============================================================================

/// A single search result with URL, title, and snippet
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SearchResult {
    pub url: String,
    pub title: String,
    pub snippet: Option<String>,
    pub domain: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>, // Which engine found this result
}

/// Search engine identifier
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SearchEngine {
    DuckDuckGo,
    Brave,
    SearXNG,
    Auto, // Try engines in order until one works
}

impl SearchEngine {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "duckduckgo" | "ddg" => SearchEngine::DuckDuckGo,
            "brave" => SearchEngine::Brave,
            "searxng" | "searx" => SearchEngine::SearXNG,
            "auto" | _ => SearchEngine::Auto,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            SearchEngine::DuckDuckGo => "duckduckgo",
            SearchEngine::Brave => "brave",
            SearchEngine::SearXNG => "searxng",
            SearchEngine::Auto => "auto",
        }
    }

    /// Returns the priority order for Auto mode
    fn auto_order() -> Vec<SearchEngine> {
        let mut engines = vec![SearchEngine::DuckDuckGo];

        // Add Brave if API key is configured
        if std::env::var("BRAVE_API_KEY").is_ok() {
            engines.insert(0, SearchEngine::Brave); // Brave first if available
        }

        // Add SearXNG if URL is configured
        if std::env::var("SEARXNG_URL").is_ok() {
            engines.push(SearchEngine::SearXNG);
        }

        engines
    }
}

/// Result of a search attempt
#[derive(Debug)]
pub struct SearchAttempt {
    pub results: Vec<SearchResult>,
    pub engine: SearchEngine,
    pub success: bool,
    pub error: Option<String>,
    pub elapsed_ms: u64,
    pub retry_count: u32,
}

// ============================================================================
// Cache
// ============================================================================

#[derive(Clone, Debug)]
struct CachedResponse {
    results: Vec<SearchResult>,
    engine: SearchEngine,
    cached_at: Instant,
}

struct SearchCache {
    entries: HashMap<String, CachedResponse>,
    ttl: Duration,
    max_entries: usize,
}

impl SearchCache {
    fn new(ttl_secs: u64, max_entries: usize) -> Self {
        Self {
            entries: HashMap::new(),
            ttl: Duration::from_secs(ttl_secs),
            max_entries,
        }
    }

    fn get(&self, key: &str) -> Option<(Vec<SearchResult>, SearchEngine)> {
        self.entries.get(key).and_then(|cached| {
            if cached.cached_at.elapsed() < self.ttl {
                Some((cached.results.clone(), cached.engine))
            } else {
                None
            }
        })
    }

    fn insert(&mut self, key: String, results: Vec<SearchResult>, engine: SearchEngine) {
        if self.entries.len() >= self.max_entries {
            self.entries.retain(|_, v| v.cached_at.elapsed() < self.ttl);
        }
        if self.entries.len() >= self.max_entries {
            if let Some(oldest_key) = self
                .entries
                .iter()
                .min_by_key(|(_, v)| v.cached_at)
                .map(|(k, _)| k.clone())
            {
                self.entries.remove(&oldest_key);
            }
        }
        self.entries.insert(
            key,
            CachedResponse {
                results,
                engine,
                cached_at: Instant::now(),
            },
        );
    }
}

static SEARCH_CACHE: std::sync::LazyLock<RwLock<SearchCache>> = std::sync::LazyLock::new(|| {
    let ttl = std::env::var("DEVIT_SEARCH_CACHE_TTL_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(900);
    RwLock::new(SearchCache::new(ttl, 100))
});

// ============================================================================
// Search Backend Trait
// ============================================================================

#[async_trait]
trait SearchBackend: Send + Sync {
    fn engine(&self) -> SearchEngine;
    fn is_available(&self) -> bool;

    async fn search(
        &self,
        query: &str,
        max_results: usize,
        time_range: Option<&str>,
        safe_mode: &str,
        timeout_ms: u64,
    ) -> Result<Vec<SearchResult>, String>;
}

// ============================================================================
// DuckDuckGo Backend
// ============================================================================

struct DuckDuckGoBackend;

impl DuckDuckGoBackend {
    fn ddg_base() -> String {
        std::env::var("DDG_ENDPOINT").unwrap_or_else(|_| "https://duckduckgo.com/html".to_string())
    }

    fn parse_results(html: &str, limit: usize, max_per_domain: usize) -> Vec<SearchResult> {
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        let mut per_domain: HashMap<String, usize> = HashMap::new();

        // Block-based parsing
        let block_re =
            Regex::new(r#"(?s)<div[^>]*class="[^"]*result[^"]*"[^>]*>.*?</div>\s*</div>"#).ok();
        let link_re =
            Regex::new(r#"<a[^>]+href="(https://duckduckgo\.com/l/[^"]+)"[^>]*>(.*?)</a>"#).ok();
        let snippet_re =
            Regex::new(r#"<a[^>]+class="[^"]*result__snippet[^"]*"[^>]*>(.*?)</a>"#).ok();

        if let Some(ref block_pattern) = block_re {
            for block in block_pattern.find_iter(html) {
                let block_html = block.as_str();

                let link_match = link_re.as_ref().and_then(|re| re.captures(block_html));
                let Some(link_caps) = link_match else {
                    continue;
                };

                let href = link_caps.get(1).map(|m| m.as_str()).unwrap_or("");
                let title_raw = link_caps.get(2).map(|m| m.as_str()).unwrap_or("");

                let Ok(ddg_url) = Url::parse(href) else {
                    continue;
                };
                let target = ddg_url
                    .query_pairs()
                    .find(|(k, _)| k == "uddg")
                    .map(|(_, v)| v.into_owned());
                let Some(decoded) = target else { continue };

                if seen.contains(&decoded) {
                    continue;
                }

                let domain = domain_of(&decoded).unwrap_or_default();
                let count = per_domain.get(&domain).copied().unwrap_or(0);
                if count >= max_per_domain {
                    continue;
                }

                let snippet = snippet_re
                    .as_ref()
                    .and_then(|re| re.captures(block_html))
                    .map(|caps| caps.get(1).map(|m| m.as_str()).unwrap_or(""))
                    .map(html_unescape)
                    .filter(|s| !s.is_empty());

                let title = html_unescape(title_raw);

                out.push(SearchResult {
                    url: decoded.clone(),
                    title,
                    snippet,
                    domain: domain.clone(),
                    source: Some("duckduckgo".to_string()),
                });

                seen.insert(decoded);
                per_domain.insert(domain, count + 1);

                if out.len() >= limit {
                    break;
                }
            }
        }

        // Fallback: direct link extraction
        if out.is_empty() {
            if let Some(ref re) = link_re {
                for caps in re.captures_iter(html) {
                    let href = caps.get(1).map(|m| m.as_str()).unwrap_or("");
                    let title_raw = caps.get(2).map(|m| m.as_str()).unwrap_or("");

                    let Ok(ddg_url) = Url::parse(href) else {
                        continue;
                    };
                    let target = ddg_url
                        .query_pairs()
                        .find(|(k, _)| k == "uddg")
                        .map(|(_, v)| v.into_owned());
                    let Some(decoded) = target else { continue };

                    if seen.contains(&decoded) {
                        continue;
                    }

                    let domain = domain_of(&decoded).unwrap_or_default();
                    let count = per_domain.get(&domain).copied().unwrap_or(0);
                    if count >= max_per_domain {
                        continue;
                    }

                    let title = html_unescape(title_raw);

                    out.push(SearchResult {
                        url: decoded.clone(),
                        title,
                        snippet: None,
                        domain: domain.clone(),
                        source: Some("duckduckgo".to_string()),
                    });

                    seen.insert(decoded);
                    per_domain.insert(domain, count + 1);

                    if out.len() >= limit {
                        break;
                    }
                }
            }
        }

        out
    }

    fn max_per_domain(safe_mode: &str) -> usize {
        match safe_mode {
            "off" => usize::MAX,
            "moderate" => 3,
            _ => 2,
        }
    }
}

#[async_trait]
impl SearchBackend for DuckDuckGoBackend {
    fn engine(&self) -> SearchEngine {
        SearchEngine::DuckDuckGo
    }

    fn is_available(&self) -> bool {
        true // Always available
    }

    async fn search(
        &self,
        query: &str,
        max_results: usize,
        time_range: Option<&str>,
        safe_mode: &str,
        timeout_ms: u64,
    ) -> Result<Vec<SearchResult>, String> {
        let mut url = Url::parse(&Self::ddg_base()).map_err(|e| e.to_string())?;
        {
            let mut pairs = url.query_pairs_mut();
            pairs.append_pair("q", query);
            if let Some(range) = time_range {
                let df = match range {
                    "day" => Some("d"),
                    "week" => Some("w"),
                    "month" => Some("m"),
                    "year" => Some("y"),
                    _ => None,
                };
                if let Some(df_val) = df {
                    pairs.append_pair("df", df_val);
                }
            }
        }

        let client = Client::builder()
            .user_agent(user_agent())
            .redirect(RedirectPolicy::limited(5))
            .timeout(Duration::from_millis(timeout_ms))
            .build()
            .map_err(|e| e.to_string())?;

        let resp = client.get(url).send().await.map_err(|e| e.to_string())?;

        if !resp.status().is_success() {
            return Err(format!("HTTP {}", resp.status().as_u16()));
        }

        let body = resp.text().await.map_err(|e| e.to_string())?;

        // Check for CAPTCHA
        if body.contains("anomaly-modal")
            || body.contains("Please complete the following challenge")
        {
            return Err("CAPTCHA_REQUIRED".to_string());
        }

        let results = Self::parse_results(&body, max_results, Self::max_per_domain(safe_mode));
        Ok(results)
    }
}

// ============================================================================
// Brave Search Backend
// ============================================================================

struct BraveBackend {
    api_key: String,
}

impl BraveBackend {
    fn new() -> Option<Self> {
        std::env::var("BRAVE_API_KEY")
            .ok()
            .map(|key| Self { api_key: key })
    }
}

#[async_trait]
impl SearchBackend for BraveBackend {
    fn engine(&self) -> SearchEngine {
        SearchEngine::Brave
    }

    fn is_available(&self) -> bool {
        !self.api_key.is_empty()
    }

    async fn search(
        &self,
        query: &str,
        max_results: usize,
        time_range: Option<&str>,
        _safe_mode: &str,
        timeout_ms: u64,
    ) -> Result<Vec<SearchResult>, String> {
        let mut url = Url::parse("https://api.search.brave.com/res/v1/web/search")
            .map_err(|e| e.to_string())?;
        {
            let mut pairs = url.query_pairs_mut();
            pairs.append_pair("q", query);
            pairs.append_pair("count", &max_results.min(20).to_string());

            // Brave uses freshness parameter
            if let Some(range) = time_range {
                let freshness = match range {
                    "day" => Some("pd"),   // past day
                    "week" => Some("pw"),  // past week
                    "month" => Some("pm"), // past month
                    "year" => Some("py"),  // past year
                    _ => None,
                };
                if let Some(f) = freshness {
                    pairs.append_pair("freshness", f);
                }
            }
        }

        let client = Client::builder()
            .timeout(Duration::from_millis(timeout_ms))
            .build()
            .map_err(|e| e.to_string())?;

        let resp = client
            .get(url)
            .header("X-Subscription-Token", &self.api_key)
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|e| e.to_string())?;

        if !resp.status().is_success() {
            return Err(format!("HTTP {}", resp.status().as_u16()));
        }

        let json: Value = resp.json().await.map_err(|e| e.to_string())?;

        // Parse Brave API response
        let mut results = Vec::new();
        if let Some(web) = json
            .get("web")
            .and_then(|w| w.get("results"))
            .and_then(|r| r.as_array())
        {
            for item in web.iter().take(max_results) {
                let url = item
                    .get("url")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let title = item
                    .get("title")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let snippet = item
                    .get("description")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                if url.is_empty() {
                    continue;
                }

                let domain = domain_of(&url).unwrap_or_default();

                results.push(SearchResult {
                    url,
                    title,
                    snippet,
                    domain,
                    source: Some("brave".to_string()),
                });
            }
        }

        Ok(results)
    }
}

// ============================================================================
// SearXNG Backend
// ============================================================================

struct SearXNGBackend {
    base_url: String,
}

impl SearXNGBackend {
    fn new() -> Option<Self> {
        std::env::var("SEARXNG_URL")
            .ok()
            .map(|url| Self { base_url: url })
    }
}

#[async_trait]
impl SearchBackend for SearXNGBackend {
    fn engine(&self) -> SearchEngine {
        SearchEngine::SearXNG
    }

    fn is_available(&self) -> bool {
        !self.base_url.is_empty()
    }

    async fn search(
        &self,
        query: &str,
        max_results: usize,
        time_range: Option<&str>,
        _safe_mode: &str,
        timeout_ms: u64,
    ) -> Result<Vec<SearchResult>, String> {
        let mut url = Url::parse(&format!("{}/search", self.base_url.trim_end_matches('/')))
            .map_err(|e| e.to_string())?;
        {
            let mut pairs = url.query_pairs_mut();
            pairs.append_pair("q", query);
            pairs.append_pair("format", "json");
            pairs.append_pair("pageno", "1");

            // SearXNG time range
            if let Some(range) = time_range {
                let tr = match range {
                    "day" => Some("day"),
                    "week" => Some("week"),
                    "month" => Some("month"),
                    "year" => Some("year"),
                    _ => None,
                };
                if let Some(t) = tr {
                    pairs.append_pair("time_range", t);
                }
            }
        }

        let client = Client::builder()
            .user_agent(user_agent())
            .timeout(Duration::from_millis(timeout_ms))
            .build()
            .map_err(|e| e.to_string())?;

        let resp = client
            .get(url)
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|e| e.to_string())?;

        if !resp.status().is_success() {
            return Err(format!("HTTP {}", resp.status().as_u16()));
        }

        let json: Value = resp.json().await.map_err(|e| e.to_string())?;

        // Parse SearXNG response
        let mut results = Vec::new();
        if let Some(items) = json.get("results").and_then(|r| r.as_array()) {
            for item in items.iter().take(max_results) {
                let url = item
                    .get("url")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let title = item
                    .get("title")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let snippet = item
                    .get("content")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                if url.is_empty() {
                    continue;
                }

                let domain = domain_of(&url).unwrap_or_default();

                results.push(SearchResult {
                    url,
                    title,
                    snippet,
                    domain,
                    source: Some("searxng".to_string()),
                });
            }
        }

        Ok(results)
    }
}

// ============================================================================
// Search Orchestrator
// ============================================================================

struct SearchOrchestrator {
    backends: Vec<Box<dyn SearchBackend>>,
}

impl SearchOrchestrator {
    fn new() -> Self {
        let mut backends: Vec<Box<dyn SearchBackend>> = Vec::new();

        // Add available backends
        backends.push(Box::new(DuckDuckGoBackend));

        if let Some(brave) = BraveBackend::new() {
            backends.push(Box::new(brave));
        }

        if let Some(searxng) = SearXNGBackend::new() {
            backends.push(Box::new(searxng));
        }

        Self { backends }
    }

    fn get_backend(&self, engine: SearchEngine) -> Option<&dyn SearchBackend> {
        self.backends
            .iter()
            .find(|b| b.engine() == engine && b.is_available())
            .map(|b| b.as_ref())
    }

    async fn search_with_fallback(
        &self,
        query: &str,
        max_results: usize,
        time_range: Option<&str>,
        safe_mode: &str,
        timeout_ms: u64,
        preferred_engine: SearchEngine,
        max_retries: u32,
    ) -> SearchAttempt {
        let engines = if preferred_engine == SearchEngine::Auto {
            SearchEngine::auto_order()
        } else {
            vec![preferred_engine]
        };

        let mut last_error: Option<String> = None;
        let start = Instant::now();

        for engine in engines {
            let Some(backend) = self.get_backend(engine) else {
                continue;
            };

            let mut attempt = 0u32;
            while attempt < max_retries {
                if attempt > 0 {
                    tokio::time::sleep(retry_delay(attempt - 1)).await;
                }

                match backend
                    .search(query, max_results, time_range, safe_mode, timeout_ms)
                    .await
                {
                    Ok(results) => {
                        return SearchAttempt {
                            results,
                            engine,
                            success: true,
                            error: None,
                            elapsed_ms: start.elapsed().as_millis() as u64,
                            retry_count: attempt,
                        };
                    }
                    Err(e) => {
                        last_error = Some(e.clone());
                        // Don't retry on CAPTCHA - fallback to next engine
                        if e == "CAPTCHA_REQUIRED" {
                            warn!(target: "mcp.search", engine = %engine.as_str(), "CAPTCHA detected, trying next engine");
                            break;
                        }
                        attempt += 1;
                    }
                }
            }
        }

        SearchAttempt {
            results: Vec::new(),
            engine: preferred_engine,
            success: false,
            error: last_error,
            elapsed_ms: start.elapsed().as_millis() as u64,
            retry_count: 0,
        }
    }
}

// ============================================================================
// Deduplication
// ============================================================================

fn deduplicate_results(results: Vec<SearchResult>) -> Vec<SearchResult> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut deduped = Vec::new();

    for result in results {
        // Normalize URL for comparison
        let normalized = normalize_url(&result.url);
        if seen.contains(&normalized) {
            continue;
        }
        seen.insert(normalized);
        deduped.push(result);
    }

    deduped
}

fn normalize_url(url: &str) -> String {
    if let Ok(mut parsed) = Url::parse(url) {
        // Remove trailing slash
        let path = parsed.path().trim_end_matches('/').to_string();
        parsed.set_path(&path);
        // Remove common tracking params
        let query: Vec<(String, String)> = parsed
            .query_pairs()
            .filter(|(k, _)| {
                !["utm_source", "utm_medium", "utm_campaign", "ref"].contains(&k.as_ref())
            })
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        if query.is_empty() {
            parsed.set_query(None);
        } else {
            parsed.set_query(Some(
                &query
                    .iter()
                    .map(|(k, v)| format!("{}={}", k, v))
                    .collect::<Vec<_>>()
                    .join("&"),
            ));
        }
        parsed.to_string().to_lowercase()
    } else {
        url.to_lowercase()
    }
}

// ============================================================================
// MCP Tool
// ============================================================================

pub struct SearchWebTool {
    engine: String,
}

impl SearchWebTool {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            engine: "multi".to_string(),
        })
    }

    /// Alias for new() - used by lib.rs
    pub fn new_default() -> Arc<Self> {
        Self::new()
    }

    fn query_firewall_blocked(q: &str) -> Option<&'static str> {
        let low = q.to_lowercase();
        if low.contains("file://") {
            return Some("FILE_PROTOCOL");
        }
        if low.contains("s3://") || low.contains("gs://") {
            return Some("CLOUD_STORAGE_PROTOCOL");
        }
        // Block obvious internal IPs
        let internal_patterns = [
            "10.",
            "192.168.",
            "172.16.",
            "172.17.",
            "127.0.0.1",
            "localhost",
        ];
        for p in internal_patterns {
            if low.contains(p) {
                return Some("INTERNAL_ADDRESS");
            }
        }
        // Block JWT / Bearer tokens
        if low.contains("eyjh") || low.contains("bearer ") {
            return Some("SECRET_LIKE_TOKEN");
        }
        None
    }

    fn max_retries() -> u32 {
        std::env::var("DEVIT_SEARCH_MAX_RETRIES")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(3)
    }
}

#[async_trait]
impl McpTool for SearchWebTool {
    fn name(&self) -> &str {
        "devit_search_web"
    }

    fn description(&self) -> &str {
        "Search the web (SERP) via DuckDuckGo HTML with safety guards"
    }

    async fn execute(&self, params: Value) -> McpResult<Value> {
        let query = params
            .get("query")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| McpError::InvalidRequest("'query' is required".into()))?;

        if let Some(code) = Self::query_firewall_blocked(query) {
            return Err(McpError::rpc(
                -32600,
                "Query blocked by safety policy",
                Some(json!({
                    "code": code,
                    "message": "The provided query is not allowed",
                    "hint": "Remove internal URLs, private IPs or secret-like tokens"
                })),
            ));
        }

        let max_results = params
            .get("max_results")
            .and_then(Value::as_u64)
            .map(|n| n.min(20).max(1) as usize)
            .unwrap_or(5);
        let include_snippets = params
            .get("include_snippets")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let time_range = params.get("time_range").and_then(Value::as_str);
        let preferred_engine = params
            .get("engine")
            .and_then(Value::as_str)
            .map(SearchEngine::from_str)
            .unwrap_or(SearchEngine::Auto);
        let (effective_limits, limit_sources): (EffectiveLimits, LimitSources) =
            resolve_search_limits(params.get("timeout_ms").and_then(Value::as_u64));
        let timeout_ms = effective_limits.timeout_ms;
        let safe_mode = params
            .get("safe_mode")
            .and_then(Value::as_str)
            .unwrap_or("strict");

        let trace_id = Uuid::new_v4().to_string();
        let start = Instant::now();

        // Compute cache key
        let cache_query = format!(
            "{}|engine={}|df={}",
            query,
            preferred_engine.as_str(),
            time_range.unwrap_or("none")
        );
        let cache_key_val = cache_key(&cache_query, "text/html", &user_agent(), safe_mode, true);

        // Check cache
        let cached = {
            let cache = SEARCH_CACHE.read();
            cache.get(&cache_key_val)
        };

        let (results, from_cache, used_engine, error_msg, retry_count) =
            if let Some((cached_results, cached_engine)) = cached {
                info!(
                    target: "mcp.search",
                    %trace_id,
                    engine = %cached_engine.as_str(),
                    results = cached_results.len(),
                    cache = "hit",
                    "cache hit"
                );
                (cached_results, true, cached_engine, None, 0u32)
            } else {
                let orchestrator = SearchOrchestrator::new();
                let attempt = orchestrator
                    .search_with_fallback(
                        query,
                        max_results,
                        time_range,
                        safe_mode,
                        timeout_ms,
                        preferred_engine,
                        Self::max_retries(),
                    )
                    .await;

                if attempt.success {
                    // Cache results
                    let mut cache = SEARCH_CACHE.write();
                    cache.insert(
                        cache_key_val.clone(),
                        attempt.results.clone(),
                        attempt.engine,
                    );
                }

                info!(
                    target: "mcp.search",
                    %trace_id,
                    engine = %attempt.engine.as_str(),
                    results = attempt.results.len(),
                    success = attempt.success,
                    elapsed_ms = %attempt.elapsed_ms,
                    retries = %attempt.retry_count,
                    "search completed"
                );

                (
                    attempt.results,
                    false,
                    attempt.engine,
                    attempt.error,
                    attempt.retry_count,
                )
            };

        // Deduplicate results
        let results = deduplicate_results(results);

        // Convert to JSON
        let results_json: Vec<Value> = results
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let mut obj = json!({
                    "rank": i + 1,
                    "title": r.title,
                    "url": r.url,
                    "domain": r.domain
                });
                if include_snippets {
                    if let Some(ref snippet) = r.snippet {
                        obj["snippet"] = json!(snippet);
                    }
                }
                if let Some(ref source) = r.source {
                    obj["source"] = json!(source);
                }
                obj
            })
            .collect();

        let elapsed_ms = start.elapsed().as_millis() as u64;
        let retrieved_at = Utc::now().to_rfc3339();

        let meta = json!({
            "engine_requested": preferred_engine.as_str(),
            "engine_used": used_engine.as_str(),
            "trace_id": trace_id,
            "partial": error_msg.is_some(),
            "from_cache": from_cache,
            "elapsed_ms": elapsed_ms,
            "retry_count": retry_count,
            "time_range": time_range,
            "error": error_msg,
            "effective_limits": effective_limits,
            "limit_sources": limit_sources,
            "cache_key": cache_key_val,
            "available_engines": SearchEngine::auto_order().iter().map(|e| e.as_str()).collect::<Vec<_>>()
        });

        jbe::append(
            "search",
            &json!({
                "query": query,
                "retrieved_at": retrieved_at,
                "results_count": results_json.len(),
                "meta": meta
            }),
        );

        // Build summary
        let mut summary_parts = vec![
            format!("Query: '{}'", query),
            format!("Results: {}", results_json.len()),
            format!("Engine: {}", used_engine.as_str()),
        ];
        if from_cache {
            summary_parts.push("(cached)".to_string());
        }
        if let Some(range) = time_range {
            summary_parts.push(format!("Time: {}", range));
        }
        if let Some(ref err) = error_msg {
            summary_parts.push(format!("Warning: {}", err));
        }

        Ok(json!({
            "content": [
                {
                    "type": "text",
                    "text": summary_parts.join(" | ")
                }
            ],
            "metadata": {
                "query": query,
                "retrieved_at": retrieved_at,
                "results": results_json,
                "meta": meta
            }
        }))
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "minLength": 1,
                    "description": "Search query string"
                },
                "max_results": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 20,
                    "default": 5,
                    "description": "Maximum number of results to return"
                },
                "engine": {
                    "type": "string",
                    "enum": ["auto", "duckduckgo", "brave", "searxng"],
                    "default": "auto",
                    "description": "Search engine to use. 'auto' tries available engines with fallback."
                },
                "include_snippets": {
                    "type": "boolean",
                    "default": true,
                    "description": "Include snippet/description for each result"
                },
                "time_range": {
                    "type": "string",
                    "enum": ["day", "week", "month", "year"],
                    "description": "Filter results by time range"
                },
                "timeout_ms": {
                    "type": "integer",
                    "minimum": 100,
                    "maximum": 10000,
                    "description": "Request timeout in milliseconds"
                },
                "safe_mode": {
                    "type": "string",
                    "enum": ["strict", "moderate", "off"],
                    "default": "strict",
                    "description": "Safety mode for domain diversity"
                }
            },
            "required": ["query"]
        })
    }
}

// ============================================================================
// Helpers
// ============================================================================

fn html_unescape(s: &str) -> String {
    let mut out = s
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">");
    out = out.replace("&quot;", "\"").replace("&#39;", "'");
    if let Ok(re) = Regex::new(r"<[^>]+>") {
        out = re.replace_all(&out, "").to_string();
    }
    out
}

fn domain_of(u: &str) -> Option<String> {
    let host = if let Ok(p) = Url::parse(u) {
        p.host_str().map(|s| s.to_lowercase())?
    } else {
        u.to_lowercase()
    };

    // eTLD+1 heuristic
    let parts: Vec<&str> = host.split('.').collect();
    if parts.len() < 2 {
        return Some(host);
    }

    // Handle known multi-part TLDs
    let multi_tlds = ["co.uk", "com.au", "co.nz", "co.jp", "com.br", "co.in"];
    for mtld in multi_tlds {
        if host.ends_with(mtld) {
            let suffix_parts = mtld.split('.').count();
            if parts.len() > suffix_parts {
                return Some(parts[parts.len() - suffix_parts - 1..].join("."));
            }
            return Some(host);
        }
    }

    // Default: last two parts
    Some(parts[parts.len() - 2..].join("."))
}

fn user_agent() -> String {
    std::env::var("DEVIT_HTTP_USER_AGENT").unwrap_or_else(|_| "DevItBot/1.0".to_string())
}

fn retry_delay(attempt: u32) -> Duration {
    match attempt {
        0 => Duration::from_millis(100),
        1 => Duration::from_millis(500),
        _ => Duration::from_millis(2000),
    }
}

// Expose domain_of for tests
#[cfg(any(test, feature = "test-utils"))]
pub fn __test_domain_of(u: &str) -> Option<String> {
    domain_of(u)
}
