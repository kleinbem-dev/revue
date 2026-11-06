use std::path::{Path, PathBuf};
use std::time::Duration;
use tracing::{debug, info, warn};

/// Resolves the filesystem root of the given repository against current working directory or fleet roots.
pub fn resolve_repo_root(repo_arg: &str) -> Option<PathBuf> {
    let repo_basename = repo_arg.rsplit('/').next().unwrap_or(repo_arg);
    if repo_basename.is_empty() || repo_basename == "." {
        return None;
    }

    // 1. Current working directory
    let cwd = PathBuf::from(".");
    if cwd.join(".git").exists() || cwd.join(".jj").exists() {
        if let Ok(canon) = cwd.canonicalize() {
            if canon.file_name().and_then(|n| n.to_str()) == Some(repo_basename) {
                return Some(canon);
            }
        }
    }

    // 2. Explicit workspace root from env
    if let Ok(root) = std::env::var("KLEINBEM_WORKSPACE_ROOT") {
        let p = PathBuf::from(root).join(repo_basename);
        if p.is_dir() {
            return Some(p);
        }
    }

    // 3. Default fleet checkout path: ~/Develop/github.com/kleinbem/<repo_basename>
    if let Ok(home) = std::env::var("HOME") {
        let p = PathBuf::from(home)
            .join("Develop/github.com/kleinbem")
            .join(repo_basename);
        if p.is_dir() {
            return Some(p);
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
    pub async fn prepare(
        repo_name: &str,
        head_sha: &str,
        pr_number: Option<u64>,
    ) -> Option<Self> {
        // Reject invalid or non-hexadecimal git SHAs
        if head_sha.len() < 7 || head_sha.len() > 64 || !head_sha.bytes().all(|b| b.is_ascii_hexdigit()) {
            warn!("Invalid git head SHA '{}'; skipping ephemeral tree extraction", head_sha);
            return None;
        }

        let repo_root = resolve_repo_root(repo_name)?;

        // Ensure the commit exists in the local git repository; fetch if absent
        let has_commit = match tokio::time::timeout(
            Duration::from_secs(10),
            tokio::process::Command::new("git")
                .current_dir(&repo_root)
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
                .current_dir(&repo_root)
                .env("GIT_TERMINAL_PROMPT", "0")
                .env("GIT_ASKPASS", "/bin/false");

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
                .current_dir(&repo_root)
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
            "revue-index-{}-{}-{}-{}.tmp",
            repo_name.rsplit('/').next().unwrap_or("repo"),
            &head_sha[..head_sha.len().min(8)],
            std::process::id(),
            nanos
        ));

        let extract_pipeline = async {
            // 1. Read tree at head_sha into temporary index (bypassing export-ignore)
            let read_status = tokio::process::Command::new("git")
                .current_dir(&repo_root)
                .env("GIT_INDEX_FILE", &temp_index)
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

            // 2. Checkout all files into temp_dir without smudge or conversion alterations
            let prefix_arg = format!("{}/", temp_dir.display());
            let checkout_status = tokio::process::Command::new("git")
                .current_dir(&repo_root)
                .env("GIT_INDEX_FILE", &temp_index)
                .args([
                    "-c",
                    "core.autocrlf=false",
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

        let extract_result = match tokio::time::timeout(Duration::from_secs(30), extract_pipeline).await {
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
