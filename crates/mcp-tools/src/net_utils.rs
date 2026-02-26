// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2025-2026 naskel.com

use regex::Regex;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RobotsPolicy {
    Allow,
    Disallow,
}

/// Very small robots.txt evaluator for User-agent: * block only.
/// Longest-prefix rule; Allow beats Disallow when longer.
pub fn robots_policy_for(path: &str, robots: &str) -> RobotsPolicy {
    let mut in_star = false;
    let mut allows: Vec<String> = Vec::new();
    let mut disallows: Vec<String> = Vec::new();
    for raw in robots.lines() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        if line.to_ascii_lowercase().starts_with("user-agent:") {
            let agent = line
                .splitn(2, ':')
                .nth(1)
                .unwrap_or("")
                .trim()
                .to_ascii_lowercase();
            in_star = agent == "*";
            continue;
        }
        if !in_star {
            continue;
        }
        if line.to_ascii_lowercase().starts_with("allow:") {
            let p = line.splitn(2, ':').nth(1).unwrap_or("").trim().to_string();
            allows.push(p);
        } else if line.to_ascii_lowercase().starts_with("disallow:") {
            let p = line.splitn(2, ':').nth(1).unwrap_or("").trim().to_string();
            disallows.push(p);
        }
    }

    let mut best_dis: Option<&str> = None;
    for d in &disallows {
        if d.is_empty() {
            continue;
        }
        if path.starts_with(d) {
            if best_dis.map(|b| d.len() > b.len()).unwrap_or(true) {
                best_dis = Some(d);
            }
        }
    }
    let mut best_all: Option<&str> = None;
    for a in &allows {
        if path.starts_with(a) {
            if best_all.map(|b| a.len() > b.len()).unwrap_or(true) {
                best_all = Some(a);
            }
        }
    }
    if best_all.is_some() {
        return RobotsPolicy::Allow;
    }
    if best_dis.is_some() {
        RobotsPolicy::Disallow
    } else {
        RobotsPolicy::Allow
    }
}

/// Best-effort sanitizer: strips scripts/styles/noscripts/tags, event handlers, javascript: links.
/// Also decodes common entities and collapses whitespace; capped to 100k chars.
pub fn sanitize_html_to_text(html: &str) -> String {
    let mut out = html.to_string();
    if let Ok(re) = Regex::new(r"(?is)<script[^>]*>.*?</script>") {
        out = re.replace_all(&out, "").to_string();
    }
    if let Ok(re) = Regex::new(r"(?is)<style[^>]*>.*?</style>") {
        out = re.replace_all(&out, "").to_string();
    }
    if let Ok(re) = Regex::new(r"(?is)<noscript[^>]*>.*?</noscript>") {
        out = re.replace_all(&out, "").to_string();
    }
    if let Ok(re) = Regex::new(r#"(?i) on[a-zA-Z]+\s*=\s*(\"[^\"]*\"|'[^']*')"#) {
        out = re.replace_all(&out, "").to_string();
    }
    if let Ok(re) = Regex::new(r#"(?i)href\s*=\s*\"\s*javascript:[^\"]*\""#) {
        out = re.replace_all(&out, "href=\"#\"").to_string();
    }
    if let Ok(re) = Regex::new(r"(?is)<[^>]+>") {
        out = re.replace_all(&out, "").to_string();
    }
    let mut out = out
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'");
    if let Ok(re) = Regex::new(r"\s+") {
        out = re.replace_all(&out, " ").to_string();
    }
    if out.len() > 100_000 {
        out.truncate(100_000);
    }
    out
}

/// Heuristic detection of paywall hints.
pub fn detect_paywall_hint(html: &str) -> bool {
    let low = html.to_lowercase();
    low.contains("paywall") || (low.contains("subscribe") && low.contains("premium"))
}

/// Heuristic detection of prompt-injection text.
pub fn detect_injection_text(text: &str) -> bool {
    let low = text.to_lowercase();
    let mut hits = 0;
    for kw in [
        "ignore previous instructions",
        "system_prompt",
        "tool_call",
        "exfiltrate",
    ] {
        if low.contains(kw) {
            hits += 1;
        }
    }
    hits >= 2
}

/// Extraction mode for content
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ExtractMode {
    /// Return all text content (original behavior)
    #[default]
    Raw,
    /// Extract main article content using heuristics
    Article,
    /// Auto-detect: use Article mode for HTML, Raw for plain text
    Auto,
}

impl ExtractMode {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "article" => ExtractMode::Article,
            "auto" => ExtractMode::Auto,
            _ => ExtractMode::Raw,
        }
    }
}

/// Result of article extraction with quality metrics
#[derive(Debug)]
pub struct ArticleContent {
    /// Extracted text content
    pub text: String,
    /// Title if found
    pub title: Option<String>,
    /// Estimated content quality (0.0 - 1.0)
    pub quality_score: f32,
    /// Whether code blocks were preserved
    pub has_code: bool,
    /// Number of paragraphs extracted
    pub paragraph_count: usize,
}

/// Extract main article content from HTML using Readability-like heuristics.
///
/// Strategy:
/// 1. Find semantic containers: <article>, <main>, [role="main"]
/// 2. Remove boilerplate: <nav>, <header>, <footer>, <aside>, <menu>
/// 3. Preserve code blocks: <pre>, <code>
/// 4. Score paragraphs by text density (text chars vs link chars)
/// 5. Extract paragraphs with good density scores
pub fn extract_article_content(html: &str) -> ArticleContent {
    let mut out = html.to_string();

    // Extract title first
    let title = extract_title(&out);

    // Remove script, style, noscript
    for pattern in [
        r"(?is)<script[^>]*>.*?</script>",
        r"(?is)<style[^>]*>.*?</style>",
        r"(?is)<noscript[^>]*>.*?</noscript>",
    ] {
        if let Ok(re) = Regex::new(pattern) {
            out = re.replace_all(&out, "").to_string();
        }
    }

    // Try to find article/main content container
    let article_content = find_main_content(&out);

    // If we found a main content area, use it; otherwise use full body
    let content_html = if let Some(content) = article_content {
        content
    } else {
        // Fallback: try to extract body
        if let Ok(re) = Regex::new(r"(?is)<body[^>]*>(.*?)</body>") {
            re.captures(&out)
                .and_then(|c| c.get(1))
                .map(|m| m.as_str().to_string())
                .unwrap_or(out.clone())
        } else {
            out.clone()
        }
    };

    // Remove navigation/boilerplate elements
    let mut clean = content_html;
    for pattern in [
        r"(?is)<nav[^>]*>.*?</nav>",
        r"(?is)<header[^>]*>.*?</header>",
        r"(?is)<footer[^>]*>.*?</footer>",
        r"(?is)<aside[^>]*>.*?</aside>",
        r"(?is)<menu[^>]*>.*?</menu>",
        r#"(?is)<div[^>]*class="[^"]*(?:sidebar|menu|nav|footer|header|ad|advertisement|banner|popup|modal|cookie)[^"]*"[^>]*>.*?</div>"#,
    ] {
        if let Ok(re) = Regex::new(pattern) {
            clean = re.replace_all(&clean, "").to_string();
        }
    }

    // Preserve code blocks by marking them
    let mut code_blocks: Vec<String> = Vec::new();
    if let Ok(re) = Regex::new(r"(?is)<pre[^>]*>.*?</pre>") {
        for cap in re.find_iter(&clean) {
            let block = cap.as_str();
            // Extract just the text from the code block
            let code_text = sanitize_html_to_text(block);
            code_blocks.push(format!("\n```\n{}\n```\n", code_text.trim()));
        }
    }

    // Extract paragraphs with quality scoring
    let paragraphs = extract_quality_paragraphs(&clean);

    // Build final text
    let mut result_parts: Vec<String> = Vec::new();

    // Add title if found
    if let Some(ref t) = title {
        result_parts.push(format!("# {}\n", t));
    }

    // Add paragraphs
    for para in &paragraphs {
        if !para.text.is_empty() {
            result_parts.push(para.text.clone());
        }
    }

    // Insert code blocks (they were removed during paragraph extraction)
    // For simplicity, append them at the end
    for block in &code_blocks {
        if !result_parts.iter().any(|p| p.contains(block.trim())) {
            result_parts.push(block.clone());
        }
    }

    let text = result_parts.join("\n\n");

    // Calculate quality score
    let total_chars = text.chars().count() as f32;
    let quality_score = if total_chars > 500.0 {
        (paragraphs.iter().map(|p| p.score).sum::<f32>() / paragraphs.len().max(1) as f32).min(1.0)
    } else if total_chars > 100.0 {
        0.5
    } else {
        0.2
    };

    // Truncate if too long
    let text = if text.len() > 100_000 {
        text[..100_000].to_string()
    } else {
        text
    };

    ArticleContent {
        text,
        title,
        quality_score,
        has_code: !code_blocks.is_empty(),
        paragraph_count: paragraphs.len(),
    }
}

/// Extract page title from HTML
fn extract_title(html: &str) -> Option<String> {
    // Try <title> tag
    if let Ok(re) = Regex::new(r"(?is)<title[^>]*>(.*?)</title>") {
        if let Some(cap) = re.captures(html) {
            let title = cap.get(1).map(|m| m.as_str()).unwrap_or("");
            let title = decode_entities(title).trim().to_string();
            if !title.is_empty() {
                return Some(title);
            }
        }
    }

    // Try <h1>
    if let Ok(re) = Regex::new(r"(?is)<h1[^>]*>(.*?)</h1>") {
        if let Some(cap) = re.captures(html) {
            let title = cap.get(1).map(|m| m.as_str()).unwrap_or("");
            let title = strip_tags(title).trim().to_string();
            if !title.is_empty() && title.len() < 200 {
                return Some(title);
            }
        }
    }

    None
}

/// Find main content area in HTML
fn find_main_content(html: &str) -> Option<String> {
    // Priority order: article, main, [role="main"], .content, .post, .entry
    let patterns = [
        r"(?is)<article[^>]*>(.*?)</article>",
        r"(?is)<main[^>]*>(.*?)</main>",
        r#"(?is)<[^>]+role\s*=\s*["']main["'][^>]*>(.*?)</[^>]+>"#,
        r#"(?is)<div[^>]*class="[^"]*(?:content|post|entry|article)[^"]*"[^>]*>(.*?)</div>"#,
    ];

    for pattern in patterns {
        if let Ok(re) = Regex::new(pattern) {
            if let Some(cap) = re.captures(html) {
                if let Some(m) = cap.get(1) {
                    let content = m.as_str();
                    // Only use if it has substantial content
                    if content.len() > 200 {
                        return Some(content.to_string());
                    }
                }
            }
        }
    }

    None
}

/// A scored paragraph
struct ScoredParagraph {
    text: String,
    score: f32,
}

/// Extract paragraphs and score them by text density
fn extract_quality_paragraphs(html: &str) -> Vec<ScoredParagraph> {
    let mut paragraphs = Vec::new();

    // Extract all paragraph-like elements
    let patterns = [
        r"(?is)<p[^>]*>(.*?)</p>",
        r"(?is)<li[^>]*>(.*?)</li>",
        r"(?is)<h[1-6][^>]*>(.*?)</h[1-6]>",
        r"(?is)<blockquote[^>]*>(.*?)</blockquote>",
    ];

    for pattern in patterns {
        if let Ok(re) = Regex::new(pattern) {
            for cap in re.captures_iter(html) {
                if let Some(m) = cap.get(1) {
                    let raw = m.as_str();
                    let text = strip_tags(raw).trim().to_string();

                    if text.len() < 20 {
                        continue; // Skip very short paragraphs
                    }

                    // Calculate text density score
                    let score = calculate_text_density(raw, &text);

                    if score > 0.3 {
                        // Only keep paragraphs with reasonable density
                        paragraphs.push(ScoredParagraph { text, score });
                    }
                }
            }
        }
    }

    paragraphs
}

/// Calculate text density: ratio of text chars to total chars including links
fn calculate_text_density(html: &str, text: &str) -> f32 {
    let text_len = text.chars().count() as f32;
    if text_len == 0.0 {
        return 0.0;
    }

    // Count link text
    let mut link_len = 0usize;
    if let Ok(re) = Regex::new(r"(?is)<a[^>]*>(.*?)</a>") {
        for cap in re.captures_iter(html) {
            if let Some(m) = cap.get(1) {
                link_len += strip_tags(m.as_str()).chars().count();
            }
        }
    }

    // Density = (text - links) / text
    // High density = mostly text, low density = mostly links (navigation)
    let non_link_ratio = (text_len - link_len as f32) / text_len;

    // Bonus for longer paragraphs (more likely to be content)
    let length_bonus = (text_len / 500.0).min(0.3);

    (non_link_ratio + length_bonus).min(1.0)
}

/// Strip HTML tags from text
fn strip_tags(html: &str) -> String {
    if let Ok(re) = Regex::new(r"<[^>]+>") {
        decode_entities(&re.replace_all(html, ""))
    } else {
        decode_entities(html)
    }
}

/// Decode common HTML entities
fn decode_entities(text: &str) -> String {
    text.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
        .replace("&#x27;", "'")
        .replace("&#x2F;", "/")
}
