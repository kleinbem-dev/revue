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
