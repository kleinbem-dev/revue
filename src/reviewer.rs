use crate::config::RevueConfig;
use crate::diff::ParsedDiff;
use crate::forges::{forgejo::ForgejoClient, github::GitHubClient, Forge, InlineReviewComment};
use crate::providers::{
    antigravity::AntigravityCliProvider, claude::ClaudeCliProvider, gemini::GeminiProvider,
    mesh_llm::MeshLlmProvider, ollama::OllamaProvider, Provider, ProviderChain,
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

pub fn build_provider_chain(config: &RevueConfig) -> ProviderChain {
    let mut providers: Vec<Box<dyn Provider>> = Vec::new();

    for name in &config.providers.cascade {
        match name.as_str() {
            "antigravity" | "gemini-cli" | "antigravity-cli" => {
                providers.push(Box::new(AntigravityCliProvider::new()));
            }
            "mesh-llm" | "mesh" => {
                providers.push(Box::new(MeshLlmProvider::new(
                    config.providers.mesh_llm_url.clone(),
                    config.providers.mesh_llm_model.clone(),
                )));
            }
            "claude" | "claude-cli" => {
                providers.push(Box::new(ClaudeCliProvider::new()));
            }
            "gemini" | "gemini-api" => {
                if let Ok(gemini) = GeminiProvider::new(config.providers.gemini_model.clone()) {
                    providers.push(Box::new(gemini));
                } else {
                    tracing::warn!("Gemini API skipped (no GEMINI_API_KEY / GOOGLE_API_KEY found)");
                }
            }
            "ollama" => {
                providers.push(Box::new(OllamaProvider::new(
                    config.providers.ollama_url.clone(),
                    config.providers.ollama_model.clone(),
                )));
            }
            _ => tracing::warn!("Unknown provider in cascade: {}", name),
        }
    }

    ProviderChain::new(providers)
}

fn format_diff_and_context(diff: &ParsedDiff) -> String {
    let mut out = String::new();
    for f in &diff.files {
        out.push_str(&format!("=== MODIFIED FILE: {} ===\n", f.new_path));
        out.push_str(&format!("--- a/{}\n+++ b/{}\n", f.new_path, f.new_path));
        for h in &f.hunks {
            out.push_str(&format!(
                "@@ -{},{} +{},{} @@\n",
                h.old_start, h.old_lines, h.new_start, h.new_lines
            ));
            for l in &h.lines {
                out.push_str(l);
                out.push('\n');
            }
        }

        if let Some(ctx) = &f.file_context {
            out.push_str(&format!("\n[SURROUNDING FILE CONTEXT FOR {} (numbered lines)]:\n", f.new_path));
            out.push_str(ctx);
            out.push('\n');
        }
        out.push('\n');
    }
    out
}

pub fn generate_pass1_detective_prompt(diff: &ParsedDiff, config: &RevueConfig) -> String {
    let mut rules_text = String::new();
    for rule in &config.review.rules {
        rules_text.push_str(&format!("- For files matching `{}`: {}\n", rule.path, rule.instructions));
    }
    let code_content = format_diff_and_context(diff);

    format!(
r#"IMPORTANT: Answer directly in text. Do NOT use any tools, shell commands, or file operations. Base your response purely on the provided diff and surrounding file context.

You are an expert, highly investigative code detective.
Your task is PASS 1: Broad Defect Discovery.

Carefully examine the git diff and the surrounding file context below.
Discover all potential defects, edge cases, subtle logic bugs, security holes, concurrency issues, and broken assumptions.

REPOSITORY RULES:
{rules_text}

INSTRUCTIONS:
1. Check how modified lines interact with the surrounding code (variable initializations, error returns, calling conventions).
2. For every potential issue you spot, output a bullet point containing:
   - File and line number
   - The hypothesized bug / vulnerability / edge case
   - Why it might fail or what assumption breaks
3. DO NOT comment on code formatting, whitespace, or minor naming nits.
4. Output your analysis in clear, structured markdown bullet points.

CODE UNDER REVIEW:
{code_content}
"#
    )
}

pub fn generate_pass2_judge_prompt(
    diff: &ParsedDiff,
    config: &RevueConfig,
    candidates: &str,
) -> String {
    let tone = &config.review.tone;
    let code_content = format_diff_and_context(diff);

    format!(
r#"IMPORTANT: Answer directly in text. Do NOT use any tools, shell commands, or file operations. Base your response purely on the provided diff and surrounding file context.

You are a Senior Principal Engineer and Impartial Review Judge.
Your task is PASS 2: Verification and Precision Filtering.

In Pass 1, an auditor proposed the candidate concerns below.
Your job is to rigorously evaluate each candidate against the actual code diff and surrounding context.

CANDIDATE ISSUES FROM PASS 1:
{candidates}

VERIFICATION GUIDELINES:
1. Verify if each issue is a TRUE DEFECT or a false positive (check if surrounding code already handles it, or if it is intentional).
2. Verify the exact line number exists in the diff additions.
3. Score each finding's confidence from 1 to 10. Discard anything below 8.
4. Eliminate all stylistic nits and subjective opinions. Keep only verified, actionable defects.
5. Provide a raw code replacement in `suggestion` whenever possible.

TONE INSTRUCTIONS:
{tone}

OUTPUT FORMAT:
Output ONLY a single valid JSON object matching this schema:
{{
  "summary": "1-3 high-level bullet points summarizing the PR changes and impact",
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

CODE UNDER REVIEW:
{code_content}
"#
    )
}

pub fn generate_single_pass_prompt(diff: &ParsedDiff, config: &RevueConfig) -> String {
    let tone = &config.review.tone;
    let mut rules_text = String::new();
    for rule in &config.review.rules {
        rules_text.push_str(&format!("- For files matching `{}`: {}\n", rule.path, rule.instructions));
    }
    let code_content = format_diff_and_context(diff);

    format!(
r#"IMPORTANT: Answer directly in text. Do NOT use any tools, shell commands, or file operations. Base your response purely on the provided diff and surrounding file context.

You are an expert, highly critical code reviewer.
Review the following git diff and output ONLY a valid JSON object matching the requested schema.

TONE INSTRUCTIONS:
{tone}

REPOSITORY-SPECIFIC RULES:
{rules_text}

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

DIFF AND CONTEXT:
{code_content}
"#
    )
}

pub async fn execute_review_pipeline(
    diff: &ParsedDiff,
    config: &RevueConfig,
    chain: &ProviderChain,
) -> Result<(ReviewOutput, String)> {
    if !config.review.two_pass {
        info!("Running single-pass review pipeline");
        let prompt = generate_single_pass_prompt(diff, config);
        let (resp, prov) = chain.complete_with_fallback(&prompt).await?;
        let parsed = parse_review_response(&resp)?;
        return Ok((parsed, prov));
    }

    info!("Executing Review Pass 1 (Detective - broad anomaly discovery)...");
    let pass1_prompt = generate_pass1_detective_prompt(diff, config);
    let (pass1_candidates, provider_name) = chain.complete_with_fallback(&pass1_prompt).await?;

    info!("Executing Review Pass 2 (Judge - verification & confidence filtering)...");
    let pass2_prompt = generate_pass2_judge_prompt(diff, config, &pass1_candidates);
    let (pass2_response, _) = chain.complete_with_fallback(&pass2_prompt).await?;

    let review = parse_review_response(&pass2_response)?;
    Ok((review, provider_name))
}

pub fn parse_review_response(raw: &str) -> Result<ReviewOutput> {
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

fn fetch_local_diff(base: &str) -> Result<String> {
    // 1. Try `git diff <base>`
    if let Ok(output) = Command::new("git").args(["diff", base]).output() {
        if output.status.success() {
            let s = String::from_utf8_lossy(&output.stdout).to_string();
            if !s.trim().is_empty() {
                return Ok(s);
            }
        }
    }

    // 2. Try `jj diff --git -r <base>..@` (for Jujutsu colocated/added workspaces)
    let jj_range = format!("{}..@", base);
    if let Ok(output) = Command::new("jj")
        .args(["diff", "--git", "-r", &jj_range])
        .output()
    {
        if output.status.success() {
            let s = String::from_utf8_lossy(&output.stdout).to_string();
            if !s.trim().is_empty() {
                return Ok(s);
            }
        }
    }

    Ok(String::new())
}

pub async fn run_diff_review(base: &str, format: &str, config_path: Option<&str>) -> Result<()> {
    let config = RevueConfig::find_and_load(config_path)?;
    info!("Running local diff review against '{}'", base);

    let raw_diff = fetch_local_diff(base)?;
    if raw_diff.trim().is_empty() {
        println!("No changes detected between working copy and '{}' (checked git and jj).", base);
        return Ok(());
    }

    let mut parsed = ParsedDiff::parse(&raw_diff)?;
    parsed.apply_filters(&config.review.ignore);

    if parsed.files.is_empty() {
        println!("All changed files were ignored by review.ignore rules.");
        return Ok(());
    }

    // Enrich with actual surrounding code from disk
    parsed.enrich_with_disk_context(None);

    let chain = build_provider_chain(&config);
    let (review, provider_name) = execute_review_pipeline(&parsed, &config, &chain).await?;

    if format == "json" {
        println!("{}", serde_json::to_string_pretty(&review)?);
    } else {
        println!("\n## nix-revue Analysis (Provider: {})\n", provider_name);
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
    config_path: Option<&str>,
) -> Result<()> {
    let config = RevueConfig::find_and_load(config_path)?;
    let repo = repo_arg.ok_or_else(|| anyhow!("--repo is required for PR review"))?;

    let forge: Box<dyn Forge> = if config.forge.r#type == "forgejo"
        || config.forge.url.contains("git.")
        || config.forge.token.is_some()
    {
        Box::new(ForgejoClient::new(
            config.forge.url.clone(),
            config.forge.token.clone(),
        )?)
    } else {
        Box::new(GitHubClient::new(None)?)
    };

    info!("Fetching PR #{} diff for {}", index, repo);
    let raw_diff = forge.fetch_pr_diff(repo, index).await?;
    let mut parsed = ParsedDiff::parse(&raw_diff)?;
    parsed.apply_filters(&config.review.ignore);

    // Enrich with local filesystem context if working inside a checkout of the repo
    parsed.enrich_with_disk_context(None);

    let chain = build_provider_chain(&config);
    let (review, provider_name) = execute_review_pipeline(&parsed, &config, &chain).await?;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_review_markdown_fences() {
        let raw_with_fences = r#"```json
{
  "summary": "Added a new greeting function",
  "issues": [
    {
      "file": "src/main.rs",
      "line": 42,
      "severity": "bug",
      "title": "Unwrap panic",
      "comment": "This may unwrap on None",
      "suggestion": "if let Some(x) = opt { ... }"
    }
  ]
}
```"#;

        let output = parse_review_response(raw_with_fences).expect("Should parse markdown fences");
        assert_eq!(output.summary, "Added a new greeting function");
        assert_eq!(output.issues.len(), 1);
        assert_eq!(output.issues[0].file, "src/main.rs");
        assert_eq!(output.issues[0].line, 42);
        assert_eq!(output.issues[0].severity, "bug");
        assert!(output.issues[0].suggestion.is_some());
    }

    #[test]
    fn test_parse_review_raw_json() {
        let raw = r#"{"summary": "No changes", "issues": []}"#;
        let output = parse_review_response(raw).expect("Should parse raw json");
        assert_eq!(output.summary, "No changes");
        assert!(output.issues.is_empty());
    }
}
