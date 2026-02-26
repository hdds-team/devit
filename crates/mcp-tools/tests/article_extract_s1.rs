// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2025-2026 naskel.com

#![cfg(feature = "test-utils")]

use mcp_tools::test_helpers::*;

#[test]
fn extract_article_finds_main_content() {
    let html = r#"
        <!DOCTYPE html>
        <html>
        <head><title>Test Article</title></head>
        <body>
            <nav><a href="/">Home</a> | <a href="/about">About</a></nav>
            <article>
                <h1>Main Heading</h1>
                <p>This is the first paragraph of the article. It contains substantial content that should be extracted.</p>
                <p>This is the second paragraph with more meaningful content for the reader to consume.</p>
            </article>
            <footer>Copyright 2025</footer>
        </body>
        </html>
    "#;

    let result = extract_article_content(html);

    // Should extract title
    assert!(result.title.is_some());
    assert!(result.title.as_ref().unwrap().contains("Test Article"));

    // Should extract article content
    assert!(result.text.contains("first paragraph"));
    assert!(result.text.contains("second paragraph"));

    // Should NOT contain nav or footer
    assert!(!result.text.contains("Home"));
    assert!(!result.text.contains("Copyright"));

    // Quality score should be reasonable
    assert!(result.quality_score > 0.3);
}

#[test]
fn extract_article_preserves_code_blocks() {
    let html = r#"
        <html>
        <head><title>Code Tutorial</title></head>
        <body>
            <article>
                <p>Here is some code:</p>
                <pre><code>fn main() {
    println!("Hello, world!");
}</code></pre>
                <p>That was Rust code.</p>
            </article>
        </body>
        </html>
    "#;

    let result = extract_article_content(html);

    // Should preserve code
    assert!(result.has_code);
    assert!(result.text.contains("fn main()"));
    assert!(result.text.contains("println!"));
}

#[test]
fn extract_article_handles_plain_body() {
    let html = r#"
        <html>
        <head><title>Simple Page</title></head>
        <body>
            <p>This is a simple page without article tags but with meaningful paragraph content.</p>
            <p>It should still extract the text properly from the body element.</p>
        </body>
        </html>
    "#;

    let result = extract_article_content(html);

    assert!(result.text.contains("simple page"));
    assert!(result.paragraph_count >= 1);
}

#[test]
fn extract_mode_from_str_works() {
    assert_eq!(ExtractMode::from_str("article"), ExtractMode::Article);
    assert_eq!(ExtractMode::from_str("ARTICLE"), ExtractMode::Article);
    assert_eq!(ExtractMode::from_str("auto"), ExtractMode::Auto);
    assert_eq!(ExtractMode::from_str("raw"), ExtractMode::Raw);
    assert_eq!(ExtractMode::from_str("unknown"), ExtractMode::Raw);
}
