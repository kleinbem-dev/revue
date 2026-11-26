//! Running provider CLIs (claude, antigravity) as child processes.

use anyhow::{Result, anyhow};
use std::path::PathBuf;
use std::process::{Output, Stdio};
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

/// Linux rejects a single argv string above 128 KiB (MAX_ARG_STRLEN) with E2BIG.
pub const MAX_ARG_BYTES: usize = 128 * 1024 - 1;

const DEFAULT_TIMEOUT_SECS: u64 = 600;

/// How long one CLI call may run before it is killed (`REVUE_CLI_TIMEOUT_SECS`).
pub fn cli_timeout() -> Duration {
    let secs = std::env::var("REVUE_CLI_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|s| *s > 0)
        .unwrap_or(DEFAULT_TIMEOUT_SECS);
    Duration::from_secs(secs)
}

/// Makes a relative program path with a directory part (`./bin/claude`) absolute, so it still
/// resolves after `current_dir` changes. A bare name like `claude` keeps the PATH lookup.
pub fn resolve_program(bin: &str) -> PathBuf {
    let path = PathBuf::from(bin);
    if path.is_relative()
        && path.components().count() > 1
        && let Ok(cwd) = std::env::current_dir()
    {
        return cwd.join(path);
    }
    path
}

/// An empty directory, removed on drop, so a CLI finds no project files or settings.
pub struct EmptyDir(PathBuf);

impl EmptyDir {
    pub fn new(prefix: &str) -> Result<Self> {
        static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let path = std::env::temp_dir().join(format!(
            "{}-{}-{}-{}",
            prefix,
            std::process::id(),
            nanos,
            seq
        ));
        let mut builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder
            .create(&path)
            .map_err(|e| anyhow!("Failed to create {}: {}", path.display(), e))?;
        Ok(Self(path))
    }

    pub fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for EmptyDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Runs `cmd`, writes `stdin` to it if given, and kills it once `timeout` passes.
pub async fn run(
    mut cmd: Command,
    stdin: Option<&str>,
    timeout: Duration,
    what: &str,
) -> Result<Output> {
    cmd.kill_on_drop(true)
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|e| anyhow!("Failed to spawn {}: {}", what, e))?;

    let input = stdin.map(str::to_owned);
    let mut pipe = child.stdin.take();
    let feed = async move {
        if let (Some(pipe), Some(input)) = (pipe.as_mut(), input) {
            // A CLI that exits early closes the pipe; its exit status reports that.
            let _ = pipe.write_all(input.as_bytes()).await;
        }
        drop(pipe);
    };

    let run = async {
        let (_, output) = tokio::join!(feed, child.wait_with_output());
        output
    };

    match tokio::time::timeout(timeout, run).await {
        Ok(output) => output.map_err(|e| anyhow!("{} failed: {}", what, e)),
        Err(_) => Err(anyhow!(
            "{} timed out after {}s and was killed",
            what,
            timeout.as_secs()
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn passes_stdin_through() {
        let out = run(
            Command::new("cat"),
            Some("hello"),
            Duration::from_secs(10),
            "cat",
        )
        .await
        .unwrap();
        assert_eq!(String::from_utf8_lossy(&out.stdout), "hello");
    }

    #[tokio::test]
    async fn kills_a_hung_cli() {
        let mut cmd = Command::new("sleep");
        cmd.arg("30");
        let started = std::time::Instant::now();
        let err = run(cmd, None, Duration::from_millis(200), "sleep")
            .await
            .unwrap_err();
        assert!(err.to_string().contains("timed out"));
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn resolves_relative_program_paths_only() {
        assert_eq!(resolve_program("claude"), PathBuf::from("claude"));
        // temp_dir() is absolute on every platform, unlike a literal "/usr/bin".
        let abs = std::env::temp_dir().join("claude");
        assert_eq!(resolve_program(abs.to_str().unwrap()), abs);
        let rel = resolve_program("./bin/claude");
        assert!(rel.is_absolute());
        assert!(rel.ends_with("bin/claude"));
    }

    #[test]
    fn empty_dir_is_removed_on_drop() {
        let dir = EmptyDir::new("revue-test").unwrap();
        let path = dir.path().to_path_buf();
        assert!(path.is_dir());
        drop(dir);
        assert!(!path.exists());
    }
}
