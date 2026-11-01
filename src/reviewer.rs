use crate::config::AppConfig;
use crate::diff::ParsedDiff;
use crate::forges::{forgejo::ForgejoClient, github::GitHubClient, Forge, InlineReviewComment};
use crate::providers::{
    claude::ClaudeCliProvider, gemini::GeminiProvider, ollama::OllamaProvider, Provider,
    ProviderChain,
};
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::process::Command;
use tracing::info;

#[derive(Debug, Serialize, Deserialize)]
pub struct ReviewOutput {
    pub summary: String,
    #[serde(default)]
    pub issues: Vec<ReviewIssue>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ReviewIssue {
    pub file: String,
    pub line: u64,
    #[serde(default = "default_severity")]
    pub severity: String,
    pub title: String,
    pub comment: String,
    pub suggestion: Option<String>,
}

fn default_severity() -> String {
    "warning".to_string()
}

pub fn build_provider_chain(config: &AppConfig) -> ProviderChain {
    let mut providers: Vec<Box<dyn Provider>> = Vec::new();

    for name in &config.providers.order {
        match name.as_str() {
            "claude-cli" => {
                providers.push(Box::new(ClaudeCliProvider::new()));
            }
            "gemini" => {
                if let Ok(gemini) = GeminiProvider::new(config.providers.gemini_model.clone()) {
                    providers.push(Box::new(gemini));
                } else {
                    tracing::warn!("Gemini provider skipped (GEMINI_API_KEY not set)");
                }
            }
            "ollama" => {
                providers.push(Box::new(OllamaProvider::new(
                    config.providers.ollama_url.clone(),
                    config.providers.ollama_model.clone(),
                )));
            }
            _ => tracing::warn!("Unknown provider: {}", name),
        }
    }

    ProviderChain::new(providers)
}

pub fn generate_review_prompt(diff: &ParsedDiff, config: &AppConfig) -> String {
    let tone = config
        .tone_instructions
        .as_deref()
        .unwrap_or("Short and direct. Lead with the problem and the fix. No praise, no filler.");

    let mut path_instructions_text = String::new();
    for pi in &config.reviews.path_instructions {
        path_instructions_text.push_str(&format!("- For files matching `{}`: {}\n", pi.path, pi.instructions));
    }

    let mut files_diff_text = String::new();
    for f in &diff.files {
        files_diff_text.push_str(&format!("--- a/{}\n+++ b/{}\n", f.new_path, f.new_path));
        for h in &f.hunks {
            files_diff_text.push_str(&format!(
                "@@ -{},{} +{},{} @@\n",
                h.old_start, h.old_lines, h.new_start, h.new_lines
            ));
            for l in &h.lines {
                files_diff_text.push_str(l);
                files_diff_text.push('\n');
            }
        }
    }

    format!(
        r#"You are an expert, highly critical code reviewer.
Review the following git diff and output ONLY a valid JSON object matching the requested schema.

TONE INSTRUCTIONS:
{tone}

REPOSITORY-SPECIFIC RULES:
{path_instructions_text}

GENERAL GUIDELINES:
1. Focus strictly on real bugs, security vulnerabilities, race conditions, edge cases, logic flaws, and breaking changes.
2. DO NOT comment on formatting, whitespace, or minor stylistic nits that linters handle.
3. Every inline comment MUST refer to a real line present in the modified diff hunks.
4. If a suggested fix is clear, provide it in the `suggestion` field as raw replacement code.

SCHEMA:
{{
  "summary": "1-3 bullet points summarizing the PR changes and impact",
  "issues": [
    {{
      "file": "path/to/file.ext",
      "line": 42,
      "severity": "bug|security|performance|warning",
      "title": "Short title describing the defect",
      "comment": "Why this is an issue and how to resolve it",
      "suggestion": "optional replacement code"
    }}
  ]
}}

DIFF TO REVIEW:
{files_diff_text}
"#
    )
}

pub fn parse_review_response(raw: &str) -> Result<ReviewOutput> {
    // Strip markdown code block fences if the LLM wrapped JSON in ```json ... ```
    let trimmed = raw.trim();
    let json_str = if let Some(stripped) = trimmed.strip_prefix("```json") {
        stripped.strip_suffix("```").unwrap_or(stripped).trim()
    } else if let Some(stripped) = trimmed.strip_prefix("```") {
        stripped.strip_suffix("```").unwrap_or(stripped).trim()
    } else {
        trimmed
    };

    serde_json::from_str(json_str).with_context(|| format!("Failed to parse LLM JSON: {}", raw))
}

pub async fn run_diff_review(base: &str, format: &str, config_path: &str) -> Result<()> {
    let config = AppConfig::load_or_default(config_path)?;
    info!("Running local diff review against '{}'", base);

    let output = Command::new("git")
        .args(["diff", base])
        .output()
        .context("Failed to run `git diff`")?;

    let raw_diff = String::from_utf8_lossy(&output.stdout);
    if raw_diff.trim().is_empty() {
        println!("No changes detected between working copy and '{}'.", base);
        return Ok(());
    }

    let mut parsed = ParsedDiff::parse(&raw_diff)?;
    parsed.apply_filters(&config.reviews.path_filters);

    if parsed.files.is_empty() {
        println!("All changed files were ignored by path filters.");
        return Ok(());
    }

    let prompt = generate_review_prompt(&parsed, &config);
    let chain = build_provider_chain(&config);
    let (response, provider_name) = chain.complete_with_fallback(&prompt).await?;

    let review = parse_review_response(&response)?;

    if format == "json" {
        println!("{}", serde_json::to_string_pretty(&review)?);
    } else {
        println!("\n## Code Review Summary (via {})\n", provider_name);
        println!("{}\n", review.summary);

        if review.issues.is_empty() {
            println!("🎉 **No issues found! Everything looks clean.**");
        } else {
            println!("### Findings ({}):", review.issues.len());
            for issue in &review.issues {
                println!(
                    "- **[{}] `{}:{}`** - **{}**\n  {}\n",
                    issue.severity.to_uppercase(),
                    issue.file,
                    issue.line,
                    issue.title,
                    issue.comment
                );
                if let Some(sugg) = &issue.suggestion {
                    println!("  ```suggestion\n  {}\n  ```\n", sugg.trim());
                }
            }
        }
    }

    Ok(())
}

pub async fn run_pr_review(
    index: u64,
    repo_arg: Option<&str>,
    dry_run: bool,
    config_path: &str,
) -> Result<()> {
    let config = AppConfig::load_or_default(config_path)?;
    let repo = repo_arg.ok_or_else(|| anyhow!("--repo is required for PR review"))?;

    let forge: Box<dyn Forge> = if config.forgejo.token.is_some() {
        Box::new(ForgejoClient::new(
            config.forgejo.url.clone(),
            config.forgejo.token.clone(),
        )?)
    } else {
        Box::new(GitHubClient::new(None)?)
    };

    info!("Fetching PR #{} diff for {}", index, repo);
    let raw_diff = forge.fetch_pr_diff(repo, index).await?;
    let mut parsed = ParsedDiff::parse(&raw_diff)?;
    parsed.apply_filters(&config.reviews.path_filters);

    let prompt = generate_review_prompt(&parsed, &config);
    let chain = build_provider_chain(&config);
    let (response, provider_name) = chain.complete_with_fallback(&prompt).await?;
    let review = parse_review_response(&response)?;

    let mut inline_comments = Vec::new();
    for issue in &review.issues {
        if parsed.is_valid_line(&issue.file, issue.line) {
            let body = if let Some(sugg) = &issue.suggestion {
                format!(
                    "**{}**: {}\n\n```suggestion\n{}\n```",
                    issue.title,
                    issue.comment,
                    sugg.trim()
                )
            } else {
                format!("**{}**: {}", issue.title, issue.comment)
            };

            inline_comments.push(InlineReviewComment {
                path: issue.file.clone(),
                line: issue.line,
                body,
            });
        }
    }

    let summary = format!(
        "## nix-revue Analysis (Provider: {})\n\n{}\n\n*Reviewed {} files, flagged {} items.*",
        provider_name,
        review.summary,
        parsed.files.len(),
        inline_comments.len()
    );

    if dry_run {
        println!("{}", summary);
        for c in &inline_comments {
            println!("- {}:{} -> {}", c.path, c.line, c.body);
        }
    } else {
        forge
            .post_review(repo, index, &summary, &inline_comments)
            .await?;
    }

    Ok(())
}
