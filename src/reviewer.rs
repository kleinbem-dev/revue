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
use std::time::Duration;
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
            out.push_str(&format!(
                "\n[SURROUNDING FILE CONTEXT FOR {} (numbered lines)]:\n",
                f.new_path
            ));
            out.push_str(ctx);
            out.push('\n');
        }
        out.push('\n');
    }

    // DLP Guard: Scrub private keys, cloud tokens, and sensitive credentials
    crate::scrubber::scrub_diff(&out).0
}

fn get_skills_block(diff: &ParsedDiff, config: &RevueConfig) -> String {
    let changed_files: Vec<String> = diff.files.iter().map(|f| f.new_path.clone()).collect();
    let skills_dir = crate::skills::find_skills_dir(config.review.skills_dir.as_deref());
    let matching_skills =
        crate::skills::find_matching_skills(skills_dir.as_deref(), &changed_files);
    crate::skills::format_skills_for_prompt(&matching_skills)
}

#[derive(Debug, Deserialize)]
pub struct SemgrepOutput {
    #[serde(default)]
    pub results: Vec<SemgrepResult>,
    #[serde(default)]
    pub errors: Vec<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub struct SemgrepResult {
    pub check_id: String,
    pub path: String,
    pub start: SemgrepPosition,
    #[serde(default)]
    pub end: Option<SemgrepPosition>,
    pub extra: SemgrepExtra,
}

#[derive(Debug, Deserialize)]
pub struct SemgrepPosition {
    pub line: u64,
    #[serde(default)]
    pub col: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub struct SemgrepExtra {
    pub message: String,
    #[serde(default)]
    pub severity: String,
    #[serde(default)]
    pub lines: Option<String>,
    #[serde(default)]
    pub fix: Option<String>,
}

pub fn parse_semgrep_results(raw_json: &str) -> Vec<ReviewIssue> {
    parse_semgrep_findings_with_spans(raw_json)
        .into_iter()
        .map(|(issue, _)| issue)
        .collect()
}

fn parse_semgrep_findings_with_spans(raw_json: &str) -> Vec<(ReviewIssue, Option<u64>)> {
    let output = match serde_json::from_str::<SemgrepOutput>(raw_json) {
        Ok(out) => out,
        Err(e) => {
            tracing::warn!("Failed to deserialize Semgrep JSON output: {}", e);
            return Vec::new();
        }
    };

    if !output.errors.is_empty() {
        tracing::warn!(
            "Semgrep reported {} internal error(s) during scan: {:?}",
            output.errors.len(),
            output.errors
        );
    }

    output
        .results
        .into_iter()
        .map(|res| {
            let severity = match res.extra.severity.to_uppercase().as_str() {
                "ERROR" => {
                    let id_lower = res.check_id.to_lowercase();
                    if id_lower.contains("sec")
                        || id_lower.contains("vuln")
                        || id_lower.contains("cve")
                    {
                        "security".to_string()
                    } else {
                        "bug".to_string()
                    }
                }
                "WARNING" => "warning".to_string(),
                _ => "warning".to_string(),
            };

            let clean_path = res.path.strip_prefix("./").unwrap_or(&res.path).to_string();
            let title = format!("AST Rule: {}", res.check_id);
            let end_line = res.end.map(|e| e.line);
            (
                ReviewIssue {
                    file: clean_path,
                    line: res.start.line,
                    severity,
                    title,
                    comment: crate::scrubber::scrub_diff(&res.extra.message).0,
                    suggestion: res.extra.fix.map(|fix| crate::scrubber::scrub_diff(&fix).0),
                },
                end_line,
            )
        })
        .collect()
}

pub async fn run_semgrep_pass0(
    diff: &ParsedDiff,
    config: &RevueConfig,
    repo_path: Option<&std::path::Path>,
) -> Vec<ReviewIssue> {
    if !config.review.semgrep || diff.files.is_empty() {
        return Vec::new();
    }

    let root = match repo_path {
        Some(r) => r,
        None => {
            tracing::debug!("No local checkout path provided for Semgrep Pass 0; skipping");
            return Vec::new();
        }
    };

    let valid_targets: Vec<String> = diff
        .files
        .iter()
        .map(|f| f.new_path.clone())
        .filter(|p| root.join(p).is_file())
        .collect();

    if valid_targets.is_empty() {
        tracing::debug!("No modified files found on disk for Semgrep Pass 0; skipping");
        return Vec::new();
    }

    let config_arg = if let Some(cfg) = &config.review.semgrep_config {
        Some(cfg.clone())
    } else if root.join(".semgrep").exists() {
        Some(root.join(".semgrep").to_string_lossy().to_string())
    } else if root.join(".semgrep.yml").exists() {
        Some(root.join(".semgrep.yml").to_string_lossy().to_string())
    } else if root.join(".semgrep.yaml").exists() {
        Some(root.join(".semgrep.yaml").to_string_lossy().to_string())
    } else {
        tracing::debug!(
            "No Semgrep rules configuration (.semgrep/ or .semgrep.yml) found; skipping Pass 0"
        );
        return Vec::new();
    };

    let semgrep_bin = std::env::var("SEMGREP_BIN").unwrap_or_else(|_| "semgrep".to_string());

    let mut cmd = tokio::process::Command::new(&semgrep_bin);
    cmd.arg("scan")
        .arg("--quiet")
        .arg("--json")
        .arg("--disable-version-check")
        .arg("--metrics=off");

    if let Some(cfg) = config_arg {
        cmd.arg("--config").arg(cfg);
    }

    cmd.arg("--");
    for target in &valid_targets {
        cmd.arg(target);
    }

    cmd.current_dir(root).kill_on_drop(true);

    let output = match tokio::time::timeout(Duration::from_secs(30), cmd.output()).await {
        Ok(Ok(out)) => out,
        Ok(Err(e)) => {
            if e.kind() == std::io::ErrorKind::NotFound {
                tracing::warn!(
                    "semgrep binary '{}' not found in PATH; skipping Pass 0 AST scan",
                    semgrep_bin
                );
            } else {
                tracing::warn!("Failed to execute semgrep for Pass 0: {}", e);
            }
            return Vec::new();
        }
        Err(_) => {
            tracing::warn!("Semgrep Pass 0 subprocess timed out after 30 seconds; skipping");
            return Vec::new();
        }
    };

    if !output.status.success() {
        let stderr_str = String::from_utf8_lossy(&output.stderr);
        tracing::warn!(
            "Semgrep Pass 0 exited with status {}; deterministic findings may be missing: {}",
            output.status,
            stderr_str.trim()
        );
    }

    let stdout_str = String::from_utf8_lossy(&output.stdout);
    let all_findings = parse_semgrep_findings_with_spans(&stdout_str);

    let relevant_findings: Vec<ReviewIssue> = all_findings
        .into_iter()
        .filter_map(|(mut issue, end_line)| {
            if diff.is_added_line(&issue.file, issue.line) {
                Some(issue)
            } else if let Some(end) = end_line {
                if let Some(added) =
                    diff.find_first_added_line_in_range(&issue.file, issue.line, end)
                {
                    issue.line = added;
                    Some(issue)
                } else {
                    None
                }
            } else {
                None
            }
        })
        .collect();

    if !relevant_findings.is_empty() {
        tracing::info!(
            "Semgrep Pass 0 discovered {} deterministic AST rule violation(s) in diff",
            relevant_findings.len()
        );
    }

    relevant_findings
}

pub fn merge_pass0_findings(review: &mut ReviewOutput, pass0_issues: Vec<ReviewIssue>) {
    if pass0_issues.is_empty() {
        return;
    }
    for issue in pass0_issues {
        let exists = review.issues.iter().any(|existing| {
            existing.file == issue.file
                && existing.line == issue.line
                && (existing.title == issue.title
                    || existing.comment.trim() == issue.comment.trim())
        });
        if !exists {
            review.issues.push(issue);
        }
    }
}

pub fn generate_pass1_detective_prompt(
    diff: &ParsedDiff,
    config: &RevueConfig,
    pass0_findings: &[ReviewIssue],
) -> String {
    let mut rules_text = String::new();
    for rule in &config.review.rules {
        rules_text.push_str(&format!(
            "- For files matching `{}`: {}\n",
            rule.path, rule.instructions
        ));
    }
    let skills_text = get_skills_block(diff, config);
    let code_content = format_diff_and_context(diff);

    let mut pass0_text = String::new();
    if !pass0_findings.is_empty() {
        pass0_text.push_str("\nDETERMINISTIC AST RULES (PASS 0 FINDINGS):\n");
        pass0_text.push_str("Static AST analysis (Semgrep) deterministically flagged the following rule violations in modified code:\n");
        for finding in pass0_findings {
            pass0_text.push_str(&format!(
                "- [{}] `{}:{}` - {}: {}\n",
                finding.severity.to_uppercase(),
                finding.file,
                finding.line,
                finding.title,
                finding.comment
            ));
            if let Some(sugg) = &finding.suggestion {
                pass0_text.push_str(&format!("  Suggested Fix: {}\n", sugg.trim()));
            }
        }
        pass0_text.push_str("Incorporate these verified findings into your analysis and investigate if any cascading or related defects exist.\n");
    }

    format!(
        r#"IMPORTANT: Answer directly in text. Do NOT use any tools, shell commands, or file operations. Base your response purely on the provided diff and surrounding file context.

You are an expert, highly investigative code detective.
Your task is PASS 1: Broad Defect Discovery.

Carefully examine the git diff and the surrounding file context below.
Discover all potential defects, edge cases, subtle logic bugs, security holes, concurrency issues, and broken assumptions.

REPOSITORY RULES:
{rules_text}
{skills_text}
{pass0_text}
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
    false_positives: &[crate::db::FindingFeedback],
) -> String {
    let tone = &config.review.tone;
    let skills_text = get_skills_block(diff, config);
    let code_content = format_diff_and_context(diff);

    let mut fp_text = String::new();
    if !false_positives.is_empty() {
        fp_text.push_str("\nPREVIOUSLY DISMISSED FALSE POSITIVES (DO NOT RE-FLAG THESE):\n");
        fp_text.push_str("The engineering team previously flagged and dismissed the following candidate issues as false alarms:\n");
        for item in false_positives {
            fp_text.push_str(&format!(
                "- In `{}`: \"{}\"",
                item.file_path, item.issue_title
            ));
            if let Some(reason) = &item.reason {
                fp_text.push_str(&format!(" (Reason: {})", reason));
            }
            fp_text.push('\n');
        }
        fp_text.push_str("If any candidate issue from Pass 1 matches or resembles these patterns, DISCARD it immediately as an intentional pattern.\n");
    }

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
{skills_text}
{fp_text}
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

pub fn generate_single_pass_prompt(
    diff: &ParsedDiff,
    config: &RevueConfig,
    pass0_findings: &[ReviewIssue],
) -> String {
    let tone = &config.review.tone;
    let mut rules_text = String::new();
    for rule in &config.review.rules {
        rules_text.push_str(&format!(
            "- For files matching `{}`: {}\n",
            rule.path, rule.instructions
        ));
    }
    let skills_text = get_skills_block(diff, config);
    let code_content = format_diff_and_context(diff);

    let mut pass0_text = String::new();
    if !pass0_findings.is_empty() {
        pass0_text.push_str("\nDETERMINISTIC AST RULES (PASS 0 FINDINGS):\n");
        pass0_text.push_str("Static AST analysis (Semgrep) deterministically flagged the following rule violations in modified code:\n");
        for finding in pass0_findings {
            pass0_text.push_str(&format!(
                "- [{}] `{}:{}` - {}: {}\n",
                finding.severity.to_uppercase(),
                finding.file,
                finding.line,
                finding.title,
                finding.comment
            ));
            if let Some(sugg) = &finding.suggestion {
                pass0_text.push_str(&format!("  Suggested Fix: {}\n", sugg.trim()));
            }
        }
    }

    format!(
        r#"IMPORTANT: Answer directly in text. Do NOT use any tools, shell commands, or file operations. Base your response purely on the provided diff and surrounding file context.

You are an expert, highly critical code reviewer.
Review the following git diff and output ONLY a valid JSON object matching the requested schema.

TONE INSTRUCTIONS:
{tone}

REPOSITORY-SPECIFIC RULES:
{rules_text}
{skills_text}
{pass0_text}
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
    false_positives: &[crate::db::FindingFeedback],
    repo_path: Option<&std::path::Path>,
) -> Result<(ReviewOutput, String)> {
    let pass0_issues = if config.review.semgrep {
        info!("Executing Review Pass 0 (Semgrep AST scan)...");
        run_semgrep_pass0(diff, config, repo_path).await
    } else {
        Vec::new()
    };
    if !pass0_issues.is_empty() {
        info!(
            "Pass 0 detected {} deterministic AST finding(s)",
            pass0_issues.len()
        );
    }

    if !config.review.two_pass {
        info!("Running single-pass review pipeline");
        let prompt = generate_single_pass_prompt(diff, config, &pass0_issues);
        let (resp, prov) = chain.complete_with_fallback(&prompt).await?;
        let mut parsed = parse_review_response(&resp)?;
        merge_pass0_findings(&mut parsed, pass0_issues);
        return Ok((parsed, prov));
    }

    info!("Executing Review Pass 1 (Detective - broad anomaly discovery)...");
    let pass1_prompt = generate_pass1_detective_prompt(diff, config, &pass0_issues);
    let (pass1_candidates, provider_name) = chain.complete_with_fallback(&pass1_prompt).await?;

    info!("Executing Review Pass 2 (Judge - verification & confidence filtering)...");
    let pass2_prompt =
        generate_pass2_judge_prompt(diff, config, &pass1_candidates, false_positives);
    let (pass2_response, _) = chain.complete_with_fallback(&pass2_prompt).await?;

    let mut review = parse_review_response(&pass2_response)?;
    merge_pass0_findings(&mut review, pass0_issues);
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

/// Execute two-pass review directly on a raw diff string. Used by CLI and MCP server.
pub async fn review_diff_content(
    raw_diff: &str,
    config_path: Option<&str>,
) -> Result<(ReviewOutput, String)> {
    let config = RevueConfig::find_and_load(config_path)?;
    let parsed = crate::diff::ParsedDiff::parse(raw_diff)?;
    let chain = build_provider_chain(&config);
    execute_review_pipeline(&parsed, &config, &chain, &[], None).await
}

pub async fn run_diff_review(base: &str, format: &str, config_path: Option<&str>) -> Result<()> {
    let config = RevueConfig::find_and_load(config_path)?;
    info!("Running local diff review against '{}'", base);

    let raw_diff = fetch_local_diff(base)?;
    if raw_diff.trim().is_empty() {
        println!(
            "No changes detected between working copy and '{}' (checked git and jj).",
            base
        );
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
    let (review, provider_name) = execute_review_pipeline(
        &parsed,
        &config,
        &chain,
        &[],
        Some(std::path::Path::new(".")),
    )
    .await?;

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

fn is_local_checkout_matching_repo(repo: &str) -> bool {
    if let Ok(output) = Command::new("git")
        .args(["remote", "get-url", "origin"])
        .output()
    {
        if output.status.success() {
            let url = String::from_utf8_lossy(&output.stdout);
            let url = url.trim().trim_end_matches('/').trim_end_matches(".git");
            let repo_path = repo.trim().trim_matches('/').trim_end_matches(".git");
            let remote_path = match url.split_once("://") {
                Some((_, rest)) => rest.split_once('/').map_or("", |(_, path)| path),
                None => url.split_once(':').map_or("", |(_, path)| path),
            };
            return remote_path == repo_path;
        }
    }
    false
}

pub async fn run_pr_review(
    index: u64,
    repo_arg: Option<&str>,
    dry_run: bool,
    config_path: Option<&str>,
) -> Result<()> {
    let config = RevueConfig::find_and_load(config_path)?;
    let repo = repo_arg.ok_or_else(|| anyhow!("--repo is required for PR review"))?;

    let is_forgejo = if config.forge.r#type == "forgejo" {
        true
    } else if config.forge.r#type == "github" {
        false
    } else {
        // "auto": determine from repo name or explicitly configured forgejo URL
        repo.contains("git.") || repo.contains("forgejo")
    };

    let forge: Box<dyn Forge> = if is_forgejo {
        Box::new(ForgejoClient::new(
            config.forge.url.clone(),
            config.forge.token.clone(),
        )?)
    } else {
        Box::new(GitHubClient::new(config.forge.token.clone())?)
    };

    info!("Fetching PR #{} diff for {}", index, repo);
    let raw_diff = forge.fetch_pr_diff(repo, index).await?;
    let mut parsed = ParsedDiff::parse(&raw_diff)?;
    parsed.apply_filters(&config.review.ignore);

    // Enrich with local filesystem context only if working inside a checkout of the matching repo
    let local_checkout = if is_local_checkout_matching_repo(repo) {
        Some(std::path::Path::new("."))
    } else {
        tracing::debug!(
            "Current directory does not match target PR repository '{}'; skipping local checkout scan",
            repo
        );
        None
    };

    if let Some(path) = local_checkout {
        parsed.enrich_with_disk_context(Some(path));
    }

    let chain = build_provider_chain(&config);
    let (review, provider_name) =
        execute_review_pipeline(&parsed, &config, &chain, &[], local_checkout).await?;

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

    let pipeline_desc = if config.review.semgrep {
        "Pass 0 (Semgrep AST) → Pass 1 (Detective) → Pass 2 (Judge)"
    } else {
        "Pass 1 (Detective) → Pass 2 (Judge)"
    };

    let summary = format!(
        "<!-- revue-ai-generated:v1 -->\n## 🔍 Revue Code Analysis (`{}`)\n\n{}\n\n---\n<details>\n<summary><b>📋 Verification & Review Details</b></summary>\n\n- **Inspected Files:** {}\n- **Actionable Issues Flagged:** {}\n- **Review Pipeline:** {}\n- **Engine:** Revue v0.1.0\n</details>",
        provider_name,
        review.summary,
        parsed.files.len(),
        inline_comments.len(),
        pipeline_desc
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

    #[test]
    fn test_parse_semgrep_json_results() {
        let sample = r#"{
            "errors": [],
            "results": [
                {
                    "check_id": "rules.security.no-hardcoded-keys",
                    "path": "./hosts/nixos-nvme/default.nix",
                    "start": { "line": 42, "col": 5 },
                    "extra": {
                        "message": "Do not hardcode private keys in repository files",
                        "severity": "ERROR",
                        "lines": "key = \"xyz\";",
                        "fix": "sops.secrets.key = { ... };"
                    }
                },
                {
                    "check_id": "rules.no-options-in-hosts",
                    "path": "hosts/nixos-nvme/default.nix",
                    "start": { "line": 15, "col": 1 },
                    "extra": {
                        "message": "Declare options in modules, not host definitions",
                        "severity": "WARNING",
                        "lines": "options = { ... };"
                    }
                }
            ]
        }"#;

        let issues = parse_semgrep_results(sample);
        assert_eq!(issues.len(), 2);

        // First finding: security severity, stripped ./ prefix, suggestion preserved
        assert_eq!(issues[0].file, "hosts/nixos-nvme/default.nix");
        assert_eq!(issues[0].line, 42);
        assert_eq!(issues[0].severity, "security");
        assert_eq!(
            issues[0].title,
            "AST Rule: rules.security.no-hardcoded-keys"
        );
        assert_eq!(
            issues[0].comment,
            "Do not hardcode private keys in repository files"
        );
        assert_eq!(
            issues[0].suggestion.as_deref(),
            Some("sops.secrets.key = { ... };")
        );

        // Second finding: warning severity
        assert_eq!(issues[1].file, "hosts/nixos-nvme/default.nix");
        assert_eq!(issues[1].line, 15);
        assert_eq!(issues[1].severity, "warning");
        assert_eq!(issues[1].suggestion, None);
    }

    #[test]
    fn test_pass1_detective_prompt_with_pass0_findings() {
        let diff_str = r#"--- a/src/main.rs
+++ b/src/main.rs
@@ -1,3 +1,4 @@
 fn main() {
+    let x = 1;
 }
"#;
        let diff = ParsedDiff::parse(diff_str).unwrap();
        let config = RevueConfig::default();
        let pass0_findings = vec![ReviewIssue {
            file: "src/main.rs".to_string(),
            line: 2,
            severity: "bug".to_string(),
            title: "AST Rule: unused-var".to_string(),
            comment: "Variable x is unused".to_string(),
            suggestion: Some("let _x = 1;".to_string()),
        }];

        let prompt = generate_pass1_detective_prompt(&diff, &config, &pass0_findings);
        assert!(prompt.contains("DETERMINISTIC AST RULES (PASS 0 FINDINGS):"));
        assert!(prompt.contains("AST Rule: unused-var"));
        assert!(prompt.contains("Variable x is unused"));
        assert!(prompt.contains("Suggested Fix: let _x = 1;"));
    }

    #[test]
    fn test_merge_pass0_findings() {
        let mut review = ReviewOutput {
            summary: "Initial".to_string(),
            issues: vec![ReviewIssue {
                file: "src/main.rs".to_string(),
                line: 2,
                severity: "bug".to_string(),
                title: "Semgrep Bug".to_string(),
                comment: "Exact duplicate finding".to_string(),
                suggestion: None,
            }],
        };

        let pass0_issues = vec![
            ReviewIssue {
                file: "src/main.rs".to_string(),
                line: 2, // duplicate finding (same title) -> should not be added
                severity: "bug".to_string(),
                title: "Semgrep Bug".to_string(),
                comment: "Exact duplicate finding".to_string(),
                suggestion: None,
            },
            ReviewIssue {
                file: "src/main.rs".to_string(),
                line: 2, // distinct finding on same line -> SHOULD be added
                severity: "security".to_string(),
                title: "Distinct Sec Bug".to_string(),
                comment: "Separate vulnerability on line 2".to_string(),
                suggestion: None,
            },
            ReviewIssue {
                file: "src/lib.rs".to_string(),
                line: 10, // new file/line -> should be added
                severity: "security".to_string(),
                title: "Semgrep Sec".to_string(),
                comment: "New issue".to_string(),
                suggestion: None,
            },
        ];

        merge_pass0_findings(&mut review, pass0_issues);
        assert_eq!(review.issues.len(), 3);
        assert_eq!(review.issues[0].title, "Semgrep Bug");
        assert_eq!(review.issues[1].title, "Distinct Sec Bug");
        assert_eq!(review.issues[2].title, "Semgrep Sec");
    }
}
