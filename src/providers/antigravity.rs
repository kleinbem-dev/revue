use super::Provider;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use tokio::process::Command;

pub struct AntigravityCliProvider {
    bin_path: String,
}

impl AntigravityCliProvider {
    pub fn new() -> Self {
        let bin_path = std::env::var("ANTIGRAVITY_BIN")
            .or_else(|_| std::env::var("ANTIGRAVITY_AGENTAPI_EXE"))
            .unwrap_or_else(|_| "antigravity-cli".to_string());
        Self { bin_path }
    }
}

#[async_trait]
impl Provider for AntigravityCliProvider {
    fn name(&self) -> &str {
        "Google Gemini (Antigravity)"
    }

    async fn complete(&self, prompt: &str) -> Result<String> {
        let effort = std::env::var("REVUE_GEMINI_EFFORT").unwrap_or_else(|_| "low".to_string());
        let output = Command::new(&self.bin_path)
            .arg("--dangerously-skip-permissions")
            .arg("--disable-slash-commands")
            .arg("--effort")
            .arg(&effort)
            .arg("-p")
            .arg(prompt)
            .output()
            .await
            .map_err(|e| {
                anyhow!(
                    "Failed to spawn antigravity CLI at '{}': {}",
                    self.bin_path,
                    e
                )
            })?;

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
