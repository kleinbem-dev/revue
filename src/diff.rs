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
        let patches = patch::Patch::from_multiple(diff_text)
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
            let mut hunks = Vec::new();

            for h in &p.hunks {
                let mut current_new_line = h.new_range.start;
                let mut hunk_lines = Vec::new();

                for line in &h.lines {
                    match line {
                        patch::Line::Add(content) => {
                            valid_lines.insert(current_new_line);
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
                if let Ok(matcher) = glob::Pattern::new(&glob_pattern) {
                    if matcher.matches(&f.new_path) {
                        tracing::debug!("Ignoring filtered file: {}", f.new_path);
                        return false;
                    }
                }
            }
            true
        });
    }

    /// Enrich diff with actual surrounding file context from disk (like CodeRabbit AST context)
    pub fn enrich_with_disk_context(&mut self, root_dir: Option<&std::path::Path>) {
        let base = root_dir.unwrap_or_else(|| std::path::Path::new("."));

        for f in &mut self.files {
            let file_path = base.join(&f.new_path);
            if file_path.is_file() {
                if let Ok(content) = std::fs::read_to_string(&file_path) {
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
    }

    /// Check if a proposed comment line is within the diff hunk additions/modifications
    pub fn is_valid_line(&self, file_path: &str, line: u64) -> bool {
        self.files
            .iter()
            .find(|f| f.new_path == file_path)
            .map(|f| f.valid_lines.contains(&line))
            .unwrap_or(false)
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
}
