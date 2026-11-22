use std::path::{Path, PathBuf};
use std::time::Duration;
use tracing::{debug, info, warn};

const GIT_REPO_ENV_VARS: &[&str] = &[
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_COMMON_DIR",
    "GIT_PREFIX",
];

fn isolated_tokio_git(repo_root: &Path) -> tokio::process::Command {
    let mut cmd = tokio::process::Command::new("git");
    cmd.current_dir(repo_root);
    for var in GIT_REPO_ENV_VARS {
        cmd.env_remove(var);
    }
    cmd
}

/// Resolves the filesystem root of the given repository against current working directory or fleet roots.
pub fn resolve_repo_root(repo_arg: &str) -> Option<PathBuf> {
    // Reject empty segments (including leading/trailing '/'), traversal, git metadata, and hidden dirs (except .github)
    if repo_arg.split('/').any(|seg| {
        seg.is_empty()
            || seg == ".."
            || seg == "."
            || seg == ".git"
            || (seg.starts_with('.') && seg != ".github")
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

fn rewrite_github_ssh_to_https(remote_url: &str) -> Option<String> {
    let trimmed = remote_url.trim();
    let rest = trimmed
        .strip_prefix("git@github.com:")
        .or_else(|| trimmed.strip_prefix("ssh://git@github.com/"))
        .or_else(|| trimmed.strip_prefix("https://github.com/"))?;
    let slug = rest.trim_matches('/').trim_end_matches(".git");
    if slug.is_empty() || !slug.contains('/') {
        return None;
    }
    Some(format!("https://github.com/{}.git", slug))
}

fn isolated_bare_git(bare_dir: &Path) -> tokio::process::Command {
    let mut cmd = isolated_tokio_git(bare_dir);
    cmd.arg("--git-dir").arg(bare_dir);
    cmd
}

fn is_scp_like_remote(raw: &str) -> bool {
    let Some(colon_idx) = raw.find(':') else {
        return false;
    };
    !raw[..colon_idx].contains('/')
}

async fn resolve_fetch_remote_url(
    repo_root: Option<&Path>,
    fallback_clone_url: Option<&str>,
) -> Option<String> {
    if let Some(root) = repo_root
        && let Ok(Ok(out)) = tokio::time::timeout(
            Duration::from_secs(5),
            isolated_tokio_git(root)
                .args(["remote", "get-url", "origin"])
                .output(),
        )
        .await
        && out.status.success()
    {
        let raw = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if let Some(https_url) = rewrite_github_ssh_to_https(&raw) {
            return Some(https_url);
        }
        if !raw.is_empty() {
            if !raw.contains("://") && !raw.starts_with('/') && !is_scp_like_remote(&raw) {
                let joined = root.join(&raw);
                let resolved = joined.canonicalize().unwrap_or(joined);
                return Some(resolved.to_string_lossy().to_string());
            }
            return Some(raw);
        }
    }

    fallback_clone_url
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

async fn resolve_ssh_command(repo_root: Option<&Path>) -> Option<String> {
    let root = repo_root?;
    let out = tokio::time::timeout(
        Duration::from_secs(5),
        isolated_tokio_git(root)
            .args(["config", "--get", "core.sshCommand"])
            .output(),
    )
    .await
    .ok()?
    .ok()?;
    if !out.status.success() {
        return None;
    }
    let val = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if val.is_empty() { None } else { Some(val) }
}

async fn has_git_commit(git_dir: &Path, is_bare: bool, head_sha: &str) -> bool {
    let mut cmd = if is_bare {
        isolated_bare_git(git_dir)
    } else {
        isolated_tokio_git(git_dir)
    };
    match tokio::time::timeout(
        Duration::from_secs(10),
        cmd.args(["cat-file", "-e", head_sha]).output(),
    )
    .await
    {
        Ok(Ok(o)) => o.status.success(),
        _ => false,
    }
}

async fn fetch_refspec_into_bare(
    bare_dir: &Path,
    remote_url: &str,
    refspec: &str,
    git_token: Option<&str>,
    ssh_command: Option<&str>,
) -> bool {
    let mut fetch_cmd = isolated_bare_git(bare_dir);
    fetch_cmd
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_ASKPASS", "");

    if remote_url.starts_with("https://github.com/")
        && let Some(tok) = git_token.map(str::trim).filter(|t| !t.is_empty())
    {
        let basic =
            crate::forges::base64_encode(format!("x-access-token:{}", tok).as_bytes(), false);
        fetch_cmd
            .env(
                "REVUE_GIT_AUTH_HEADER",
                format!("AUTHORIZATION: basic {}", basic),
            )
            .arg("--config-env=http.https://github.com/.extraHeader=REVUE_GIT_AUTH_HEADER");
    } else if !remote_url.starts_with("http://")
        && !remote_url.starts_with("https://")
        && let Some(ssh_cmd) = ssh_command
    {
        fetch_cmd.env("GIT_SSH_COMMAND", ssh_cmd);
    }

    fetch_cmd.args(["fetch", "--depth=1", remote_url, refspec]);
    fetch_cmd.kill_on_drop(true);

    match tokio::time::timeout(Duration::from_secs(30), fetch_cmd.output()).await {
        Ok(Ok(o)) if o.status.success() => true,
        Ok(Ok(o)) => {
            let stderr = crate::scrubber::scrub_text(&String::from_utf8_lossy(&o.stderr));
            warn!(
                "git fetch failed with status {}: {}",
                o.status,
                stderr.trim()
            );
            false
        }
        Ok(Err(e)) => {
            warn!("git fetch could not run: {}", e);
            false
        }
        Err(_) => {
            warn!("git fetch timed out after 30s");
            false
        }
    }
}

struct TempDirCleanup(PathBuf);

impl Drop for TempDirCleanup {
    fn drop(&mut self) {
        if self.0.exists() {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

impl EphemeralTreeGuard {
    /// Extracts a clean tree for `head_sha` into a temporary directory without modifying working copies.
    pub async fn prepare(
        repo_name: &str,
        head_sha: &str,
        pr_number: Option<u64>,
        git_token: Option<&str>,
        fallback_clone_url: Option<&str>,
    ) -> Option<Self> {
        let repo_root = resolve_repo_root(repo_name);
        Self::prepare_inner(
            repo_root.as_deref(),
            repo_name,
            head_sha,
            pr_number,
            git_token,
            fallback_clone_url,
        )
        .await
    }

    #[cfg(test)]
    pub(crate) async fn prepare_from_root(
        repo_root: &Path,
        repo_name: &str,
        head_sha: &str,
        pr_number: Option<u64>,
        git_token: Option<&str>,
    ) -> Option<Self> {
        Self::prepare_inner(
            Some(repo_root),
            repo_name,
            head_sha,
            pr_number,
            git_token,
            None,
        )
        .await
    }

    async fn prepare_inner(
        repo_root: Option<&Path>,
        repo_name: &str,
        head_sha: &str,
        pr_number: Option<u64>,
        git_token: Option<&str>,
        fallback_clone_url: Option<&str>,
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

        static INVOCATION_COUNTER: std::sync::atomic::AtomicU64 =
            std::sync::atomic::AtomicU64::new(1);
        let count = INVOCATION_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);

        let temp_base = std::env::temp_dir();
        let slug = repo_name.rsplit('/').next().unwrap_or("repo");
        let short_sha = &head_sha[..head_sha.len().min(8)];

        // Prefer an existing read-only local checkout when it already has the commit;
        // otherwise shallow-fetch into a temporary bare repo under /tmp so read-only
        // host checkouts are never mutated.
        let mut _temp_bare_guard: Option<TempDirCleanup> = None;
        let (git_source_dir, is_bare_source): (PathBuf, bool) = match repo_root {
            Some(root) if has_git_commit(root, false, head_sha).await => {
                (root.to_path_buf(), false)
            }
            _ => {
                let Some(remote_url) =
                    resolve_fetch_remote_url(repo_root, fallback_clone_url).await
                else {
                    warn!(
                        "No local commit '{}' and no fetch remote for {}; skipping ephemeral tree",
                        head_sha, repo_name
                    );
                    return None;
                };
                let ssh_command = resolve_ssh_command(repo_root).await;

                let bare_dir = temp_base.join(format!(
                    "revue-bare-{}-{}-{}-{}-{}",
                    slug,
                    short_sha,
                    std::process::id(),
                    nanos,
                    count
                ));
                let mut bare_builder = std::fs::DirBuilder::new();
                #[cfg(unix)]
                {
                    use std::os::unix::fs::DirBuilderExt;
                    bare_builder.mode(0o700);
                }
                if let Err(e) = bare_builder.create(&bare_dir) {
                    warn!("Failed to create temporary bare git dir: {}", e);
                    return None;
                }
                _temp_bare_guard = Some(TempDirCleanup(bare_dir.clone()));

                let init_ok = matches!(
                    tokio::time::timeout(
                        Duration::from_secs(10),
                        isolated_tokio_git(&bare_dir)
                            .args(["init", "--bare", "-q"])
                            .output(),
                    )
                    .await,
                    Ok(Ok(o)) if o.status.success()
                );
                if !init_ok {
                    warn!("git init --bare failed for {}", repo_name);
                    return None;
                }

                if let Some(pr) = pr_number {
                    info!(
                        "Commit '{}' not found locally; shallow-fetching pull/{}/head for {}...",
                        head_sha, pr, repo_name
                    );
                    let refspec = format!("refs/pull/{}/head", pr);
                    fetch_refspec_into_bare(
                        &bare_dir,
                        &remote_url,
                        &refspec,
                        git_token,
                        ssh_command.as_deref(),
                    )
                    .await;
                    // Recover if the PR tip advanced past head_sha after metadata was fetched.
                    if !has_git_commit(&bare_dir, true, head_sha).await {
                        fetch_refspec_into_bare(
                            &bare_dir,
                            &remote_url,
                            head_sha,
                            git_token,
                            ssh_command.as_deref(),
                        )
                        .await;
                    }
                } else {
                    fetch_refspec_into_bare(
                        &bare_dir,
                        &remote_url,
                        head_sha,
                        git_token,
                        ssh_command.as_deref(),
                    )
                    .await;
                }

                if !has_git_commit(&bare_dir, true, head_sha).await {
                    warn!(
                        "Commit '{}' remains absent after shallow fetch for {}; skipping ephemeral tree",
                        head_sha, repo_name
                    );
                    return None;
                }

                (bare_dir, true)
            }
        };

        let temp_dir = temp_base.join(format!(
            "revue-tree-{}-{}-{}-{}-{}",
            slug,
            short_sha,
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
            slug,
            short_sha,
            std::process::id(),
            nanos,
            count
        ));

        let extract_pipeline = async {
            // Isolate git from global/system smudge filters and disable symlink creation on checkout.
            let mut read_cmd = if is_bare_source {
                isolated_bare_git(&git_source_dir)
            } else {
                isolated_tokio_git(&git_source_dir)
            };
            let read_status = read_cmd
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
            let mut checkout_cmd = if is_bare_source {
                let mut cmd = isolated_bare_git(&git_source_dir);
                cmd.arg("--work-tree").arg(&temp_dir);
                cmd
            } else {
                isolated_tokio_git(&git_source_dir)
            };
            let checkout_status = checkout_cmd
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
        assert!(resolve_repo_root("/tmp/other-repo").is_none());
        assert!(resolve_repo_root("owner//repo").is_none());
        assert!(resolve_repo_root("").is_none());

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

        let isolated_git = || {
            let mut cmd = StdCommand::new("git");
            cmd.current_dir(&repo_dir)
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_CONFIG_GLOBAL", "/dev/null");
            for var in GIT_REPO_ENV_VARS {
                cmd.env_remove(var);
            }
            cmd
        };

        let git = |args: &[&str]| {
            let st = isolated_git()
                .args(args)
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

        let head_out = isolated_git().args(["rev-parse", "HEAD"]).output().unwrap();
        let sha = String::from_utf8(head_out.stdout)
            .unwrap()
            .trim()
            .to_string();

        let extracted_path = {
            let guard = EphemeralTreeGuard::prepare_from_root(
                &repo_dir,
                "owner/demo-repo",
                &sha,
                None,
                None,
            )
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

    #[test]
    fn rewrites_github_ssh_and_https_remotes_to_https() {
        assert_eq!(
            rewrite_github_ssh_to_https("git@github.com:kleinbem/nix-toolbox.git").as_deref(),
            Some("https://github.com/kleinbem/nix-toolbox.git")
        );
        assert_eq!(
            rewrite_github_ssh_to_https("ssh://git@github.com/kleinbem-dev/revue").as_deref(),
            Some("https://github.com/kleinbem-dev/revue.git")
        );
        assert_eq!(
            rewrite_github_ssh_to_https("https://github.com/kleinbem/nix-config.git").as_deref(),
            Some("https://github.com/kleinbem/nix-config.git")
        );
        assert_eq!(
            rewrite_github_ssh_to_https("https://github.com/kleinbem/nix-config.git/").as_deref(),
            Some("https://github.com/kleinbem/nix-config.git")
        );
        assert_eq!(
            rewrite_github_ssh_to_https("ssh://git@git.kleinbem.dev/kleinbem/repo.git"),
            None
        );
    }

    #[tokio::test]
    async fn test_ephemeral_tree_shallow_fetches_from_bare_remote() {
        use std::process::Command as StdCommand;

        let base = std::env::temp_dir().join(format!(
            "revue-bare-fetch-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _cleanup = TempDirCleanup(base.clone());
        let remote_dir = base.join("remote-repo");
        let local_dir = base.join("local-checkout");
        std::fs::create_dir_all(remote_dir.join("src")).unwrap();
        std::fs::create_dir_all(&local_dir).unwrap();

        let run_git = |dir: &Path, args: &[&str]| {
            let mut cmd = StdCommand::new("git");
            cmd.current_dir(dir)
                .args(args)
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_AUTHOR_NAME", "Test")
                .env("GIT_AUTHOR_EMAIL", "test@example.com")
                .env("GIT_COMMITTER_NAME", "Test")
                .env("GIT_COMMITTER_EMAIL", "test@example.com");
            for var in GIT_REPO_ENV_VARS {
                cmd.env_remove(var);
            }
            let out = cmd.output().unwrap();
            assert!(
                out.status.success(),
                "git {args:?} failed: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        };

        run_git(&remote_dir, &["init", "-q"]);
        std::fs::write(
            remote_dir.join("src/app.rs"),
            "pub fn ok() -> bool { true }\n",
        )
        .unwrap();
        run_git(&remote_dir, &["add", "."]);
        run_git(&remote_dir, &["commit", "-q", "-m", "initial"]);
        let sha = run_git(&remote_dir, &["rev-parse", "HEAD"]);

        // Local checkout has a relative origin pointing to ../remote-repo, but lacks `sha`.
        run_git(&local_dir, &["init", "-q"]);
        run_git(&local_dir, &["remote", "add", "origin", "../remote-repo"]);

        let extracted_path = {
            let guard = EphemeralTreeGuard::prepare_inner(
                Some(&local_dir),
                "owner/demo-repo",
                &sha,
                None,
                None,
                None,
            )
            .await
            .expect("shallow fetch into bare repo + checkout-index should succeed");
            let root = guard.as_ref().to_path_buf();
            assert_eq!(
                std::fs::read_to_string(root.join("src/app.rs")).unwrap(),
                "pub fn ok() -> bool { true }\n"
            );
            root
        };

        assert!(!extracted_path.exists());
    }
}
