use anyhow::Result;
use std::collections::HashSet;

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
    pub raw_patch: String,
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

impl ParsedDiff {
    /// Parse unified diff text (e.g. from `git diff` or Forgejo API)
    pub fn parse(diff_text: &str) -> Result<Self> {
        let text = keep_stanzas_with_hunks(diff_text)?;
        if !text.lines().any(|l| l.starts_with("@@")) {
            return Ok(Self { files: Vec::new() });
        }
        // patch 0.7 asserts on input it can't consume; a review should fail, not panic.
        // upstream: uniphil/patch-rs#24, uniphil/patch-rs#25
        let patches = std::panic::catch_unwind(|| patch::Patch::from_multiple(&text))
            .map_err(|_| anyhow::anyhow!("Failed to parse unified diff: parser panicked"))?
            .map_err(|e| anyhow::anyhow!("Failed to parse unified diff: {}", e))?;

        let mut files = Vec::new();

        for p in patches {
            let new_path = clean_diff_path(&p.new.path);
            let old_path = if p.old.path == "/dev/null" {
                None
            } else {
                Some(clean_diff_path(&p.old.path))
            };

            let mut valid_lines = HashSet::new();
            let mut added_lines = HashSet::new();
            let mut hunks = Vec::new();

            for h in &p.hunks {
                let mut current_new_line = h.new_range.start;
                let mut hunk_lines = Vec::new();

                for line in &h.lines {
                    match line {
                        patch::Line::Add(content) => {
                            valid_lines.insert(current_new_line);
                            added_lines.insert(current_new_line);
                            hunk_lines.push(format!("+{}", content));
                            current_new_line += 1;
                        }
                        patch::Line::Remove(content) => {
                            hunk_lines.push(format!("-{}", content));
                        }
                        patch::Line::Context(content) => {
                            valid_lines.insert(current_new_line);
                            hunk_lines.push(format!(" {}", content));
                            current_new_line += 1;
                        }
                    }
                }

                hunks.push(DiffHunk {
                    old_start: h.old_range.start,
                    old_lines: h.old_range.count,
                    new_start: h.new_range.start,
                    new_lines: h.new_range.count,
                    lines: hunk_lines,
                });
            }

            files.push(FilePatch {
                old_path,
                new_path,
                hunks,
                valid_lines,
                added_lines,
                raw_patch: format!("{:?}", p),
                file_context: None,
            });
        }

        Ok(Self { files })
    }

    /// Filter files matching ignore glob patterns (like `**/flake.lock`)
    pub fn apply_filters(&mut self, filters: &[String]) {
        if filters.is_empty() {
            return;
        }

        self.files.retain(|f| {
            for pattern in filters {
                let clean_pattern = pattern.strip_prefix('!').unwrap_or(pattern);
                let glob_pattern = clean_glob(clean_pattern);
                if let Ok(matcher) = glob::Pattern::new(&glob_pattern)
                    && matcher.matches(&f.new_path)
                {
                    tracing::debug!("Ignoring filtered file: {}", f.new_path);
                    return false;
                }
            }
            true
        });
    }

    /// Enrich diff with actual surrounding file context from disk (like CodeRabbit AST context)
    pub fn enrich_with_disk_context(&mut self, root_dir: Option<&std::path::Path>) {
        let base = root_dir.unwrap_or_else(|| std::path::Path::new("."));
        let canonical_base = base.canonicalize().ok();

        for f in &mut self.files {
            let file_path = base.join(&f.new_path);
            if let Some(ref root) = canonical_base {
                if let Ok(canon) = file_path.canonicalize() {
                    if !canon.starts_with(root) {
                        tracing::warn!(
                            "Rejecting disk context path escaping base directory: {:?}",
                            file_path
                        );
                        continue;
                    }
                } else {
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
                    let mut included_lines = std::collections::BTreeSet::new();
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

    /// Check if a proposed comment line is within the diff hunk additions/modifications
    pub fn is_valid_line(&self, file_path: &str, line: u64) -> bool {
        self.files
            .iter()
            .find(|f| f.new_path == file_path)
            .map(|f| f.valid_lines.contains(&line))
            .unwrap_or(false)
    }

    /// Check if a proposed line is strictly an added line in the diff
    pub fn is_added_line(&self, file_path: &str, line: u64) -> bool {
        self.files
            .iter()
            .find(|f| f.new_path == file_path)
            .map(|f| f.added_lines.contains(&line))
            .unwrap_or(false)
    }

    /// Find the first added line within a span [start, end]
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

    /// Formats diff and surrounding file context under an estimated token budget.
    /// Files are prioritized by semantic importance (source > tests/configs > docs).
    /// Generated lockfiles and bundles are omitted with clear summaries.
    pub fn format_with_budget(&self, max_tokens: usize) -> (String, DiffBudgetStats) {
        let mut stats = DiffBudgetStats {
            total_files: self.files.len(),
            ..Default::default()
        };

        // Standard token heuristic: ~4 characters per token; reject zero budget
        assert!(max_tokens > 0, "max_tokens must be strictly greater than zero");
        let max_chars = max_tokens.saturating_mul(4);
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

        // Emit concise summary for omitted generated lockfiles
        for generated in generated_files {
            stats.omitted_generated_files += 1;
            let note = format!(
                "=== FILE OMITTED (LOCKFILE / GENERATED): {} ===\n\n",
                effective_path(generated)
            );
            total_chars += note.len();
            out.push_str(&note);
        }

        // Sort reviewable files by priority (1 = Core source, 2 = Tests/config, 3 = Docs)
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
            let new_header = if f.new_path == "/dev/null" {
                "/dev/null".to_string()
            } else {
                format!("b/{}", f.new_path)
            };
            let file_header = format!(
                "=== MODIFIED FILE: {} ===\n--- a/{}\n+++ {}\n",
                path_display,
                f.old_path.as_deref().unwrap_or(path_display),
                new_header
            );

            // If we cannot even fit the file header, omit the file
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

            // Reserve space for truncation marker and final newline if we need to truncate
            const TRUNC_MARKER_EST: usize = 30;
            const FINAL_NEWLINE: usize = 1;

            for (hi, h) in f.hunks.iter().enumerate() {
                let hunk_header = format!(
                    "@@ -{},{} +{},{} @@\n",
                    h.old_start, h.old_lines, h.new_start, h.new_lines
                );

                if file_buf.len() + hunk_header.len() + TRUNC_MARKER_EST + FINAL_NEWLINE > remaining_chars {
                    truncated_hunks = true;
                    skipped_hunk_lines += f.hunks[hi..].iter().map(|x| x.lines.len()).sum::<usize>();
                    break;
                }
                file_buf.push_str(&hunk_header);

                for (li, l) in h.lines.iter().enumerate() {
                    if file_buf.len() + l.len() + 1 + TRUNC_MARKER_EST + FINAL_NEWLINE > remaining_chars {
                        truncated_hunks = true;
                        skipped_hunk_lines += h.lines.len() - li;
                        skipped_hunk_lines += f.hunks[hi + 1..].iter().map(|x| x.lines.len()).sum::<usize>();
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
                if file_buf.len() + ctx_header.len() + ctx.len() + FINAL_NEWLINE <= remaining_chars {
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

    // Common package/dependency lockfiles
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

    // Minified files, sourcemaps, generated bundles
    if basename.ends_with(".min.js")
        || basename.ends_with(".min.css")
        || basename.ends_with(".map")
        || basename.ends_with(".bundle.js")
    {
        return true;
    }

    // Common vendor directories
    if path.starts_with("vendor/")
        || path.contains("/vendor/")
        || path.starts_with("node_modules/")
        || path.contains("/node_modules/")
    {
        return true;
    }

    false
}

/// Semantic review priority: 1 = core logic/source, 2 = tests & configuration, 3 = documentation & text.
pub fn classify_file_priority(path: &str) -> u8 {
    let lower = path.to_lowercase();
    let ext = lower.rsplit('.').next().unwrap_or("");

    // Test files and test directories (by path segment and prefix/suffix to avoid matching words like inspector or specification)
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

    // Config files
    if matches!(ext, "toml" | "yaml" | "yml" | "json" | "env" | "ini" | "cfg") {
        return 2;
    }

    // Documentation, markdown, plain text, assets
    if matches!(ext, "md" | "txt" | "rst" | "html" | "svg" | "png" | "jpg" | "jpeg" | "ico") {
        return 3;
    }

    // Core executable source code
    if matches!(
        ext,
        "rs" | "go" | "py" | "ts" | "js" | "svelte" | "c" | "cpp" | "h" | "hpp" | "zig" | "nix"
            | "sh" | "bash" | "sql" | "java" | "kt" | "swift" | "rb" | "php" | "cs"
    ) {
        return 1;
    }

    2
}

    /// Counts added and removed lines across all hunks in a patch.
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

/// Returns the review path, resolving `/dev/null` deleted files to their original `old_path`.
fn effective_path(f: &FilePatch) -> &str {
    if f.new_path == "/dev/null" {
        f.old_path.as_deref().unwrap_or(&f.new_path)
    } else {
        &f.new_path
    }
}

/// Drops `diff --git` stanzas without a hunk (binary files, pure renames, mode changes):
/// the patch crate needs a hunk per file and panics on a trailing hunkless stanza.
/// A stanza with `---`/`+++` headers but no hunk is a truncated diff and an error.
fn keep_stanzas_with_hunks(diff_text: &str) -> Result<std::borrow::Cow<'_, str>> {
    if !diff_text.lines().any(|l| l.starts_with("diff --git ")) {
        return Ok(std::borrow::Cow::Borrowed(diff_text));
    }
    let mut kept = String::with_capacity(diff_text.len());
    let mut stanzas = Vec::new();
    let mut stanza = String::new();
    for line in diff_text.split_inclusive('\n') {
        if line.starts_with("diff --git ") && !stanza.is_empty() {
            stanzas.push(std::mem::take(&mut stanza));
        }
        stanza.push_str(line);
    }
    stanzas.push(stanza);
    for stanza in stanzas {
        if stanza.lines().any(|l| l.starts_with("@@")) {
            kept.push_str(&stanza);
        } else if let Some(header) = stanza.lines().find(|l| l.starts_with("+++ ")) {
            return Err(anyhow::anyhow!(
                "Incomplete diff: `{}` has file headers but no hunk",
                header.trim_start_matches("+++ ").trim()
            ));
        }
    }
    Ok(std::borrow::Cow::Owned(kept))
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
@@ -1,4 +1,5 @@
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
        // Lines 2 and 3 are additions
        assert!(diff.is_valid_line("src/main.rs", 2));
        assert!(diff.is_valid_line("src/main.rs", 3));
        // Line 99 does not exist
        assert!(!diff.is_valid_line("src/main.rs", 99));
    }

    #[test]
    fn test_apply_filters() {
        let mut diff = ParsedDiff::parse(SAMPLE_DIFF).expect("Diff parsing should succeed");
        assert_eq!(diff.files.len(), 2);

        let filters = vec!["**/flake.lock".to_string()];
        diff.apply_filters(&filters);

        assert_eq!(diff.files.len(), 1);
        assert_eq!(diff.files[0].new_path, "src/main.rs");
    }

    #[test]
    fn test_added_lines_vs_context_lines() {
        let diff = ParsedDiff::parse(SAMPLE_DIFF).expect("Diff parsing should succeed");
        // Line 1 is context line: valid in file, but not an added line
        assert!(diff.is_valid_line("src/main.rs", 1));
        assert!(!diff.is_added_line("src/main.rs", 1));

        // Line 2 and 3 are additions
        assert!(diff.is_added_line("src/main.rs", 2));
        assert!(diff.is_added_line("src/main.rs", 3));

        // Range starting on context line 1 spans into addition on line 2
        assert_eq!(
            diff.find_first_added_line_in_range("src/main.rs", 1, 4),
            Some(2)
        );
        // Range completely outside additions
        assert_eq!(
            diff.find_first_added_line_in_range("src/main.rs", 1, 1),
            None
        );
    }

    /// Verifies that lockfiles are automatically suppressed from prompt diffs.
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

    /// Verifies priority-based inclusion when token budget is limited.
    #[test]
    fn test_format_with_budget_prioritization_and_truncation() {
        const MULTI_DIFF: &str = r#"--- a/docs/guide.md
+++ b/docs/guide.md
@@ -1,2 +1,3 @@
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
        // Limit budget strictly to ~25 tokens (~100 chars) so second file is omitted
        let (formatted, stats) = diff.format_with_budget(25);

        assert_eq!(stats.total_files, 2);
        assert_eq!(stats.included_files, 1);
        assert_eq!(stats.omitted_budget_files, 1);
        // Core source code included first, documentation omitted due to budget
        assert!(formatted.contains("=== MODIFIED FILE: src/lib.rs ==="));
        assert!(formatted.contains("=== FILE OMITTED (EXCEEDS TOKEN BUDGET): docs/guide.md"));
    }

    /// Verifies that a truncated file stays strictly within its character budget.
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
        // 25 tokens = 100 character budget limit
        let (formatted, stats) = diff.format_with_budget(25);
        assert_eq!(stats.included_files, 1);
        assert!(
            formatted.len() <= 25 * 4,
            "formatted length {} exceeded 100",
            formatted.len()
        );
    }

    /// Verifies that a deleted non-lockfile emits direct +++ /dev/null header without 'b/' prefix.
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
        assert!(
            formatted.contains("+++ /dev/null\n"),
            "must contain direct +++ /dev/null without b/ prefix"
        );
        assert!(!formatted.contains("+++ b//dev/null"));
    }

    /// Verifies that words containing substrings like 'test' or 'spec' do not falsely demote core source files.
    #[test]
    fn test_classify_file_priority_precision() {
        // Core source files containing 'test' or 'spec' substrings must remain priority 1
        assert_eq!(classify_file_priority("src/inspector.rs"), 1);
        assert_eq!(classify_file_priority("src/latest.rs"), 1);
        assert_eq!(classify_file_priority("src/specification.rs"), 1);

        // Actual test files and directories must receive priority 2
        assert_eq!(classify_file_priority("tests/eval_benchmark.rs"), 2);
        assert_eq!(classify_file_priority("src/worker_test.rs"), 2);
        assert_eq!(classify_file_priority("src/auth.spec.ts"), 2);
    }

    /// Verifies that deleted lockfiles (where new_path is /dev/null) are properly identified and suppressed via old_path.
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

    /// Verifies that node_modules is only matched as a whole path segment.
    #[test]
    fn test_is_likely_generated_node_modules_segment() {
        assert!(is_likely_generated_or_lockfile("node_modules/pkg/index.js"));
        assert!(is_likely_generated_or_lockfile("packages/web/node_modules/pkg/index.js"));
        assert!(!is_likely_generated_or_lockfile("my_node_modules/file.rs"));
        assert!(!is_likely_generated_or_lockfile("src/node_modules_helper.rs"));
    }

    /// Verifies that passing zero max_tokens immediately triggers an assertion failure.
    #[test]
    #[should_panic(expected = "max_tokens must be strictly greater than zero")]
    fn test_format_with_budget_zero_tokens_panics() {
        let diff = ParsedDiff::parse("--- a/f.txt\n+++ b/f.txt\n@@ -1 +1 @@\n-a\n+b\n").unwrap();
        let _ = diff.format_with_budget(0);
    }

    // git extended headers: stanzas with no hunk (binary, rename, mode change).
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
}
