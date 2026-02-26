// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2025-2026 naskel.com

use crate::core::errors::{DevItError, DevItResult};
use std::path::PathBuf;
use tracing::warn;

#[derive(Debug, Clone)]
pub struct PatchHunk {
    pub old_start: usize,
    pub old_count: usize,
    pub new_start: usize,
    pub new_count: usize,
    pub lines: Vec<PatchLine>,
    pub no_trailing_newline: bool,
}

#[derive(Debug, Clone)]
pub enum PatchLine {
    Context(String),
    Add(String),
    Remove(String),
}

#[derive(Debug)]
pub struct FilePatch {
    pub old_path: Option<PathBuf>,
    pub new_path: Option<PathBuf>,
    pub hunks: Vec<PatchHunk>,
    pub is_new_file: bool,
    pub is_deleted_file: bool,
    pub old_mode: Option<u32>,
    pub new_mode: Option<u32>,
    pub adds_exec_bit: bool,
    pub is_binary: bool,
    pub no_trailing_newline: bool,
}

#[derive(Debug)]
pub struct ParsedPatch {
    pub files: Vec<FilePatch>,
}

impl ParsedPatch {
    pub fn from_diff(diff_content: &str) -> DevItResult<Self> {
        let mut files = Vec::new();
        let lines: Vec<&str> = diff_content.lines().collect();
        let mut i = 0;

        // First pass: look for `diff --git` headers (standard git diffs)
        while i < lines.len() {
            if lines[i].starts_with("diff --git ") {
                let (file_patch, next_index) = Self::parse_file_patch(&lines, i)?;
                files.push(file_patch);
                i = next_index;
            } else {
                i += 1;
            }
        }

        // Fallback: if no git-style headers found, try plain unified diff
        if files.is_empty() {
            files = Self::parse_plain_unified(&lines)?;
        }

        Ok(ParsedPatch { files })
    }

    /// Parse a plain unified diff (no `diff --git` header).
    /// Detects file boundaries by `--- `/`+++ ` pairs.
    fn parse_plain_unified(lines: &[&str]) -> DevItResult<Vec<FilePatch>> {
        let mut files = Vec::new();
        let mut i = 0;

        while i < lines.len() {
            if lines[i].starts_with("--- ")
                && i + 1 < lines.len()
                && lines[i + 1].starts_with("+++ ")
            {
                let (file_patch, next_index) = Self::parse_file_patch(lines, i)?;
                files.push(file_patch);
                i = next_index;
            } else {
                i += 1;
            }
        }

        Ok(files)
    }

    fn parse_file_patch(lines: &[&str], start: usize) -> DevItResult<(FilePatch, usize)> {
        let mut i = start;
        let mut old_path = None;
        let mut new_path = None;
        let mut is_new_file = false;
        let mut is_deleted_file = false;
        let mut old_mode = None;
        let mut new_mode = None;
        let mut hunks = Vec::new();
        let mut is_binary = false;

        // Parse diff header
        while i < lines.len() && !lines[i].starts_with("@@") {
            if let Some(rest) = lines[i].strip_prefix("old mode ") {
                old_mode = parse_mode(rest.trim(), i + 1)?;
            } else if let Some(rest) = lines[i].strip_prefix("new mode ") {
                new_mode = parse_mode(rest.trim(), i + 1)?;
            } else if lines[i].starts_with("--- ") {
                let path_str = &lines[i][4..];
                // Strip timestamp suffix (tab-separated) from plain diffs
                let path_str = path_str.split('\t').next().unwrap_or(path_str);
                if path_str != "/dev/null" {
                    old_path = Some(PathBuf::from(path_str.trim_start_matches("a/")));
                }
            } else if lines[i].starts_with("+++ ") {
                let path_str = &lines[i][4..];
                let path_str = path_str.split('\t').next().unwrap_or(path_str);
                if path_str != "/dev/null" {
                    new_path = Some(PathBuf::from(path_str.trim_start_matches("b/")));
                }
            } else if lines[i].contains("new file mode") {
                is_new_file = true;
            } else if lines[i].contains("deleted file mode") {
                is_deleted_file = true;
            } else if lines[i].starts_with("Binary files ") {
                is_binary = true;
                i += 1;
                break;
            }
            i += 1;
        }

        // Parse hunks
        while i < lines.len() && lines[i].starts_with("@@") {
            let (hunk, next_index) = Self::parse_hunk(lines, i)?;
            hunks.push(hunk);
            i = next_index;
        }

        // Infer new/deleted file from /dev/null paths
        if old_path.is_none() && new_path.is_some() && !is_new_file {
            is_new_file = true;
        }
        if new_path.is_none() && old_path.is_some() && !is_deleted_file {
            is_deleted_file = true;
        }

        let no_trailing_newline = hunks.last().map_or(false, |h| h.no_trailing_newline);

        let file_patch = FilePatch {
            old_path,
            new_path,
            hunks,
            is_new_file,
            is_deleted_file,
            old_mode,
            new_mode,
            adds_exec_bit: mode_adds_exec(old_mode, new_mode),
            is_binary,
            no_trailing_newline,
        };

        Ok((file_patch, i))
    }

    fn parse_hunk(lines: &[&str], start: usize) -> DevItResult<(PatchHunk, usize)> {
        let hunk_header = lines[start];

        // ✅ FIX #2: Parse @@ -old_start,old_count +new_start,new_count @@
        // Handle optional trailing text after second @@
        let hunk_content = if let Some(at_pos) = hunk_header.rfind("@@") {
            &hunk_header[..at_pos]
        } else {
            hunk_header
        };

        let parts: Vec<&str> = hunk_content.split_whitespace().collect();
        if parts.len() < 3 {
            return Err(DevItError::InvalidDiff {
                reason: format!("Invalid hunk header: {}", hunk_header),
                line_number: Some(start + 1),
            });
        }

        let old_range = &parts[1][1..]; // Remove '-'
        let new_range = &parts[2][1..]; // Remove '+'

        let (old_start, old_count) = Self::parse_range(old_range)?;
        let (new_start, new_count) = Self::parse_range(new_range)?;

        let mut hunk_lines = Vec::new();
        let mut i = start + 1;
        let mut no_trailing_newline = false;

        // Collect hunk lines with strict boundary detection
        while i < lines.len() {
            let line = lines[i];

            // Stop at next hunk or next file
            if line.starts_with("@@") || line.starts_with("diff --git") {
                break;
            }

            // Stop at next plain unified diff file boundary
            if line.starts_with("--- ") && i + 1 < lines.len() && lines[i + 1].starts_with("+++ ") {
                break;
            }

            match line.chars().next() {
                Some(' ') => hunk_lines.push(PatchLine::Context(line[1..].to_string())),
                Some('+') => hunk_lines.push(PatchLine::Add(line[1..].to_string())),
                Some('-') => hunk_lines.push(PatchLine::Remove(line[1..].to_string())),
                Some('\\') => {
                    // "\ No newline at end of file" marker — skip line, set flag
                    no_trailing_newline = true;
                    i += 1;
                    continue;
                }
                _ => {
                    // Non-patch line in hunk content = end of hunk
                    break;
                }
            }
            i += 1;
        }

        // Validate hunk line counts — lenient for LLM-generated patches.
        // Use actual content as source of truth, warn on mismatch.
        let context_and_removes = hunk_lines
            .iter()
            .filter(|l| matches!(l, PatchLine::Context(_) | PatchLine::Remove(_)))
            .count();

        let context_and_adds = hunk_lines
            .iter()
            .filter(|l| matches!(l, PatchLine::Context(_) | PatchLine::Add(_)))
            .count();

        if context_and_removes != old_count {
            warn!(
                "Hunk at line {}: header says old_count={}, actual {} — using actual",
                start + 1,
                old_count,
                context_and_removes
            );
        }

        if context_and_adds != new_count {
            warn!(
                "Hunk at line {}: header says new_count={}, actual {} — using actual",
                start + 1,
                new_count,
                context_and_adds
            );
        }

        if hunk_lines.is_empty() {
            return Err(DevItError::InvalidDiff {
                reason: format!("Empty hunk at line {}", start + 1),
                line_number: Some(start + 1),
            });
        }

        // Use actual counts, not header counts
        let old_count = context_and_removes;
        let new_count = context_and_adds;

        let hunk = PatchHunk {
            old_start,
            old_count,
            new_start,
            new_count,
            lines: hunk_lines,
            no_trailing_newline,
        };

        Ok((hunk, i))
    }

    fn parse_range(range: &str) -> DevItResult<(usize, usize)> {
        if let Some(comma_pos) = range.find(',') {
            let start = range[..comma_pos]
                .parse()
                .map_err(|_| DevItError::InvalidDiff {
                    reason: format!("Invalid range start: {}", range),
                    line_number: None,
                })?;
            let count = range[comma_pos + 1..]
                .parse()
                .map_err(|_| DevItError::InvalidDiff {
                    reason: format!("Invalid range count: {}", range),
                    line_number: None,
                })?;
            Ok((start, count))
        } else {
            let start = range.parse().map_err(|_| DevItError::InvalidDiff {
                reason: format!("Invalid range: {}", range),
                line_number: None,
            })?;
            Ok((start, 1))
        }
    }
}

fn parse_mode(value: &str, line_number: usize) -> DevItResult<Option<u32>> {
    if value.is_empty() {
        return Ok(None);
    }
    u32::from_str_radix(value, 8)
        .map(Some)
        .map_err(|_| DevItError::InvalidDiff {
            reason: format!("Invalid file mode '{}'", value),
            line_number: Some(line_number),
        })
}

fn mode_adds_exec(old_mode: Option<u32>, new_mode: Option<u32>) -> bool {
    const EXEC_MASK: u32 = 0o111;
    match (old_mode, new_mode) {
        (Some(old), Some(new)) => (new & EXEC_MASK) != 0 && (old & EXEC_MASK) == 0,
        (None, Some(new)) => (new & EXEC_MASK) != 0,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_git_style_diff() {
        let diff = "diff --git a/src/main.rs b/src/main.rs
index abc1234..def5678 100644
--- a/src/main.rs
+++ b/src/main.rs
@@ -1,3 +1,4 @@
 fn main() {
     println!(\"hello\");
+    println!(\"world\");
 }
";
        let parsed = ParsedPatch::from_diff(diff).unwrap();
        assert_eq!(parsed.files.len(), 1);
        assert_eq!(parsed.files[0].hunks[0].lines.len(), 4);
    }

    #[test]
    fn parse_plain_unified_diff() {
        let diff = "\
--- a/hello.txt
+++ b/hello.txt
@@ -1,3 +1,3 @@
 BEGIN
-OLD
+NEW
 END
";
        let parsed = ParsedPatch::from_diff(diff).unwrap();
        assert_eq!(parsed.files.len(), 1);
        assert_eq!(
            parsed.files[0].new_path.as_ref().unwrap().to_str().unwrap(),
            "hello.txt"
        );
        assert_eq!(parsed.files[0].hunks.len(), 1);
    }

    #[test]
    fn parse_plain_diff_with_timestamps() {
        let diff = "\
--- hello.txt\t2025-01-01 10:00:00.000000000 +0100
+++ hello.txt\t2025-01-01 10:01:00.000000000 +0100
@@ -1 +1 @@
-old
+new
";
        let parsed = ParsedPatch::from_diff(diff).unwrap();
        assert_eq!(parsed.files.len(), 1);
        assert_eq!(
            parsed.files[0].new_path.as_ref().unwrap().to_str().unwrap(),
            "hello.txt"
        );
    }

    #[test]
    fn parse_plain_diff_no_prefix() {
        let diff = "\
--- hello.txt
+++ hello.txt
@@ -1 +1 @@
-old
+new
";
        let parsed = ParsedPatch::from_diff(diff).unwrap();
        assert_eq!(parsed.files.len(), 1);
    }

    #[test]
    fn lenient_hunk_count_mismatch() {
        // Header says @@ -1,2 +1,2 @@ but content has 3 old and 3 new lines
        let diff = "diff --git a/test.txt b/test.txt
--- a/test.txt
+++ b/test.txt
@@ -1,2 +1,2 @@
 line1
-old
+new
 line3
";
        let parsed = ParsedPatch::from_diff(diff).unwrap();
        // Should use actual counts (3,3) not header counts (2,2)
        assert_eq!(parsed.files[0].hunks[0].old_count, 3);
        assert_eq!(parsed.files[0].hunks[0].new_count, 3);
    }

    #[test]
    fn no_trailing_newline_marker() {
        let diff = "diff --git a/test.txt b/test.txt
--- a/test.txt
+++ b/test.txt
@@ -1 +1 @@
-old
\\ No newline at end of file
+new
\\ No newline at end of file
";
        let parsed = ParsedPatch::from_diff(diff).unwrap();
        assert!(parsed.files[0].no_trailing_newline);
        assert!(parsed.files[0].hunks[0].no_trailing_newline);
        // Should still have the actual patch lines
        assert_eq!(parsed.files[0].hunks[0].lines.len(), 2);
    }

    #[test]
    fn random_text_parses_zero_files() {
        let diff = "this is not a patch\njust random text\n";
        let parsed = ParsedPatch::from_diff(diff).unwrap();
        assert!(parsed.files.is_empty());
    }

    #[test]
    fn plain_diff_new_file_from_dev_null() {
        let diff = "\
--- /dev/null
+++ b/new_file.txt
@@ -0,0 +1,2 @@
+hello
+world
";
        let parsed = ParsedPatch::from_diff(diff).unwrap();
        assert_eq!(parsed.files.len(), 1);
        assert!(parsed.files[0].is_new_file);
        assert!(parsed.files[0].old_path.is_none());
        assert_eq!(
            parsed.files[0].new_path.as_ref().unwrap().to_str().unwrap(),
            "new_file.txt"
        );
    }

    #[test]
    fn plain_diff_multi_file() {
        let diff = "\
--- a/file1.txt
+++ b/file1.txt
@@ -1 +1 @@
-old1
+new1
--- a/file2.txt
+++ b/file2.txt
@@ -1 +1 @@
-old2
+new2
";
        let parsed = ParsedPatch::from_diff(diff).unwrap();
        assert_eq!(parsed.files.len(), 2);
        assert_eq!(
            parsed.files[0].new_path.as_ref().unwrap().to_str().unwrap(),
            "file1.txt"
        );
        assert_eq!(
            parsed.files[1].new_path.as_ref().unwrap().to_str().unwrap(),
            "file2.txt"
        );
    }
}
