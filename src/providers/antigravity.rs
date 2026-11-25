use super::{Provider, cli};
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use tokio::process::Command;

/// Provider that executes completions using local Google Antigravity CLI in a sandbox.
pub struct AntigravityCliProvider {
    bin_path: String,
}

impl AntigravityCliProvider {
    /// Constructs a new Antigravity CLI provider resolving binary from environment.
    pub fn new() -> Self {
        let bin_path = std::env::var("ANTIGRAVITY_BIN")
            .or_else(|_| std::env::var("ANTIGRAVITY_AGENTAPI_EXE"))
            .unwrap_or_else(|_| "antigravity-cli".to_string());
        Self { bin_path }
    }
}

impl Default for AntigravityCliProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for AntigravityCliProvider {
    fn name(&self) -> &str {
        "Google Gemini (Antigravity)"
    }

    async fn complete(&self, prompt: &str) -> Result<String> {
        // -p only takes the prompt as an argument; fail over cleanly instead of E2BIG.
        if prompt.len() > cli::MAX_ARG_BYTES {
            return Err(anyhow!(
                "Prompt is {} bytes, above the {}-byte argument limit for the antigravity CLI",
                prompt.len(),
                cli::MAX_ARG_BYTES
            ));
        }
        let effort = std::env::var("REVUE_GEMINI_EFFORT").unwrap_or_else(|_| "low".to_string());
        // Run inside an empty temp directory so untrusted repo rules (GEMINI.md, .agents/) are never loaded.
        let program = cli::resolve_program(&self.bin_path);
        let cwd = cli::EmptyDir::new("revue-antigravity")?;
        let mut cmd = Command::new(&program);
        cmd.current_dir(cwd.path())
            .arg("--sandbox")
            .arg("--disable-slash-commands")
            .arg("--effort")
            .arg(&effort)
            .arg("-p")
            .arg(prompt);
        let what = format!("antigravity CLI at '{}'", self.bin_path);
        let output = cli::run(cmd, None, cli::cli_timeout(), &what).await?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        if !output.status.success() {
            let combined = format!("{}\n{}", stderr, stdout);
            if combined.contains("quota exceeded") || combined.contains("rate limit") {
                return Err(anyhow!("Gemini quota limit detected: {}", combined.trim()));
            }
            return Err(anyhow!(
                "Antigravity CLI exited with code {:?}. Stderr: {}",
                output.status.code(),
                stderr.trim()
            ));
        }

        Ok(stdout)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn oversized_prompt_fails_over_without_spawning() {
        let provider = AntigravityCliProvider {
            bin_path: "/nonexistent/antigravity-cli".to_string(),
        };
        let prompt = "x".repeat(cli::MAX_ARG_BYTES + 1);
        let err = provider.complete(&prompt).await.unwrap_err().to_string();
        assert!(err.contains("argument limit"), "{err}");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn runs_in_isolated_empty_working_directory() {
        use std::os::unix::fs::PermissionsExt;

        let dir = cli::EmptyDir::new("revue-antigravity-test").unwrap();
        let fake_bin = dir.path().join("fake-antigravity.sh");
        std::fs::write(&fake_bin, "#!/bin/sh\npwd\nls -A | wc -l\n").unwrap();
        std::fs::set_permissions(&fake_bin, std::fs::Permissions::from_mode(0o755)).unwrap();

        let provider = AntigravityCliProvider {
            bin_path: fake_bin.to_string_lossy().to_string(),
        };
        let out = provider.complete("review this").await.unwrap();
        let mut lines = out.lines();
        let ran_in = lines.next().unwrap_or("");
        let file_count = lines.next().unwrap_or("").trim();
        assert!(
            ran_in.contains("revue-antigravity-"),
            "unexpected cwd: {ran_in}"
        );
        assert_eq!(file_count, "0", "working directory must be empty");
    }
}
