use anyhow::{Result, anyhow, bail};
use std::collections::{BTreeSet, HashSet};
use std::path::Path;

#[derive(Debug, Clone)]
pub struct ParsedDiff {
    pub files: Vec<FilePatch>,
}

#[derive(Debug, Clone)]
pub struct FilePatch {
    pub old_path: Option<String>,
    pub new_path: String,
    pub hunks: Vec<DiffHunk>,
    pub valid_lines: HashSet<u64>,
    pub added_lines: HashSet<u64>,
    pub file_context: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DiffHunk {
    pub old_start: u64,
    pub old_lines: u64,
    pub new_start: u64,
    pub new_lines: u64,
    pub lines: Vec<String>,
}

#[derive(Default)]
struct PendingFile {
    old_header: Option<String>,
    new_header: Option<String>,
    raw_new_header: Option<String>,
    hunks: Vec<DiffHunk>,
    valid_lines: HashSet<u64>,
    added_lines: HashSet<u64>,
}

impl PendingFile {
    fn flush(&mut self, out: &mut Vec<FilePatch>) -> Result<()> {
        let Some(new_raw) = self.new_header.take() else {
            self.old_header = None;
            self.raw_new_header = None;
            self.hunks.clear();
            self.valid_lines.clear();
            self.added_lines.clear();
            return Ok(());
        };

        if self.hunks.is_empty() {
            let display = self.raw_new_header.take().unwrap_or(new_raw);
            bail!("Incomplete diff: `{display}` has file headers but no hunk");
        }

        let old_clean = self.old_header.take().and_then(|p| {
            let cleaned = clean_diff_path(&p);
            (cleaned != "/dev/null").then_some(cleaned)
        });
        let new_path = clean_diff_path(&new_raw);

        out.push(FilePatch {
            old_path: old_clean,
            new_path,
            hunks: std::mem::take(&mut self.hunks),
            valid_lines: std::mem::take(&mut self.valid_lines),
            added_lines: std::mem::take(&mut self.added_lines),
            file_context: None,
        });
        self.raw_new_header = None;
        Ok(())
    }
}

impl ParsedDiff {
    /// Parses unified diff text (from `git diff`, `jj diff --git`, or Forge APIs) without panicking.
    pub fn parse(diff_text: &str) -> Result<Self> {
        let lines: Vec<&str> = diff_text
            .lines()
            .map(|l| l.strip_suffix('\r').unwrap_or(l))
            .collect();

        let mut files = Vec::new();
        let mut pending = PendingFile::default();
        let mut active_hunk: Option<(DiffHunk, u64, u64, u64)> = None;

        let flush_hunk = |active: &mut Option<(DiffHunk, u64, u64, u64)>, pf: &mut PendingFile| {
            if let Some((hunk, _, _, _)) = active.take() {
                pf.hunks.push(hunk);
            }
        };

        let mut idx = 0;
        while idx < lines.len() {
            let line = lines[idx];

            if line.starts_with("diff --git ") {
                flush_hunk(&mut active_hunk, &mut pending);
                pending.flush(&mut files)?;
                idx += 1;
                continue;
            }

            let hunk_complete = active_hunk
                .as_ref()
                .is_none_or(|(_, _, rem_old, rem_new)| *rem_old == 0 && *rem_new == 0);
            if hunk_complete
                && let Some(old_hdr) = line.strip_prefix("--- ")
                && lines
                    .get(idx + 1)
                    .is_some_and(|next| next.starts_with("+++ "))
            {
                flush_hunk(&mut active_hunk, &mut pending);
                pending.flush(&mut files)?;
                let raw_new = lines[idx + 1]
                    .strip_prefix("+++ ")
                    .unwrap_or_default()
                    .trim();
                pending.old_header = Some(parse_header_path(old_hdr));
                pending.new_header = Some(parse_header_path(raw_new));
                pending.raw_new_header = Some(raw_new.to_string());
                idx += 2;
                continue;
            }

            if line.starts_with("@@") {
                if pending.new_header.is_none() {
                    bail!("Failed to parse unified diff: hunk header without file header");
                }
                flush_hunk(&mut active_hunk, &mut pending);
                let (old_start, old_lines, new_start, new_lines) = parse_hunk_header(line)?;
                active_hunk = Some((
                    DiffHunk {
                        old_start,
                        old_lines,
                        new_start,
                        new_lines,
                        lines: Vec::new(),
                    },
                    new_start,
                    old_lines,
                    new_lines,
                ));
                idx += 1;
                continue;
            }

            if let Some((ref mut hunk, ref mut cur_new, ref mut rem_old, ref mut rem_new)) =
                active_hunk
            {
                if let Some(rest) = line.strip_prefix('+') {
                    pending.valid_lines.insert(*cur_new);
                    pending.added_lines.insert(*cur_new);
                    hunk.lines.push(format!("+{rest}"));
                    *cur_new += 1;
                    *rem_new = rem_new.saturating_sub(1);
                } else if let Some(rest) = line.strip_prefix('-') {
                    hunk.lines.push(format!("-{rest}"));
                    *rem_old = rem_old.saturating_sub(1);
                } else if let Some(rest) = line.strip_prefix(' ') {
                    pending.valid_lines.insert(*cur_new);
                    hunk.lines.push(format!(" {rest}"));
                    *cur_new += 1;
                    *rem_old = rem_old.saturating_sub(1);
                    *rem_new = rem_new.saturating_sub(1);
                } else if line.starts_with('\\') {
                    // `\ No newline at end of file` carries metadata only and does not advance line numbers.
                } else if line.is_empty() {
                    // Unprefixed blank context lines emitted by stripped editors.
                    pending.valid_lines.insert(*cur_new);
                    hunk.lines.push(" ".to_string());
                    *cur_new += 1;
                    *rem_old = rem_old.saturating_sub(1);
                    *rem_new = rem_new.saturating_sub(1);
                } else {
                    bail!("Failed to parse unified diff: unexpected hunk line `{line}`");
                }
            }

            idx += 1;
        }

        flush_hunk(&mut active_hunk, &mut pending);
        pending.flush(&mut files)?;

        Ok(Self { files })
    }

    /// Filter files matching ignore glob patterns (like `**/flake.lock`).
    pub fn apply_filters(&mut self, filters: &[String]) {
        if filters.is_empty() {
            return;
        }

        self.files.retain(|f| {
            let path = effective_path(f);
            for pattern in filters {
                let clean_pattern = pattern.strip_prefix('!').unwrap_or(pattern);
                let glob_pattern = clean_glob(clean_pattern);
                if let Ok(matcher) = glob::Pattern::new(&glob_pattern)
                    && matcher.matches(path)
                {
                    tracing::debug!("Ignoring filtered file: {}", path);
                    return false;
                }
            }
            true
        });
    }

    /// Enrich diff with surrounding file context from disk.
    pub fn enrich_with_disk_context(&mut self, root_dir: Option<&Path>) {
        let base = root_dir.unwrap_or_else(|| Path::new("."));
        let canonical_base = base.canonicalize().ok();

        for f in &mut self.files {
            if f.new_path == "/dev/null" {
                continue;
            }
            let file_path = base.join(&f.new_path);
            if let Some(ref root) = canonical_base {
                let Ok(canon) = file_path.canonicalize() else {
                    continue;
                };
                if !canon.starts_with(root) {
                    tracing::warn!(
                        "Rejecting disk context path escaping base directory: {:?}",
                        file_path
                    );
                    continue;
                }
            }

            if file_path.is_file()
                && let Ok(content) = std::fs::read_to_string(&file_path)
            {
                let lines: Vec<&str> = content.lines().collect();
                if lines.len() <= 500 {
                    let mut numbered = String::new();
                    for (idx, line) in lines.iter().enumerate() {
                        numbered.push_str(&format!("{:4} | {}\n", idx + 1, line));
                    }
                    f.file_context = Some(numbered);
                } else {
                    let mut included_lines = BTreeSet::new();
                    for h in &f.hunks {
                        let start = h.new_start.saturating_sub(40).max(1);
                        let end = (h.new_start + h.new_lines + 40).min(lines.len() as u64);
                        for l in start..=end {
                            included_lines.insert(l);
                        }
                    }

                    let mut excerpt = String::new();
                    let mut last_line = 0;
                    for line_num in included_lines {
                        if last_line != 0 && line_num > last_line + 1 {
                            excerpt.push_str("     ...\n");
                        }
                        if let Some(line) = lines.get((line_num - 1) as usize) {
                            excerpt.push_str(&format!("{:4} | {}\n", line_num, line));
                        }
                        last_line = line_num;
                    }
                    f.file_context = Some(excerpt);
                }
            }
        }
    }

    /// Check if a proposed comment line is within the diff hunk additions/context.
    pub fn is_valid_line(&self, file_path: &str, line: u64) -> bool {
        self.files
            .iter()
            .find(|f| f.new_path == file_path)
            .is_some_and(|f| f.valid_lines.contains(&line))
    }

    /// Check if a proposed line is strictly an added line in the diff.
    pub fn is_added_line(&self, file_path: &str, line: u64) -> bool {
        self.files
            .iter()
            .find(|f| f.new_path == file_path)
            .is_some_and(|f| f.added_lines.contains(&line))
    }

    /// Find the first added line within a span `[start, end]`.
    pub fn find_first_added_line_in_range(
        &self,
        file_path: &str,
        start: u64,
        end: u64,
    ) -> Option<u64> {
        self.files
            .iter()
            .find(|f| f.new_path == file_path)
            .and_then(|f| (start..=end).find(|line| f.added_lines.contains(line)))
    }

    /// Formats diff and surrounding file context under an estimated token budget (~4 chars/token).
    pub fn format_with_budget(&self, max_tokens: usize) -> (String, DiffBudgetStats) {
        let mut stats = DiffBudgetStats {
            total_files: self.files.len(),
            ..Default::default()
        };

        let max_chars = max_tokens.max(1).saturating_mul(4);
        let mut total_chars = 0;
        let mut out = String::new();

        let mut reviewable_files: Vec<&FilePatch> = Vec::new();
        let mut generated_files: Vec<&FilePatch> = Vec::new();

        for f in &self.files {
            if is_likely_generated_or_lockfile(effective_path(f)) {
                generated_files.push(f);
            } else {
                reviewable_files.push(f);
            }
        }

        for generated in generated_files {
            stats.omitted_generated_files += 1;
            let note = format!(
                "=== FILE OMITTED (LOCKFILE / GENERATED): {} ===\n\n",
                effective_path(generated)
            );
            total_chars += note.len();
            out.push_str(&note);
        }

        reviewable_files.sort_by_key(|f| {
            let p = effective_path(f);
            (classify_file_priority(p), p.to_string())
        });

        for f in reviewable_files {
            let path_display = effective_path(f);
            if total_chars >= max_chars {
                stats.omitted_budget_files += 1;
                let (additions, deletions) = count_patch_lines(f);
                out.push_str(&format!(
                    "=== FILE OMITTED (EXCEEDS TOKEN BUDGET): {} (+{}, -{}) ===\n\n",
                    path_display, additions, deletions
                ));
                continue;
            }

            let remaining_chars = max_chars.saturating_sub(total_chars);
            let old_header = match f.old_path.as_deref() {
                Some(old) => format!("a/{old}"),
                None => "/dev/null".to_string(),
            };
            let new_header = if f.new_path == "/dev/null" {
                "/dev/null".to_string()
            } else {
                format!("b/{}", f.new_path)
            };
            let file_header = format!(
                "=== MODIFIED FILE: {path_display} ===\n--- {old_header}\n+++ {new_header}\n"
            );

            if file_header.len() > remaining_chars {
                stats.omitted_budget_files += 1;
                let (additions, deletions) = count_patch_lines(f);
                out.push_str(&format!(
                    "=== FILE OMITTED (EXCEEDS TOKEN BUDGET): {} (+{}, -{}) ===\n\n",
                    path_display, additions, deletions
                ));
                continue;
            }

            let mut file_buf = String::new();
            file_buf.push_str(&file_header);

            let mut truncated_hunks = false;
            let mut skipped_hunk_lines = 0;
            let mut done = false;

            const TRUNC_MARKER_EST: usize = 30;
            const FINAL_NEWLINE: usize = 1;

            for (hi, h) in f.hunks.iter().enumerate() {
                let hunk_header = format!(
                    "@@ -{},{} +{},{} @@\n",
                    h.old_start, h.old_lines, h.new_start, h.new_lines
                );

                if file_buf.len() + hunk_header.len() + TRUNC_MARKER_EST + FINAL_NEWLINE
                    > remaining_chars
                {
                    truncated_hunks = true;
                    skipped_hunk_lines +=
                        f.hunks[hi..].iter().map(|x| x.lines.len()).sum::<usize>();
                    break;
                }
                file_buf.push_str(&hunk_header);

                for (li, l) in h.lines.iter().enumerate() {
                    if file_buf.len() + l.len() + 1 + TRUNC_MARKER_EST + FINAL_NEWLINE
                        > remaining_chars
                    {
                        truncated_hunks = true;
                        skipped_hunk_lines += h.lines.len() - li;
                        skipped_hunk_lines += f.hunks[hi + 1..]
                            .iter()
                            .map(|x| x.lines.len())
                            .sum::<usize>();
                        done = true;
                        break;
                    }
                    file_buf.push_str(l);
                    file_buf.push('\n');
                }

                if done {
                    break;
                }
            }

            if truncated_hunks {
                let marker = format!(
                    "[... {} lines truncated to stay within review token budget ...]\n",
                    skipped_hunk_lines
                );
                if file_buf.len() + marker.len() + FINAL_NEWLINE <= remaining_chars {
                    file_buf.push_str(&marker);
                } else {
                    let short_marker = "[... truncated ...]\n";
                    if file_buf.len() + short_marker.len() + FINAL_NEWLINE <= remaining_chars {
                        file_buf.push_str(short_marker);
                    }
                }
            } else if let Some(ctx) = &f.file_context {
                let ctx_header = format!(
                    "\n[SURROUNDING FILE CONTEXT FOR {} (numbered lines)]:\n",
                    path_display
                );
                if file_buf.len() + ctx_header.len() + ctx.len() + FINAL_NEWLINE <= remaining_chars
                {
                    file_buf.push_str(&ctx_header);
                    file_buf.push_str(ctx);
                    file_buf.push('\n');
                }
            }

            if file_buf.len() < remaining_chars {
                file_buf.push('\n');
            }
            total_chars += file_buf.len();
            out.push_str(&file_buf);
            stats.included_files += 1;
        }

        stats.estimated_tokens = total_chars / 4;
        (out, stats)
    }
}

/// Parses `@@ -old_start[,old_count] +new_start[,new_count] @@ [heading]`.
fn parse_hunk_header(line: &str) -> Result<(u64, u64, u64, u64)> {
    let inner = line
        .strip_prefix("@@")
        .and_then(|s| s.split("@@").next())
        .map(str::trim)
        .ok_or_else(|| anyhow!("Malformed hunk header: `{line}`"))?;

    let mut parts = inner.split_whitespace();
    let old_spec = parts
        .next()
        .and_then(|s| s.strip_prefix('-'))
        .ok_or_else(|| anyhow!("Malformed old range in hunk header: `{line}`"))?;
    let new_spec = parts
        .next()
        .and_then(|s| s.strip_prefix('+'))
        .ok_or_else(|| anyhow!("Malformed new range in hunk header: `{line}`"))?;

    let (old_start, old_count) = parse_hunk_range(old_spec)
        .ok_or_else(|| anyhow!("Invalid old line numbers in hunk header: `{line}`"))?;
    let (new_start, new_count) = parse_hunk_range(new_spec)
        .ok_or_else(|| anyhow!("Invalid new line numbers in hunk header: `{line}`"))?;

    Ok((old_start, old_count, new_start, new_count))
}

fn parse_hunk_range(spec: &str) -> Option<(u64, u64)> {
    if let Some((start_s, count_s)) = spec.split_once(',') {
        Some((start_s.parse().ok()?, count_s.parse().ok()?))
    } else {
        Some((spec.parse().ok()?, 1))
    }
}

fn parse_header_path(raw: &str) -> String {
    let trimmed = raw.trim();
    if let Some(rest) = trimmed.strip_prefix('"') {
        return unescape_git_quoted_path(rest);
    }
    trimmed
        .split('\t')
        .next()
        .unwrap_or(trimmed)
        .trim()
        .to_string()
}

/// Decodes a Git C-quoted pathname (after the opening `"`) in a single pass, including octal UTF-8 bytes.
fn unescape_git_quoted_path(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;

    while i < bytes.len() {
        match bytes[i] {
            b'"' => break,
            b'\\' if i + 1 < bytes.len() => {
                i += 1;
                match bytes[i] {
                    b'0'..=b'7' => {
                        let mut val = (bytes[i] - b'0') as u32;
                        for _ in 0..2 {
                            if i + 1 < bytes.len() && matches!(bytes[i + 1], b'0'..=b'7') {
                                i += 1;
                                val = val * 8 + (bytes[i] - b'0') as u32;
                            } else {
                                break;
                            }
                        }
                        out.push((val & 0xff) as u8);
                    }
                    b'a' => out.push(0x07),
                    b'b' => out.push(0x08),
                    b'f' => out.push(0x0c),
                    b'n' => out.push(b'\n'),
                    b'r' => out.push(b'\r'),
                    b't' => out.push(b'\t'),
                    b'v' => out.push(0x0b),
                    b'\\' => out.push(b'\\'),
                    b'"' => out.push(b'"'),
                    other => out.push(other),
                }
            }
            b => out.push(b),
        }
        i += 1;
    }

    String::from_utf8_lossy(&out).into_owned()
}

/// Statistics summarizing token budgeting and file compression for review payloads.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiffBudgetStats {
    pub total_files: usize,
    pub included_files: usize,
    pub omitted_generated_files: usize,
    pub omitted_budget_files: usize,
    pub estimated_tokens: usize,
}

/// Heuristic check for auto-generated lockfiles, build artifacts, or minified assets.
pub fn is_likely_generated_or_lockfile(path: &str) -> bool {
    let lower = path.to_lowercase();
    let basename = lower.rsplit('/').next().unwrap_or(&lower);

    if matches!(
        basename,
        "cargo.lock"
            | "flake.lock"
            | "package-lock.json"
            | "pnpm-lock.yaml"
            | "yarn.lock"
            | "bun.lockb"
            | "composer.lock"
            | "poetry.lock"
            | "pipfile.lock"
            | "go.sum"
            | "gemfile.lock"
    ) {
        return true;
    }

    if basename.ends_with(".min.js")
        || basename.ends_with(".min.css")
        || basename.ends_with(".map")
        || basename.ends_with(".bundle.js")
    {
        return true;
    }

    path.starts_with("vendor/")
        || path.contains("/vendor/")
        || path.starts_with("node_modules/")
        || path.contains("/node_modules/")
}

/// Semantic review priority: 1 = core logic/source, 2 = tests & configuration, 3 = documentation & assets.
pub fn classify_file_priority(path: &str) -> u8 {
    let lower = path.to_lowercase();
    let ext = lower.rsplit('.').next().unwrap_or("");
    let file_name = lower.rsplit('/').next().unwrap_or(&lower);

    let in_test_dir = lower
        .split('/')
        .any(|seg| matches!(seg, "test" | "tests" | "spec" | "specs" | "__tests__"));
    if in_test_dir
        || file_name.starts_with("test_")
        || file_name.contains(".test.")
        || file_name.contains(".spec.")
        || file_name.contains("_test.")
        || file_name.contains("_spec.")
    {
        return 2;
    }

    if matches!(
        ext,
        "toml" | "yaml" | "yml" | "json" | "env" | "ini" | "cfg"
    ) {
        return 2;
    }

    if matches!(
        ext,
        "md" | "txt" | "rst" | "html" | "svg" | "png" | "jpg" | "jpeg" | "ico"
    ) {
        return 3;
    }

    if matches!(
        ext,
        "rs" | "go"
            | "py"
            | "ts"
            | "js"
            | "svelte"
            | "c"
            | "cpp"
            | "h"
            | "hpp"
            | "zig"
            | "nix"
            | "sh"
            | "bash"
            | "sql"
            | "java"
            | "kt"
            | "swift"
            | "rb"
            | "php"
            | "cs"
    ) {
        return 1;
    }

    2
}

fn count_patch_lines(f: &FilePatch) -> (usize, usize) {
    let mut added = 0;
    let mut removed = 0;
    for h in &f.hunks {
        for l in &h.lines {
            if l.starts_with('+') {
                added += 1;
            } else if l.starts_with('-') {
                removed += 1;
            }
        }
    }
    (added, removed)
}

fn effective_path(f: &FilePatch) -> &str {
    if f.new_path == "/dev/null" {
        f.old_path.as_deref().unwrap_or(&f.new_path)
    } else {
        &f.new_path
    }
}

fn clean_diff_path(path: &str) -> String {
    path.strip_prefix("a/")
        .or_else(|| path.strip_prefix("b/"))
        .unwrap_or(path)
        .to_string()
}

fn clean_glob(pattern: &str) -> String {
    pattern.trim_start_matches('/').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_DIFF: &str = r#"--- a/src/main.rs
+++ b/src/main.rs
@@ -1,3 +1,4 @@
 fn main() {
-    println!("hello");
+    let x = 42;
+    println!("hello {}", x);
 }
--- a/flake.lock
+++ b/flake.lock
@@ -1,2 +1,2 @@
-{"nodes": {}}
+{"nodes": {"test": {}}}
"#;

    #[test]
    fn test_parse_diff_and_valid_lines() {
        let diff = ParsedDiff::parse(SAMPLE_DIFF).expect("Diff parsing should succeed");
        assert_eq!(diff.files.len(), 2);

        let main_file = &diff.files[0];
        assert_eq!(main_file.new_path, "src/main.rs");
        assert!(diff.is_valid_line("src/main.rs", 2));
        assert!(diff.is_valid_line("src/main.rs", 3));
        assert!(diff.is_valid_line("src/main.rs", 1));
        assert!(diff.is_valid_line("src/main.rs", 4));
        assert!(!diff.is_valid_line("src/main.rs", 99));
    }

    #[test]
    fn test_apply_filters() {
        let mut diff = ParsedDiff::parse(SAMPLE_DIFF).unwrap();
        diff.apply_filters(&["**/flake.lock".to_string()]);
        assert_eq!(diff.files.len(), 1);
        assert_eq!(diff.files[0].new_path, "src/main.rs");
    }

    #[test]
    fn test_added_lines_vs_context_lines() {
        let diff = ParsedDiff::parse(SAMPLE_DIFF).unwrap();
        assert!(diff.is_valid_line("src/main.rs", 1));
        assert!(!diff.is_added_line("src/main.rs", 1));
        assert!(diff.is_added_line("src/main.rs", 2));
        assert!(diff.is_added_line("src/main.rs", 3));
        assert_eq!(
            diff.find_first_added_line_in_range("src/main.rs", 1, 4),
            Some(2)
        );
        assert_eq!(
            diff.find_first_added_line_in_range("src/main.rs", 1, 1),
            None
        );
    }

    #[test]
    fn test_format_with_budget_omits_generated_lockfiles() {
        let diff = ParsedDiff::parse(SAMPLE_DIFF).expect("Diff parsing should succeed");
        let (formatted, stats) = diff.format_with_budget(24_000);

        assert_eq!(stats.total_files, 2);
        assert_eq!(stats.included_files, 1);
        assert_eq!(stats.omitted_generated_files, 1);
        assert!(formatted.contains("=== FILE OMITTED (LOCKFILE / GENERATED): flake.lock ==="));
        assert!(formatted.contains("=== MODIFIED FILE: src/main.rs ==="));
    }

    #[test]
    fn test_format_with_budget_prioritization_and_truncation() {
        const MULTI_DIFF: &str = r#"--- a/docs/guide.md
+++ b/docs/guide.md
@@ -1,1 +1,2 @@
 # Documentation
+New documentation section.
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,2 +1,3 @@
 pub fn hello() {
+    println!("core logic");
 }
"#;
        let diff = ParsedDiff::parse(MULTI_DIFF).expect("Diff parsing should succeed");
        let (formatted, stats) = diff.format_with_budget(25);

        assert_eq!(stats.total_files, 2);
        assert_eq!(stats.included_files, 1);
        assert_eq!(stats.omitted_budget_files, 1);
        assert!(formatted.contains("=== MODIFIED FILE: src/lib.rs ==="));
        assert!(formatted.contains("=== FILE OMITTED (EXCEEDS TOKEN BUDGET): docs/guide.md"));
    }

    #[test]
    fn test_format_with_budget_truncation_strictly_within_character_limit() {
        const SINGLE_DIFF: &str = r#"--- a/src/long.rs
+++ b/src/long.rs
@@ -1,5 +1,10 @@
 line 1
+line 2
+line 3
+line 4
+line 5
"#;
        let diff = ParsedDiff::parse(SINGLE_DIFF).expect("Diff parsing should succeed");
        let (formatted, stats) = diff.format_with_budget(25);
        assert_eq!(stats.included_files, 1);
        assert!(
            formatted.len() <= 25 * 4,
            "formatted length {} exceeded 100",
            formatted.len()
        );
    }

    #[test]
    fn test_format_with_budget_deleted_file_unified_header() {
        const DELETED_SOURCE: &str = r#"--- a/src/old.rs
+++ /dev/null
@@ -1,2 +0,0 @@
-fn deprecated() {}
"#;
        let diff = ParsedDiff::parse(DELETED_SOURCE).expect("Diff parsing should succeed");
        let (formatted, stats) = diff.format_with_budget(24_000);
        assert_eq!(stats.included_files, 1);
        assert!(formatted.contains("+++ /dev/null\n"));
        assert!(!formatted.contains("+++ b//dev/null"));
    }

    #[test]
    fn test_classify_file_priority_precision() {
        assert_eq!(classify_file_priority("src/inspector.rs"), 1);
        assert_eq!(classify_file_priority("src/latest.rs"), 1);
        assert_eq!(classify_file_priority("src/specification.rs"), 1);

        assert_eq!(classify_file_priority("tests/eval_benchmark.rs"), 2);
        assert_eq!(classify_file_priority("src/worker_test.rs"), 2);
        assert_eq!(classify_file_priority("src/auth.spec.ts"), 2);
    }

    #[test]
    fn test_format_with_budget_deleted_file_classification() {
        const DELETED_DIFF: &str = r#"--- a/Cargo.lock
+++ /dev/null
@@ -1,3 +0,0 @@
-version = 3
-[[package]]
-name = "old_pkg"
"#;
        let diff = ParsedDiff::parse(DELETED_DIFF).expect("Diff parsing should succeed");
        let (formatted, stats) = diff.format_with_budget(24_000);

        assert_eq!(stats.total_files, 1);
        assert_eq!(stats.omitted_generated_files, 1);
        assert!(formatted.contains("=== FILE OMITTED (LOCKFILE / GENERATED): Cargo.lock ==="));
    }

    #[test]
    fn test_is_likely_generated_node_modules_segment() {
        assert!(is_likely_generated_or_lockfile("node_modules/pkg/index.js"));
        assert!(is_likely_generated_or_lockfile(
            "packages/web/node_modules/pkg/index.js"
        ));
        assert!(!is_likely_generated_or_lockfile("my_node_modules/file.rs"));
        assert!(!is_likely_generated_or_lockfile(
            "src/node_modules_helper.rs"
        ));
    }

    #[test]
    fn test_format_with_budget_zero_tokens_clamps_safely() {
        let diff = ParsedDiff::parse("--- a/f.txt\n+++ b/f.txt\n@@ -1 +1 @@\n-a\n+b\n").unwrap();
        let (_, stats) = diff.format_with_budget(0);
        assert_eq!(stats.total_files, 1);
        assert_eq!(stats.omitted_budget_files, 1);
    }

    const TEXT_STANZA: &str = "diff --git a/src/lib.rs b/src/lib.rs
index 1111111..2222222 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,2 +1,3 @@
 fn a() {}
+fn b() {}
 fn c() {}
";
    const BINARY_STANZA: &str = "diff --git a/assets/logo.png b/assets/logo.png
index 3333333..4444444 100644
Binary files a/assets/logo.png and b/assets/logo.png differ
";
    const RENAME_STANZA: &str = "diff --git a/old.txt b/new.txt
similarity index 100%
rename from old.txt
rename to new.txt
";
    const MODE_STANZA: &str = "diff --git a/run.sh b/run.sh
old mode 100644
new mode 100755
";

    #[test]
    fn test_parse_binary_stanza_last() {
        let diff = ParsedDiff::parse(&format!("{TEXT_STANZA}{BINARY_STANZA}")).unwrap();
        assert_eq!(diff.files.len(), 1);
        assert!(diff.is_added_line("src/lib.rs", 2));
    }

    #[test]
    fn test_parse_binary_stanza_first_keeps_following_file() {
        let diff = ParsedDiff::parse(&format!("{BINARY_STANZA}{TEXT_STANZA}")).unwrap();
        assert_eq!(diff.files.len(), 1);
        assert_eq!(diff.files[0].new_path, "src/lib.rs");
        assert!(diff.is_added_line("src/lib.rs", 2));
    }

    #[test]
    fn test_parse_only_hunkless_stanzas_is_empty() {
        for text in [
            BINARY_STANZA.to_string(),
            RENAME_STANZA.to_string(),
            MODE_STANZA.to_string(),
            format!("{BINARY_STANZA}{RENAME_STANZA}{MODE_STANZA}"),
        ] {
            let diff = ParsedDiff::parse(&text).unwrap();
            assert!(diff.files.is_empty(), "{text}");
        }
    }

    #[test]
    fn test_parse_hunkless_stanzas_between_text_files() {
        let second = TEXT_STANZA.replace("src/lib.rs", "src/two.rs");
        let diff = ParsedDiff::parse(&format!(
            "{TEXT_STANZA}{RENAME_STANZA}{MODE_STANZA}{BINARY_STANZA}{second}"
        ))
        .unwrap();
        let paths: Vec<_> = diff.files.iter().map(|f| f.new_path.as_str()).collect();
        assert_eq!(paths, ["src/lib.rs", "src/two.rs"]);
    }

    #[test]
    fn test_parse_rejects_text_stanza_without_hunk() {
        let truncated =
            "diff --git a/src/x.rs b/src/x.rs\nindex 1..2 100644\n--- a/src/x.rs\n+++ b/src/x.rs\n";
        let err = ParsedDiff::parse(&format!("{TEXT_STANZA}{truncated}"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("b/src/x.rs"), "{err}");
    }

    #[test]
    fn test_parse_empty_diff() {
        assert!(ParsedDiff::parse("").unwrap().files.is_empty());
    }

    #[test]
    fn test_parse_multi_hunk_and_no_newline_marker() {
        const MULTI_HUNK: &str = "\
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,3 +1,4 @@
 fn first() {
+    let x = 1;
 }
-// old tail
\\ No newline at end of file
+// new tail
\\ No newline at end of file
@@ -10,2 +11,3 @@
 fn second() {
+    let y = 2;
 }
";
        let diff = ParsedDiff::parse(MULTI_HUNK).unwrap();
        assert_eq!(diff.files.len(), 1);
        assert!(diff.is_valid_line("src/lib.rs", 1));
        assert!(diff.is_added_line("src/lib.rs", 2));
        assert!(diff.is_added_line("src/lib.rs", 4));
        assert!(!diff.is_valid_line("src/lib.rs", 7));
        assert!(diff.is_valid_line("src/lib.rs", 11));
        assert!(diff.is_added_line("src/lib.rs", 12));
        assert_eq!(
            diff.find_first_added_line_in_range("src/lib.rs", 10, 13),
            Some(12)
        );
    }

    #[test]
    fn test_parse_new_file_and_rename_with_edit() {
        const NEW_AND_RENAMED: &str = "\
diff --git a/dev/null b/src/created.rs
new file mode 100644
--- /dev/null
+++ b/src/created.rs
@@ -0,0 +1,2 @@
+pub fn created() {}
+pub const VERSION: u32 = 1;
diff --git \"a/src/old name.rs\" \"b/src/new name.rs\"
similarity index 80%
rename from src/old name.rs
rename to src/new name.rs
--- \"a/src/old name.rs\"\t2026-01-01 00:00:00 +0000
+++ \"b/src/new name.rs\"\t2026-01-01 00:00:00 +0000
@@ -1,2 +1,2 @@
-pub fn old_name() {}
+pub fn new_name() {}
 pub fn shared() {}
";
        let diff = ParsedDiff::parse(NEW_AND_RENAMED).unwrap();
        assert_eq!(diff.files.len(), 2);
        assert_eq!(diff.files[0].old_path.as_deref(), None);
        assert_eq!(diff.files[0].new_path, "src/created.rs");
        assert!(diff.is_added_line("src/created.rs", 1));
        assert!(diff.is_added_line("src/created.rs", 2));

        assert_eq!(diff.files[1].old_path.as_deref(), Some("src/old name.rs"));
        assert_eq!(diff.files[1].new_path, "src/new name.rs");
        assert!(diff.is_added_line("src/new name.rs", 1));
        assert!(!diff.is_added_line("src/new name.rs", 2));

        let (formatted, stats) = diff.format_with_budget(10_000);
        assert_eq!(stats.included_files, 2);
        assert!(formatted.contains("--- /dev/null\n+++ b/src/created.rs\n"));
        assert!(formatted.contains("--- a/src/old name.rs\n+++ b/src/new name.rs\n"));
    }

    #[test]
    fn test_parse_malformed_hunk_header_errors_without_panic() {
        let bad = "--- a/src/x.rs\n+++ b/src/x.rs\n@@ -not,a,number +1,2 @@\n+oops\n";
        assert!(ParsedDiff::parse(bad).is_err());
    }

    #[test]
    fn test_format_with_budget_includes_surrounding_context() {
        let mut diff =
            ParsedDiff::parse("--- a/src/ctx.rs\n+++ b/src/ctx.rs\n@@ -2 +2 @@\n-old\n+new\n")
                .unwrap();
        diff.files[0].file_context = Some("   1 | fn wrapper() {\n   2 |     old\n   3 | }".into());
        let (formatted, _) = diff.format_with_budget(1_000);
        assert!(formatted.contains("[SURROUNDING FILE CONTEXT FOR src/ctx.rs"));
        assert!(formatted.contains("fn wrapper()"));
    }

    #[test]
    fn test_hunk_lines_resembling_file_headers_and_git_quoted_octal_paths() {
        let patch = concat!(
            "diff --git \"a/src/caf\\303\\251\\\\tab.rs\" \"b/src/caf\\303\\251\\\\tab.rs\"\n",
            "--- \"a/src/caf\\303\\251\\\\tab.rs\"\t2026-01-01 00:00:00 +0000\n",
            "+++ \"b/src/caf\\303\\251\\\\tab.rs\"\t2026-01-01 00:00:00 +0000\n",
            "@@ -1,3 +1,3 @@\n",
            " fn start() {}\n",
            "--- old sql comment\n",
            "+++ new increment\n",
            " fn end() {}\n",
        );
        let parsed = ParsedDiff::parse(patch).expect("Should parse hunk with --/+ lines");
        assert_eq!(parsed.files.len(), 1);
        assert_eq!(parsed.files[0].new_path, "src/café\\tab.rs");
        assert_eq!(parsed.files[0].hunks.len(), 1);
        assert!(parsed.is_added_line("src/café\\tab.rs", 2));
        assert!(parsed.is_valid_line("src/café\\tab.rs", 3));
    }
}
