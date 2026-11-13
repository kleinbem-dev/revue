use std::path::{Path, PathBuf};
use std::time::Duration;
use tracing::{debug, info, warn};

/// Resolves the filesystem root of the given repository against current working directory or fleet roots.
pub fn resolve_repo_root(repo_arg: &str) -> Option<PathBuf> {
    // Reject path traversal components, git metadata, and disallowed hidden components (allow exact .github)
    if repo_arg.split('/').any(|seg| {
        seg == ".." || seg == "." || seg == ".git" || (seg.starts_with('.') && seg != ".github")
    }) {
        return None;
    }

    let repo_basename = repo_arg.rsplit('/').next().unwrap_or(repo_arg);
    if repo_basename.is_empty()
        || repo_basename == ".git"
        || (repo_basename != ".github"
            && !repo_basename
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.'))
    {
        return None;
    }

    // 1. Current working directory
    let cwd = PathBuf::from(".");
    if (cwd.join(".git").exists() || cwd.join(".jj").exists())
        && let Ok(canon) = cwd.canonicalize()
        && canon.file_name().and_then(|n| n.to_str()) == Some(repo_basename)
    {
        return Some(canon);
    }

    // 2. Explicit workspace root from env (REVUE_WORKSPACE_ROOT, or legacy KLEINBEM_WORKSPACE_ROOT)
    for var in ["REVUE_WORKSPACE_ROOT", "KLEINBEM_WORKSPACE_ROOT"] {
        if let Ok(root) = std::env::var(var)
            && !root.trim().is_empty()
        {
            let base = PathBuf::from(root);
            for candidate in [base.join(repo_arg), base.join(repo_basename)] {
                if candidate.is_dir() {
                    return Some(candidate);
                }
            }
        }
    }

    // 3. Conventional developer checkout layouts under $HOME when owner/repo is supplied
    if repo_arg.contains('/')
        && let Ok(home) = std::env::var("HOME")
    {
        let home_path = PathBuf::from(home);
        for prefix in ["Develop/github.com", "src/github.com"] {
            let p = home_path.join(prefix).join(repo_arg);
            if p.is_dir() {
                return Some(p);
            }
        }
    }

    None
}

/// Recursively inspects the extracted directory and unlinks any symlinks pointing outside the tree root.
fn sanitize_extracted_tree(root: &Path) {
    let canonical_root = match root.canonicalize() {
        Ok(c) => c,
        Err(_) => return,
    };

    /// Recursively visits filesystem entries to unlink escaping symlinks.
    fn visit_dir(dir: &Path, canonical_root: &Path) {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if let Ok(ft) = entry.file_type() {
                    if ft.is_symlink() {
                        let safe = path
                            .canonicalize()
                            .map(|target| target.starts_with(canonical_root))
                            .unwrap_or(false);
                        if !safe {
                            warn!(
                                "Removing unsafe symlink escaping ephemeral tree: {}",
                                path.display()
                            );
                            let _ = std::fs::remove_file(&path);
                        }
                    } else if ft.is_dir() {
                        visit_dir(&path, canonical_root);
                    }
                }
            }
        }
    }

    visit_dir(root, &canonical_root);
}

/// A scoped temporary directory holding a checkout of the repository at a specific commit SHA.
/// Automatically cleans up the directory when dropped.
pub struct EphemeralTreeGuard {
    pub path: PathBuf,
}

impl Drop for EphemeralTreeGuard {
    fn drop(&mut self) {
        if self.path.exists() {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}

impl AsRef<Path> for EphemeralTreeGuard {
    fn as_ref(&self) -> &Path {
        &self.path
    }
}

impl EphemeralTreeGuard {
    /// Extracts a clean tree for `head_sha` into a temporary directory without modifying working copies.
    pub async fn prepare(repo_name: &str, head_sha: &str, pr_number: Option<u64>) -> Option<Self> {
        let repo_root = resolve_repo_root(repo_name)?;
        Self::prepare_from_root(&repo_root, repo_name, head_sha, pr_number).await
    }

    pub(crate) async fn prepare_from_root(
        repo_root: &Path,
        repo_name: &str,
        head_sha: &str,
        pr_number: Option<u64>,
    ) -> Option<Self> {
        // Reject invalid or non-hexadecimal git SHAs
        if head_sha.len() < 7
            || head_sha.len() > 64
            || !head_sha.bytes().all(|b| b.is_ascii_hexdigit())
        {
            warn!(
                "Invalid git head SHA '{}'; skipping ephemeral tree extraction",
                head_sha
            );
            return None;
        }

        // Ensure the commit exists in the local git repository; fetch if absent
        let has_commit = match tokio::time::timeout(
            Duration::from_secs(10),
            tokio::process::Command::new("git")
                .current_dir(repo_root)
                .args(["cat-file", "-e", head_sha])
                .output(),
        )
        .await
        {
            Ok(Ok(o)) => o.status.success(),
            _ => false,
        };

        if !has_commit {
            let mut fetch_cmd = tokio::process::Command::new("git");
            fetch_cmd
                .current_dir(repo_root)
                .env("GIT_TERMINAL_PROMPT", "0")
                .env("GIT_ASKPASS", "");

            if let Some(pr) = pr_number {
                info!(
                    "Commit '{}' not found in local repo; fetching pull/{}/head from origin...",
                    head_sha, pr
                );
                let refspec = format!("refs/pull/{}/head", pr);
                fetch_cmd.args(["fetch", "origin", &refspec]);
            } else {
                fetch_cmd.args(["fetch", "origin", head_sha]);
            }

            fetch_cmd.kill_on_drop(true);
            match tokio::time::timeout(Duration::from_secs(30), fetch_cmd.output()).await {
                Ok(Ok(o)) if o.status.success() => {}
                Ok(Ok(o)) => warn!("git fetch failed with status {}", o.status),
                Ok(Err(e)) => warn!("git fetch could not run: {}", e),
                Err(_) => warn!("git fetch timed out after 30s"),
            }
        }

        // Verify that head_sha exists locally after fetch
        let has_commit_after = match tokio::time::timeout(
            Duration::from_secs(10),
            tokio::process::Command::new("git")
                .current_dir(repo_root)
                .args(["cat-file", "-e", head_sha])
                .output(),
        )
        .await
        {
            Ok(Ok(o)) => o.status.success(),
            _ => false,
        };

        if !has_commit_after {
            warn!(
                "Commit '{}' remains absent in local repo for {}; skipping ephemeral tree",
                head_sha, repo_name
            );
            return None;
        }

        static INVOCATION_COUNTER: std::sync::atomic::AtomicU64 =
            std::sync::atomic::AtomicU64::new(1);
        let count = INVOCATION_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);

        let temp_base = std::env::temp_dir();
        let temp_dir = temp_base.join(format!(
            "revue-tree-{}-{}-{}-{}-{}",
            repo_name.rsplit('/').next().unwrap_or("repo"),
            &head_sha[..head_sha.len().min(8)],
            std::process::id(),
            nanos,
            count
        ));

        let mut builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        if let Err(e) = builder.create(&temp_dir) {
            warn!("Failed to create temporary directory for PR tree: {}", e);
            return None;
        }

        // Extract the exact tree at head_sha using an isolated temporary index
        let temp_index = temp_base.join(format!(
            "revue-index-{}-{}-{}-{}-{}.tmp",
            repo_name.rsplit('/').next().unwrap_or("repo"),
            &head_sha[..head_sha.len().min(8)],
            std::process::id(),
            nanos,
            count
        ));

        let extract_pipeline = async {
            // Isolate git from global/system smudge filters and disable symlink creation on checkout.
            let read_status = tokio::process::Command::new("git")
                .current_dir(repo_root)
                .env("GIT_INDEX_FILE", &temp_index)
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .args(["read-tree", head_sha])
                .kill_on_drop(true)
                .status()
                .await?;

            if !read_status.success() {
                return Err(std::io::Error::other(format!(
                    "git read-tree exited with status {}",
                    read_status
                )));
            }

            let prefix_arg = format!("{}/", temp_dir.display());
            let checkout_status = tokio::process::Command::new("git")
                .current_dir(repo_root)
                .env("GIT_INDEX_FILE", &temp_index)
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .args([
                    "-c",
                    "core.autocrlf=false",
                    "-c",
                    "core.symlinks=false",
                    "checkout-index",
                    "-a",
                    "-f",
                    &format!("--prefix={}", prefix_arg),
                ])
                .kill_on_drop(true)
                .status()
                .await?;

            if !checkout_status.success() {
                return Err(std::io::Error::other(format!(
                    "git checkout-index exited with status {}",
                    checkout_status
                )));
            }

            Ok(())
        };

        let extract_result =
            match tokio::time::timeout(Duration::from_secs(30), extract_pipeline).await {
                Ok(res) => res,
                Err(_) => Err(std::io::Error::other("tree checkout timed out after 30s")),
            };

        // Always clean up the temporary index file
        if temp_index.exists() {
            let _ = tokio::fs::remove_file(&temp_index).await;
        }

        match extract_result {
            Ok(()) => {
                sanitize_extracted_tree(&temp_dir);
                debug!(
                    "Successfully prepared ephemeral tree for {}@{} at {}",
                    repo_name,
                    head_sha,
                    temp_dir.display()
                );
                Some(Self { path: temp_dir })
            }
            Err(e) => {
                warn!(
                    "Failed to extract ephemeral tree for {}@{}: {}",
                    repo_name, head_sha, e
                );
                let _ = std::fs::remove_dir_all(&temp_dir);
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_repo_root_traversal_rejected() {
        assert!(resolve_repo_root("../evil").is_none());
        assert!(resolve_repo_root("foo/../bar").is_none());
        assert!(resolve_repo_root("foo/../../etc/passwd").is_none());
        assert!(resolve_repo_root("..").is_none());
        assert!(resolve_repo_root(".git").is_none());
        assert!(resolve_repo_root("foo/.git").is_none());
        assert!(resolve_repo_root("foo/.hidden").is_none());
        assert!(resolve_repo_root("evil;rm -rf /").is_none());
        assert!(resolve_repo_root("evil$foo").is_none());

        // Exact .github repository is permitted by validation
        // (will return None if not present in CWD/workspace root, but passes path validation)
        let _ = resolve_repo_root("owner/.github");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_ephemeral_tree_materializes_files_and_unlinks_escaping_symlinks() {
        use std::os::unix::fs::symlink;
        use std::process::Command as StdCommand;

        let base = std::env::temp_dir().join(format!(
            "revue-tree-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let repo_dir = base.join("demo-repo");
        std::fs::create_dir_all(repo_dir.join("src")).unwrap();

        let git = |args: &[&str]| {
            let st = StdCommand::new("git")
                .args(args)
                .current_dir(&repo_dir)
                .env("GIT_AUTHOR_NAME", "Test")
                .env("GIT_AUTHOR_EMAIL", "test@example.com")
                .env("GIT_COMMITTER_NAME", "Test")
                .env("GIT_COMMITTER_EMAIL", "test@example.com")
                .status()
                .unwrap();
            assert!(st.success(), "git {args:?} failed");
        };

        git(&["init", "-q"]);
        git(&["config", "core.symlinks", "true"]);
        std::fs::write(
            repo_dir.join("src/lib.rs"),
            "pub fn line_one() {}\npub fn line_two() {}\npub fn line_three() {}\n",
        )
        .unwrap();
        std::fs::write(repo_dir.join("src/helper.rs"), "pub fn helper() {}\n").unwrap();
        symlink("helper.rs", repo_dir.join("src/in_tree_link.rs")).unwrap();
        symlink("/etc/hosts", repo_dir.join("src/escape_link.rs")).unwrap();
        git(&["add", "."]);
        git(&["commit", "-q", "-m", "init"]);

        // Verify sanitize_extracted_tree directly even when host git sets core.symlinks=false.
        let sym_test_dir = base.join("sym-check");
        std::fs::create_dir_all(&sym_test_dir).unwrap();
        std::fs::write(sym_test_dir.join("local.txt"), "ok").unwrap();
        symlink("local.txt", sym_test_dir.join("safe_link")).unwrap();
        symlink("/etc/hosts", sym_test_dir.join("escape_link")).unwrap();
        sanitize_extracted_tree(&sym_test_dir);
        assert!(sym_test_dir.join("safe_link").exists());
        assert!(sym_test_dir.join("escape_link").symlink_metadata().is_err());

        let head_out = StdCommand::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(&repo_dir)
            .output()
            .unwrap();
        let sha = String::from_utf8(head_out.stdout)
            .unwrap()
            .trim()
            .to_string();

        let extracted_path = {
            let guard =
                EphemeralTreeGuard::prepare_from_root(&repo_dir, "owner/demo-repo", &sha, None)
                    .await
                    .expect("tree extraction should succeed");
            let root = guard.as_ref().to_path_buf();
            assert!(root.join("src/lib.rs").is_file());
            // Regardless of host core.symlinks, escape_link.rs must never resolve to /etc/hosts.
            if let Ok(canon) = root.join("src/escape_link.rs").canonicalize() {
                assert!(canon.starts_with(root.canonicalize().unwrap()));
            }

            let mut diff = crate::diff::ParsedDiff::parse(
                "--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1,2 +1,2 @@\n-pub fn line_one() {}\n+pub fn line_two() {}\n",
            )
            .unwrap();
            diff.enrich_with_disk_context(Some(&root));
            assert!(
                diff.files[0]
                    .file_context
                    .as_deref()
                    .unwrap_or("")
                    .contains("line_three")
            );
            root
        };

        assert!(
            !extracted_path.exists(),
            "ephemeral checkout must be deleted when guard drops"
        );
        let _ = std::fs::remove_dir_all(base);
    }
}
