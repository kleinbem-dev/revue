use super::{DiscussionComment, Forge, InlineReviewComment, PrMetadata};
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue, USER_AGENT};
use serde::Serialize;
use tracing::info;

pub struct GitHubClient {
    client: reqwest::Client,
}

#[derive(Serialize)]
struct CreateReviewRequest<'a> {
    body: &'a str,
    event: &'a str,
    comments: Vec<GitHubReviewComment<'a>>,
}

#[derive(Serialize)]
struct GitHubReviewComment<'a> {
    path: &'a str,
    line: u64,
    body: &'a str,
}

impl GitHubClient {
    pub fn new(token: Option<String>) -> Result<Self> {
        let mut headers = HeaderMap::new();
        headers.insert(USER_AGENT, HeaderValue::from_static("revue/0.1.0"));

        let resolved_token = token.or_else(|| std::env::var("GITHUB_TOKEN").ok());

        if let Some(t) = resolved_token {
            headers.insert(
                AUTHORIZATION,
                HeaderValue::from_str(&format!("Bearer {}", t))
                    .map_err(|e| anyhow!("Invalid GitHub token: {}", e))?,
            );
        }

        let client = reqwest::Client::builder()
            .default_headers(headers)
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| anyhow!("Failed to build HTTP client: {}", e))?;

        Ok(Self { client })
    }
}

#[async_trait]
impl Forge for GitHubClient {
    async fn fetch_pr_diff(&self, repo: &str, pr_index: u64) -> Result<String> {
        let url = format!("https://api.github.com/repos/{}/pulls/{}", repo, pr_index);
        let res = self
            .client
            .get(&url)
            .header("Accept", "application/vnd.github.v3.diff")
            .send()
            .await
            .map_err(|e| anyhow!("Failed to fetch PR diff from GitHub: {}", e))?;

        if !res.status().is_success() {
            return Err(anyhow!(
                "Failed to get diff from GitHub (status {}): {}",
                res.status(),
                res.text().await.unwrap_or_default()
            ));
        }

        res.text()
            .await
            .map_err(|e| anyhow!("Failed to read diff: {}", e))
    }

    async fn fetch_pr_metadata(&self, repo: &str, pr_index: u64) -> Result<PrMetadata> {
        let url = format!("https://api.github.com/repos/{}/pulls/{}", repo, pr_index);
        let res = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| anyhow!("Failed to fetch PR metadata from GitHub: {}", e))?;

        if !res.status().is_success() {
            return Err(anyhow!(
                "Failed to get PR metadata from GitHub (status {}): {}",
                res.status(),
                res.text().await.unwrap_or_default()
            ));
        }

        let val: serde_json::Value = res.json().await?;
        let title = val.get("title").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let description = val.get("body").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let author = val
            .get("user")
            .and_then(|u| u.get("login"))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();
        let head_sha = val
            .get("head")
            .and_then(|h| h.get("sha"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let head_ref = val
            .get("head")
            .and_then(|h| h.get("ref"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let base_ref = val
            .get("base")
            .and_then(|b| b.get("ref"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let is_draft = val.get("draft").and_then(|d| d.as_bool()).unwrap_or(false);
        let labels = val
            .get("labels")
            .and_then(|l| l.as_array())
            .map(|seq| {
                seq.iter()
                    .filter_map(|item| item.get("name").and_then(|n| n.as_str()))
                    .map(String::from)
                    .collect()
            })
            .unwrap_or_default();

        Ok(PrMetadata {
            title,
            description,
            author,
            head_sha,
            head_ref,
            base_ref,
            labels,
            repo: repo.to_string(),
            pr_number: pr_index,
            is_draft,
        })
    }

    async fn fetch_pr_comments(
        &self,
        repo: &str,
        pr_index: u64,
    ) -> Result<Vec<DiscussionComment>> {
        let mut comments = Vec::new();

        // 1. Fetch inline review comments
        let pulls_url = format!(
            "https://api.github.com/repos/{}/pulls/{}/comments?per_page=100",
            repo, pr_index
        );
        if let Ok(res) = self.client.get(&pulls_url).send().await
            && res.status().is_success()
            && let Ok(val) = res.json::<serde_json::Value>().await
            && let Some(arr) = val.as_array()
        {
            for item in arr {
                if let Some(body) = item.get("body").and_then(|b| b.as_str()) {
                    let author = item
                        .get("user")
                        .and_then(|u| u.get("login"))
                        .and_then(|l| l.as_str())
                        .unwrap_or("unknown")
                        .to_string();
                    let path = item.get("path").and_then(|p| p.as_str()).map(String::from);
                    let line = item
                        .get("line")
                        .or_else(|| item.get("original_line"))
                        .and_then(|l| l.as_u64());

                    comments.push(DiscussionComment {
                        author,
                        path,
                        line,
                        body: body.to_string(),
                    });
                }
            }
        }

        // 2. Fetch conversation comments
        let issues_url = format!(
            "https://api.github.com/repos/{}/issues/{}/comments?per_page=100",
            repo, pr_index
        );
        if let Ok(res) = self.client.get(&issues_url).send().await
            && res.status().is_success()
            && let Ok(val) = res.json::<serde_json::Value>().await
            && let Some(arr) = val.as_array()
        {
            for item in arr {
                if let Some(body) = item.get("body").and_then(|b| b.as_str()) {
                    let author = item
                        .get("user")
                        .and_then(|u| u.get("login"))
                        .and_then(|l| l.as_str())
                        .unwrap_or("unknown")
                        .to_string();

                    comments.push(DiscussionComment {
                        author,
                        path: None,
                        line: None,
                        body: body.to_string(),
                    });
                }
            }
        }

        Ok(comments)
    }

    async fn post_review(
        &self,
        repo: &str,
        pr_index: u64,
        summary: &str,
        comments: &[InlineReviewComment],
    ) -> Result<()> {
        let url = format!(
            "https://api.github.com/repos/{}/pulls/{}/reviews",
            repo, pr_index
        );

        let gh_comments: Vec<_> = comments
            .iter()
            .map(|c| GitHubReviewComment {
                path: &c.path,
                line: c.line,
                body: &c.body,
            })
            .collect();

        let req = CreateReviewRequest {
            body: summary,
            event: "COMMENT",
            comments: gh_comments,
        };

        let res = self
            .client
            .post(&url)
            .json(&req)
            .send()
            .await
            .map_err(|e| anyhow!("Failed to post review to GitHub: {}", e))?;

        if !res.status().is_success() {
            let status = res.status();
            let err_text = res.text().await.unwrap_or_default();

            // If GitHub rejects with 422 (frequently due to comments outside diff range),
            // gracefully fallback to posting the review without inline comments, appending them to the body.
            if status.as_u16() == 422 && !comments.is_empty() {
                tracing::warn!(
                    "GitHub rejected inline comments with 422 on PR #{}: {}. Retrying with consolidated review body...",
                    pr_index,
                    err_text
                );

                let mut fallback_body = summary.to_string();
                fallback_body.push_str("\n\n### 📝 Inline Findings (Consolidated)\n\n");
                for c in comments {
                    fallback_body
                        .push_str(&format!("- **`{}:{}`**\n{}\n\n", c.path, c.line, c.body));
                }

                let fallback_req = CreateReviewRequest {
                    body: &fallback_body,
                    event: "COMMENT",
                    comments: vec![],
                };

                let fallback_res = self
                    .client
                    .post(&url)
                    .json(&fallback_req)
                    .send()
                    .await
                    .map_err(|e| anyhow!("Failed to post fallback review to GitHub: {}", e))?;

                if !fallback_res.status().is_success() {
                    return Err(anyhow!(
                        "GitHub review creation fallback also failed (status {}): {}",
                        fallback_res.status(),
                        fallback_res.text().await.unwrap_or_default()
                    ));
                }

                info!(
                    "Successfully submitted fallback review to GitHub PR #{}",
                    pr_index
                );
                return Ok(());
            }

            return Err(anyhow!(
                "GitHub review creation failed (status {}): {}",
                status,
                err_text
            ));
        }

        info!("Successfully submitted review to GitHub PR #{}", pr_index);
        Ok(())
    }
}
