// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2025-2026 naskel.com

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Parsed tool call from LLM response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub name: String,
    pub arguments: Value,
}

/// Tool calling mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolCallingMode {
    /// Parse XML-style tags from response content
    Prompt,
    /// Backend provides native tool_calls
    Native,
}

/// DevIt format parser - parses XML-style tool calls from LLM response
///
/// Expected format:
/// ```xml
/// <tool_call>
/// <tool_name>file_read</tool_name>
/// <arguments>
/// {"path": "src/main.rs"}
/// </arguments>
/// </tool_call>
/// ```
pub struct DevItFormatParser;

impl DevItFormatParser {
    /// Parse tool calls from LLM response content
    pub fn parse(content: &str) -> Result<Vec<ToolCall>> {
        let mut tool_calls = Vec::new();

        // Clean markdown code blocks first (```xml, ```plaintext, etc.)
        let cleaned = Self::strip_markdown_blocks(content);

        // Try to find <tool_call>...</tool_call> blocks first
        let mut start = 0;
        while let Some(block_start) = cleaned[start..].find("<tool_call>") {
            let block_start = start + block_start;
            if let Some(block_end_rel) = cleaned[block_start..].find("</tool_call>") {
                let block_end = block_start + block_end_rel + "</tool_call>".len();
                let block = &cleaned[block_start..block_end];

                // Skip blocks that look like hallucinated tool results (context contamination)
                // These contain patterns like {"content": [...], "text": ...} instead of proper tool calls
                if Self::looks_like_tool_result(block) {
                    start = block_end;
                    continue;
                }

                if let Some(tool_call) = Self::parse_single_block(block) {
                    tool_calls.push(tool_call);
                }

                start = block_end;
            } else {
                break;
            }
        }

        // If no <tool_call> blocks found, try to find standalone <tool_name> tags
        // (some models forget the <tool_call> wrapper)
        if tool_calls.is_empty() {
            let mut start = 0;
            while let Some(name_start) = cleaned[start..].find("<tool_name>") {
                let name_start = start + name_start;

                // Find the end of this tool call block
                let block_end =
                    if let Some(args_end_rel) = cleaned[name_start..].find("</arguments>") {
                        name_start + args_end_rel + "</arguments>".len()
                    } else if let Some(name_end_rel) = cleaned[name_start..].find("</tool_name>") {
                        // No arguments, just tool_name
                        name_start + name_end_rel + "</tool_name>".len()
                    } else {
                        break;
                    };

                // Create a fake block with tool_call wrapper
                let fake_block =
                    format!("<tool_call>{}</tool_call>", &cleaned[name_start..block_end]);

                if let Some(tool_call) = Self::parse_single_block(&fake_block) {
                    tool_calls.push(tool_call);
                }

                start = block_end;
            }
        }

        // If still no tool calls found, try JSON format
        // Format: {"name": "tool_name", "arguments": {...}}
        if tool_calls.is_empty() {
            if let Some(call) = Self::try_parse_json_format(&cleaned) {
                tool_calls.push(call);
            }
        }

        // Try GLM-style format: <tool_name><param>value</param></tool_name>
        // e.g., <file_read><path>main.rs</path></file_read>
        if tool_calls.is_empty() {
            tool_calls.extend(Self::parse_glm_style(&cleaned));
        }

        Ok(tool_calls)
    }

    /// Parse GLM-style tool calls: <tool_name><param>value</param></tool_name>
    fn parse_glm_style(content: &str) -> Vec<ToolCall> {
        let mut calls = Vec::new();

        // Common tool names to look for
        let tool_names = [
            "file_read",
            "file_write",
            "file_search",
            "file_list",
            "shell",
            "exec",
            "search_web",
            "fetch_url",
            "git_log",
            "git_diff",
            "git_show",
            "git_blame",
            "git_search",
            "project_structure",
            "directory_list",
            "explorer",
            "patch_apply",
            "snapshot",
            "ocr",
            "screenshot",
        ];

        for tool_name in tool_names {
            let open_tag = format!("<{}>", tool_name);
            let close_tag = format!("</{}>", tool_name);

            let mut pos = 0;
            while let Some(start) = content[pos..].find(&open_tag) {
                let start = pos + start;
                if let Some(end_rel) = content[start..].find(&close_tag) {
                    let end = start + end_rel + close_tag.len();
                    let inner = &content[start + open_tag.len()..start + end_rel];

                    // Parse inner XML params into JSON
                    if let Some(args) = Self::parse_xml_params(inner) {
                        calls.push(ToolCall {
                            name: tool_name.to_string(),
                            arguments: args,
                        });
                    }
                    pos = end;
                } else {
                    break;
                }
            }
        }

        calls
    }

    /// Parse XML-style params: <param>value</param> into JSON object
    fn parse_xml_params(content: &str) -> Option<Value> {
        use std::collections::HashMap;
        let mut params: HashMap<String, Value> = HashMap::new();

        let mut pos = 0;
        while pos < content.len() {
            // Find opening tag
            if let Some(tag_start) = content[pos..].find('<') {
                let tag_start = pos + tag_start;
                if let Some(tag_end) = content[tag_start..].find('>') {
                    let tag_end = tag_start + tag_end;
                    let tag_name = &content[tag_start + 1..tag_end];

                    // Skip if it's a closing tag
                    if tag_name.starts_with('/') {
                        pos = tag_end + 1;
                        continue;
                    }

                    // Find closing tag
                    let close_tag = format!("</{}>", tag_name);
                    if let Some(close_start) = content[tag_end + 1..].find(&close_tag) {
                        let value_start = tag_end + 1;
                        let value_end = tag_end + 1 + close_start;
                        let value = content[value_start..value_end].trim();

                        // Try to parse as JSON, otherwise use as string
                        let json_value = serde_json::from_str(value)
                            .unwrap_or_else(|_| Value::String(value.to_string()));
                        params.insert(tag_name.to_string(), json_value);

                        pos = value_end + close_tag.len();
                    } else {
                        pos = tag_end + 1;
                    }
                } else {
                    break;
                }
            } else {
                break;
            }
        }

        if params.is_empty() {
            None
        } else {
            Some(Value::Object(params.into_iter().collect()))
        }
    }

    /// Try to parse JSON format tool call: {"name": "...", "arguments": {...}}
    fn try_parse_json_format(content: &str) -> Option<ToolCall> {
        // Look for JSON object with "name" and "arguments" fields
        // Try to find a JSON object in the content
        let mut brace_start = None;
        let mut brace_count = 0;

        for (i, c) in content.char_indices() {
            match c {
                '{' => {
                    if brace_start.is_none() {
                        brace_start = Some(i);
                    }
                    brace_count += 1;
                }
                '}' => {
                    brace_count -= 1;
                    if brace_count == 0 {
                        let Some(start) = brace_start else { continue };
                        let json_str = &content[start..=i];
                        if let Ok(obj) = serde_json::from_str::<Value>(json_str) {
                            // Check for {"name": "...", "arguments": {...}} format
                            if let (Some(name), Some(args)) = (
                                obj.get("name").and_then(|v| v.as_str()),
                                obj.get("arguments"),
                            ) {
                                return Some(ToolCall {
                                    name: name.to_string(),
                                    arguments: args.clone(),
                                });
                            }
                        }
                        // Reset and try to find another JSON object
                        brace_start = None;
                    }
                }
                _ => {}
            }
        }
        None
    }

    /// Detect if a block looks like a hallucinated tool result instead of a real tool call
    /// This happens when the model is contaminated by seeing tool results in context
    fn looks_like_tool_result(block: &str) -> bool {
        // Real tool calls have <tool_name> tags
        // Hallucinated results have patterns like:
        // - "content": [ (MCP result format)
        // - "text": " (text content in result)
        // - "File written successfully" (success messages)
        // - "metadata": { (result metadata)
        let has_tool_name = block.contains("<tool_name>");
        let has_result_patterns = block.contains("\"content\": [")
            || block.contains("\"text\":")
            || block.contains("File written")
            || block.contains("\"metadata\":")
            || block.contains("File written successfully")
            || block.contains("bytes_written");

        // If it has result patterns but no proper tool_name tag, it's a hallucinated result
        !has_tool_name && has_result_patterns
    }

    /// Remove markdown code blocks (```xml, ```plaintext, etc.)
    fn strip_markdown_blocks(content: &str) -> String {
        let mut result = content.to_string();

        // Remove opening code blocks: ```xml, ```plaintext, ```
        result = result.replace("```xml\n", "");
        result = result.replace("```plaintext\n", "");
        result = result.replace("```\n", "");
        result = result.replace("```xml", "");
        result = result.replace("```plaintext", "");
        result = result.replace("```", "");

        result
    }

    fn parse_single_block(block: &str) -> Option<ToolCall> {
        // Try standard format first: <tool_call><tool_name>NAME</tool_name><arguments>...</arguments></tool_call>
        if let (Some(name_start), Some(name_end)) =
            (block.find("<tool_name>"), block.find("</tool_name>"))
        {
            let name = block[name_start + "<tool_name>".len()..name_end]
                .trim()
                .to_string();
            let arguments = Self::extract_arguments(block, &name);
            return Some(ToolCall { name, arguments });
        }

        // Try GLM/compact format: <tool_call>NAME<arguments>...</arguments></tool_call>
        // The tool name is directly after <tool_call> and before <arguments>
        if let Some(args_start) = block.find("<arguments>") {
            let after_tag = block
                .find("<tool_call>")
                .map(|i| i + "<tool_call>".len())
                .unwrap_or(0);
            let name = block[after_tag..args_start].trim().to_string();
            if !name.is_empty() && !name.contains('<') {
                let arguments = Self::extract_arguments(block, &name);
                return Some(ToolCall { name, arguments });
            }
        }

        None
    }

    /// Extract arguments from a tool call block
    fn extract_arguments(block: &str, tool_name: &str) -> Value {
        if let (Some(args_start), Some(args_end)) =
            (block.find("<arguments>"), block.find("</arguments>"))
        {
            let args_str = block[args_start + "<arguments>".len()..args_end].trim();
            // Parse JSON arguments - with fallback for malformed JSON
            match serde_json::from_str::<Value>(args_str) {
                Ok(val) => val,
                Err(_) => {
                    // Try to repair malformed JSON (common with file_write content)
                    Self::try_repair_json(args_str, tool_name)
                        .unwrap_or(Value::Object(Default::default()))
                }
            }
        } else {
            // No arguments block - use empty object
            Value::Object(Default::default())
        }
    }

    /// Try to repair malformed JSON, especially for file_write with unescaped content
    fn try_repair_json(json_str: &str, tool_name: &str) -> Option<Value> {
        // For file_write/devit_file_write, try to extract path and content heuristically
        if tool_name.contains("file_write") {
            return Self::repair_file_write_json(json_str);
        }

        // For other tools, try simple fixes
        // Remove trailing commas before }
        let fixed = json_str.replace(",\n}", "\n}").replace(", }", " }");

        serde_json::from_str(&fixed).ok()
    }

    /// Repair file_write JSON where content has unescaped quotes
    fn repair_file_write_json(json_str: &str) -> Option<Value> {
        // Pattern: {"path": "...", "content": "..."}
        // The content field often has unescaped quotes from code

        // Strategy: Find "path": "VALUE" using regex-like matching
        // Then find "content": " and take everything until the final "\n} or "}

        // Find path value: look for "path": " then extract until next unescaped "
        let path_marker = "\"path\":";
        let path_idx = json_str.find(path_marker)?;
        let after_path_colon = &json_str[path_idx + path_marker.len()..];
        let after_path_trimmed = after_path_colon.trim_start();

        if !after_path_trimmed.starts_with('"') {
            return None;
        }

        // Find the closing quote for path (path should be simple, no unescaped quotes)
        let path_content = &after_path_trimmed[1..]; // Skip opening quote
        let mut path_end = 0;
        let path_chars: Vec<char> = path_content.chars().collect();
        while path_end < path_chars.len() {
            if path_chars[path_end] == '"' && (path_end == 0 || path_chars[path_end - 1] != '\\') {
                break;
            }
            path_end += 1;
        }
        let path_value: String = path_chars[..path_end].iter().collect();

        // Find content value: look for "content": "
        let content_marker = "\"content\":";
        let content_idx = json_str.find(content_marker)?;
        let after_content_colon = &json_str[content_idx + content_marker.len()..];
        let after_content_trimmed = after_content_colon.trim_start();

        if !after_content_trimmed.starts_with('"') {
            return None;
        }

        // Content starts after the opening quote
        let content_start_in_substr = 1; // Skip opening quote
        let content_substr = &after_content_trimmed[content_start_in_substr..];

        // Find the end: look for "\n} or "} pattern at the end of the original json_str
        // The content ends at the last occurrence of these patterns
        let content_value = if let Some(rel_end) = content_substr.rfind("\"\n}") {
            &content_substr[..rel_end]
        } else if let Some(rel_end) = content_substr.rfind("\"}") {
            &content_substr[..rel_end]
        } else if let Some(rel_end) = content_substr.rfind("\"") {
            &content_substr[..rel_end]
        } else {
            return None;
        };

        // Build the repaired JSON object
        let mut map = serde_json::Map::new();
        map.insert("path".to_string(), Value::String(path_value));
        map.insert(
            "content".to_string(),
            Value::String(content_value.to_string()),
        );

        Some(Value::Object(map))
    }

    /// Remove tool call blocks from content, leaving only the text
    pub fn strip_tool_calls(content: &str) -> String {
        let mut result = content.to_string();

        // Strip XML format tool calls
        while let Some(start) = result.find("<tool_call>") {
            if let Some(end_rel) = result[start..].find("</tool_call>") {
                let end = start + end_rel + "</tool_call>".len();
                result.replace_range(start..end, "");
            } else {
                break;
            }
        }

        // Strip JSON format tool calls in code blocks
        // Pattern: ```json\n{...}\n```
        result = Self::strip_json_code_blocks(&result);

        // Normalize whitespace: collapse multiple newlines to max 1 blank line
        let lines: Vec<&str> = result.lines().collect();
        let mut normalized = Vec::new();
        let mut empty_count = 0;

        for line in lines {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                empty_count += 1;
                if empty_count <= 1 {
                    normalized.push("");
                }
            } else {
                empty_count = 0;
                normalized.push(trimmed);
            }
        }

        // Trim leading/trailing empty lines
        while normalized.first() == Some(&"") {
            normalized.remove(0);
        }
        while normalized.last() == Some(&"") {
            normalized.pop();
        }

        normalized.join("\n")
    }

    /// Strip JSON code blocks that contain tool calls
    fn strip_json_code_blocks(content: &str) -> String {
        let mut result = String::new();
        let mut in_code_block = false;
        let mut code_block_content = String::new();

        for line in content.lines() {
            if line.trim().starts_with("```") {
                if in_code_block {
                    // End of code block - check if it was a tool call
                    if !Self::is_tool_call_json(&code_block_content) {
                        // Not a tool call, keep it
                        result.push_str("```\n");
                        result.push_str(&code_block_content);
                        result.push_str("```\n");
                    }
                    code_block_content.clear();
                    in_code_block = false;
                } else {
                    // Start of code block
                    in_code_block = true;
                }
            } else if in_code_block {
                code_block_content.push_str(line);
                code_block_content.push('\n');
            } else {
                result.push_str(line);
                result.push('\n');
            }
        }

        result
    }

    /// Check if JSON content looks like a tool call
    fn is_tool_call_json(content: &str) -> bool {
        if let Ok(obj) = serde_json::from_str::<Value>(content.trim()) {
            obj.get("name").is_some() && obj.get("arguments").is_some()
        } else {
            false
        }
    }
}

/// Detect tool calling format from model family
pub fn detect_format_from_family(family: Option<&str>) -> ToolCallingMode {
    match family {
        Some("qwen") | Some("llama3") | Some("mistral") => ToolCallingMode::Prompt,
        _ => ToolCallingMode::Prompt, // Default to prompt-based for Phase 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_single_tool_call() {
        let content = r#"I'll read that file for you.

<tool_call>
<tool_name>file_read</tool_name>
<arguments>
{"path": "src/main.rs"}
</arguments>
</tool_call>

Let me check the contents."#;

        let calls = DevItFormatParser::parse(content).unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "file_read");
        assert_eq!(calls[0].arguments["path"], "src/main.rs");
    }

    #[test]
    fn test_parse_multiple_tool_calls() {
        let content = r#"
<tool_call>
<tool_name>file_read</tool_name>
<arguments>
{"path": "Cargo.toml"}
</arguments>
</tool_call>

<tool_call>
<tool_name>file_list</tool_name>
<arguments>
{"pattern": "*.rs"}
</arguments>
</tool_call>
"#;

        let calls = DevItFormatParser::parse(content).unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].name, "file_read");
        assert_eq!(calls[1].name, "file_list");
    }

    #[test]
    fn test_strip_tool_calls() {
        let content = r#"I'll read that file.

<tool_call>
<tool_name>file_read</tool_name>
<arguments>
{"path": "src/main.rs"}
</arguments>
</tool_call>

Done!"#;

        let stripped = DevItFormatParser::strip_tool_calls(content);
        // Should normalize to single blank line between paragraphs
        assert_eq!(stripped, "I'll read that file.\n\nDone!");
    }

    #[test]
    fn test_parse_json_format_tool_call() {
        let content = r#"Sure, let me list the files.

```json
{"name": "devit_directory_list", "arguments": {"path": ".", "include_files": true}}
```
"#;

        let calls = DevItFormatParser::parse(content).unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "devit_directory_list");
        assert_eq!(calls[0].arguments["path"], ".");
        assert_eq!(calls[0].arguments["include_files"], true);
    }

    #[test]
    fn test_parse_json_format_inline() {
        let content = r#"{"name": "file_read", "arguments": {"path": "Cargo.toml"}}"#;

        let calls = DevItFormatParser::parse(content).unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "file_read");
    }

    #[test]
    fn test_repair_malformed_file_write() {
        // This simulates what the fine-tuned model generates - unescaped quotes in content
        let content = r#"
<tool_call>
<tool_name>devit_file_write</tool_name>
<arguments>
{
  "path": "main.rs",
  "content": "fn main() {
    let msg = "Hello World";
    println!("{}", msg);
}
"
}
</arguments>
</tool_call>
"#;

        let calls = DevItFormatParser::parse(content).unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "devit_file_write");
        assert_eq!(calls[0].arguments["path"], "main.rs");
        // Content should be extracted even with unescaped quotes
        let content_val = calls[0].arguments["content"].as_str().unwrap();
        assert!(content_val.contains("Hello World"));
        assert!(content_val.contains("fn main()"));
    }
}
