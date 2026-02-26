// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2025-2026 naskel.com

//! Diff management and hunk-based editing

use crate::{ContextError, EditOperation, Result, StructuredEdit};
use serde::{Deserialize, Serialize};
use similar::{ChangeTag, TextDiff};

/// A single edit hunk for UI display and approval
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditHunk {
    /// Unique identifier for this hunk
    pub id: String,

    /// Target file path
    pub file_path: String,

    /// Start line in original file (1-indexed)
    pub start_line: usize,

    /// End line in original file (1-indexed)
    pub end_line: usize,

    /// Original content (before edit)
    pub original: String,

    /// New content (after edit)
    pub replacement: String,

    /// Description of the change (for UI)
    pub description: String,

    /// Status of this hunk
    pub status: HunkStatus,
}

/// Status of an edit hunk
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HunkStatus {
    /// Pending user review
    Pending,
    /// Accepted by user
    Accepted,
    /// Rejected by user
    Rejected,
    /// Applied to file
    Applied,
}

/// Diff applier for managing code edits
pub struct DiffApplier;

impl DiffApplier {
    /// Parse LLM JSON edit response into hunks
    pub fn parse_edit(json_str: &str) -> Result<Vec<EditHunk>> {
        let edit: StructuredEdit = serde_json::from_str(json_str)
            .map_err(|e| ContextError::Diff(format!("Failed to parse edit JSON: {}", e)))?;

        let mut hunks = Vec::new();

        for (idx, op) in edit.edits.iter().enumerate() {
            let hunk = match op {
                EditOperation::Replace {
                    start_line,
                    end_line,
                    content,
                } => EditHunk {
                    id: format!("{}-{}", edit.file, idx),
                    file_path: edit.file.clone(),
                    start_line: *start_line,
                    end_line: *end_line,
                    original: String::new(), // Will be filled when loading file
                    replacement: content.clone(),
                    description: format!("Replace lines {}-{}", start_line, end_line),
                    status: HunkStatus::Pending,
                },
                EditOperation::InsertAfter {
                    after_line,
                    content,
                } => EditHunk {
                    id: format!("{}-{}", edit.file, idx),
                    file_path: edit.file.clone(),
                    start_line: *after_line,
                    end_line: *after_line,
                    original: String::new(),
                    replacement: content.clone(),
                    description: format!("Insert after line {}", after_line),
                    status: HunkStatus::Pending,
                },
                EditOperation::Delete {
                    start_line,
                    end_line,
                } => EditHunk {
                    id: format!("{}-{}", edit.file, idx),
                    file_path: edit.file.clone(),
                    start_line: *start_line,
                    end_line: *end_line,
                    original: String::new(),
                    replacement: String::new(),
                    description: format!("Delete lines {}-{}", start_line, end_line),
                    status: HunkStatus::Pending,
                },
                EditOperation::ReplaceFile { content } => EditHunk {
                    id: format!("{}-{}", edit.file, idx),
                    file_path: edit.file.clone(),
                    start_line: 1,
                    end_line: usize::MAX, // Marker for full file
                    original: String::new(),
                    replacement: content.clone(),
                    description: "Replace entire file".to_string(),
                    status: HunkStatus::Pending,
                },
            };

            hunks.push(hunk);
        }

        Ok(hunks)
    }

    /// Fill in original content for hunks from file
    pub fn fill_original_content(hunks: &mut [EditHunk], file_content: &str) -> Result<()> {
        let lines: Vec<&str> = file_content.lines().collect();

        for hunk in hunks {
            if hunk.end_line == usize::MAX {
                // Full file replacement
                hunk.original = file_content.to_string();
                hunk.end_line = lines.len();
            } else {
                // Extract original lines
                let start_idx = hunk.start_line.saturating_sub(1);
                let end_idx = hunk.end_line.min(lines.len());

                if start_idx < lines.len() {
                    hunk.original = lines[start_idx..end_idx].join("\n");
                }
            }
        }

        Ok(())
    }

    /// Apply accepted hunks to file content
    ///
    /// Returns the new file content
    pub fn apply_hunks(original_content: &str, hunks: &[EditHunk]) -> Result<String> {
        // Filter to only accepted hunks
        let accepted: Vec<&EditHunk> = hunks
            .iter()
            .filter(|h| h.status == HunkStatus::Accepted)
            .collect();

        if accepted.is_empty() {
            return Ok(original_content.to_string());
        }

        // Sort hunks by start line in reverse order (apply from bottom to top)
        let mut sorted_hunks = accepted;
        sorted_hunks.sort_by(|a, b| b.start_line.cmp(&a.start_line));

        let mut lines: Vec<String> = original_content.lines().map(|s| s.to_string()).collect();

        for hunk in sorted_hunks {
            let start_idx = hunk.start_line.saturating_sub(1);
            let end_idx = hunk.end_line.min(lines.len());

            if hunk.replacement.is_empty() {
                // Delete
                if start_idx < lines.len() {
                    lines.drain(start_idx..end_idx);
                }
            } else if hunk.original.is_empty() && hunk.start_line == hunk.end_line {
                // Insert after
                let insert_idx = hunk.start_line.min(lines.len());
                let new_lines: Vec<String> =
                    hunk.replacement.lines().map(|s| s.to_string()).collect();
                for (i, line) in new_lines.into_iter().enumerate() {
                    lines.insert(insert_idx + i, line);
                }
            } else {
                // Replace
                if start_idx < lines.len() {
                    let new_lines: Vec<String> =
                        hunk.replacement.lines().map(|s| s.to_string()).collect();
                    lines.splice(start_idx..end_idx, new_lines);
                }
            }
        }

        Ok(lines.join("\n"))
    }

    /// Generate a unified diff between two strings
    pub fn unified_diff(original: &str, modified: &str, file_path: &str) -> String {
        let diff = TextDiff::from_lines(original, modified);

        let mut output = String::new();
        output.push_str(&format!("--- a/{}\n", file_path));
        output.push_str(&format!("+++ b/{}\n", file_path));

        for (idx, group) in diff.grouped_ops(3).iter().enumerate() {
            if idx > 0 {
                output.push_str("\n");
            }

            // Generate hunk header
            let (old_start, old_len, new_start, new_len) = Self::calc_hunk_range(group);
            output.push_str(&format!(
                "@@ -{},{} +{},{} @@\n",
                old_start, old_len, new_start, new_len
            ));

            // Generate changes
            for op in group {
                for change in diff.iter_changes(op) {
                    let prefix = match change.tag() {
                        ChangeTag::Equal => " ",
                        ChangeTag::Delete => "-",
                        ChangeTag::Insert => "+",
                    };
                    output.push_str(prefix);
                    output.push_str(change.value());
                    if change.missing_newline() {
                        output.push_str("\n\\ No newline at end of file\n");
                    }
                }
            }
        }

        output
    }

    /// Calculate hunk range for unified diff header
    fn calc_hunk_range(ops: &[similar::DiffOp]) -> (usize, usize, usize, usize) {
        let mut old_start = usize::MAX;
        let mut old_end = 0;
        let mut new_start = usize::MAX;
        let mut new_end = 0;

        for op in ops {
            match op {
                similar::DiffOp::Equal {
                    old_index,
                    new_index,
                    len,
                } => {
                    old_start = old_start.min(*old_index);
                    old_end = old_end.max(*old_index + *len);
                    new_start = new_start.min(*new_index);
                    new_end = new_end.max(*new_index + *len);
                }
                similar::DiffOp::Delete {
                    old_index, old_len, ..
                } => {
                    old_start = old_start.min(*old_index);
                    old_end = old_end.max(*old_index + *old_len);
                }
                similar::DiffOp::Insert {
                    new_index, new_len, ..
                } => {
                    new_start = new_start.min(*new_index);
                    new_end = new_end.max(*new_index + *new_len);
                }
                similar::DiffOp::Replace {
                    old_index,
                    old_len,
                    new_index,
                    new_len,
                } => {
                    old_start = old_start.min(*old_index);
                    old_end = old_end.max(*old_index + *old_len);
                    new_start = new_start.min(*new_index);
                    new_end = new_end.max(*new_index + *new_len);
                }
            }
        }

        (
            old_start + 1, // 1-indexed
            old_end - old_start,
            new_start + 1,
            new_end - new_start,
        )
    }

    /// Generate side-by-side diff for UI display
    pub fn side_by_side_diff(original: &str, modified: &str) -> Vec<DiffLine> {
        let diff = TextDiff::from_lines(original, modified);
        let mut result = Vec::new();

        for change in diff.iter_all_changes() {
            let line = DiffLine {
                tag: match change.tag() {
                    ChangeTag::Equal => DiffTag::Equal,
                    ChangeTag::Delete => DiffTag::Delete,
                    ChangeTag::Insert => DiffTag::Insert,
                },
                old_line: change.old_index().map(|i| i + 1),
                new_line: change.new_index().map(|i| i + 1),
                content: change.value().trim_end_matches('\n').to_string(),
            };
            result.push(line);
        }

        result
    }
}

/// A line in a diff
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffLine {
    pub tag: DiffTag,
    pub old_line: Option<usize>,
    pub new_line: Option<usize>,
    pub content: String,
}

/// Type of diff line
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiffTag {
    Equal,
    Delete,
    Insert,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_edit() {
        let json = r#"{
            "file": "src/main.rs",
            "edits": [
                {
                    "op": "replace",
                    "start_line": 10,
                    "end_line": 15,
                    "content": "fn new_code() {}"
                }
            ]
        }"#;

        let hunks = DiffApplier::parse_edit(json).unwrap();
        assert_eq!(hunks.len(), 1);
        assert_eq!(hunks[0].file_path, "src/main.rs");
        assert_eq!(hunks[0].start_line, 10);
        assert_eq!(hunks[0].end_line, 15);
    }

    #[test]
    fn test_apply_hunks() {
        let original = "line 1\nline 2\nline 3\nline 4\nline 5";
        let hunks = vec![EditHunk {
            id: "test-0".to_string(),
            file_path: "test.txt".to_string(),
            start_line: 2,
            end_line: 3,
            original: "line 2\nline 3".to_string(),
            replacement: "new line 2\nnew line 3".to_string(),
            description: "Replace lines 2-3".to_string(),
            status: HunkStatus::Accepted,
        }];

        let result = DiffApplier::apply_hunks(original, &hunks).unwrap();
        assert!(result.contains("new line 2"));
        assert!(result.contains("new line 3"));
    }

    #[test]
    fn test_unified_diff() {
        let original = "line 1\nline 2\nline 3";
        let modified = "line 1\nmodified line 2\nline 3";

        let diff = DiffApplier::unified_diff(original, modified, "test.txt");
        assert!(diff.contains("--- a/test.txt"));
        assert!(diff.contains("+++ b/test.txt"));
        assert!(diff.contains("-line 2"));
        assert!(diff.contains("+modified line 2"));
    }
}
