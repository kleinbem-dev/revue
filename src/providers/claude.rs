use super::Provider;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use tokio::process::Command;

pub struct ClaudeCliProvider {
    bin_path: String,
}

impl ClaudeCliProvider {
    pub fn new() -> Self {
        let bin_path = std::env::var("CLAUDE_BIN").unwrap_or_else(|_| "claude".to_string());
        Self { bin_path }
    }
}

#[async_trait]
impl Provider for ClaudeCliProvider {
    fn name(&self) -> &str {
        "Claude Code (Subscription)"
    }

    async fn complete(&self, prompt: &str) -> Result<String> {
        let output = Command::new(&self.bin_path)
            .arg("-p")
            .arg(prompt)
            .output()
            .await
            .map_err(|e| anyhow!("Failed to spawn claude CLI at '{}': {}", self.bin_path, e))?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        if !output.status.success() {
            let combined = format!("{}\n{}", stderr, stdout);
            if combined.contains("usage limit")
                || combined.contains("rate limit")
                || combined.contains("overloaded")
            {
                return Err(anyhow!("Claude usage limit detected: {}", combined.trim()));
            }
            return Err(anyhow!(
                "Claude CLI exited with code {:?}. Stderr: {}",
                output.status.code(),
                stderr.trim()
            ));
        }

        Ok(stdout)
    }
}
