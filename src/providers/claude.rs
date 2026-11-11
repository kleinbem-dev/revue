use super::{Provider, cli};
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use tokio::process::Command;

/// The review prompt carries untrusted PR content, so the CLI gets no tools, no MCP
/// servers and no user settings (hooks): it can only answer in text.
const ARGS: &[&str] = &[
    "-p",
    "--tools",
    "",
    "--strict-mcp-config",
    "--setting-sources",
    "project",
    "--no-session-persistence",
];

pub struct ClaudeCliProvider {
    bin_path: String,
}

impl ClaudeCliProvider {
    pub fn new() -> Self {
        let bin_path = std::env::var("CLAUDE_BIN").unwrap_or_else(|_| "claude".to_string());
        Self { bin_path }
    }
}

impl Default for ClaudeCliProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for ClaudeCliProvider {
    fn name(&self) -> &str {
        "Claude Code (Subscription)"
    }

    async fn complete(&self, prompt: &str) -> Result<String> {
        // Empty cwd: no project settings or CLAUDE.md. Prompt on stdin: no E2BIG.
        let workdir = cli::EmptyDir::new("revue-claude")?;
        let mut cmd = Command::new(cli::resolve_program(&self.bin_path));
        cmd.args(ARGS).current_dir(workdir.path());
        let what = format!("claude CLI at '{}'", self.bin_path);
        let output = cli::run(cmd, Some(prompt), cli::cli_timeout(), &what).await?;

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

#[cfg(test)]
mod tests {
    use super::ARGS;

    #[test]
    fn claude_runs_without_tools_mcp_or_user_settings() {
        let at = |flag: &str| ARGS.iter().position(|a| *a == flag).expect(flag);
        assert_eq!(ARGS[at("--tools") + 1], "");
        assert_eq!(ARGS[at("--setting-sources") + 1], "project");
        assert!(ARGS.contains(&"--strict-mcp-config"));
        // The prompt goes on stdin, never into argv.
        assert_eq!(ARGS[0], "-p");
        assert_eq!(ARGS.iter().filter(|a| !a.starts_with('-')).count(), 2);
    }
}
