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

/// An empty directory, removed on drop, so a CLI finds no project files or settings.
pub struct EmptyDir(PathBuf);

impl EmptyDir {
    pub fn new(prefix: &str) -> Result<Self> {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let path =
            std::env::temp_dir().join(format!("{}-{}-{}", prefix, std::process::id(), nanos));
        std::fs::create_dir(&path)
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
    fn empty_dir_is_removed_on_drop() {
        let dir = EmptyDir::new("revue-test").unwrap();
        let path = dir.path().to_path_buf();
        assert!(path.is_dir());
        drop(dir);
        assert!(!path.exists());
    }
}
