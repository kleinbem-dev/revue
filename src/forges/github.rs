use super::{
    DiscussionComment, Forge, InlineReviewComment, PrMetadata, build_http_client,
    format_consolidated_review_body, parse_comments_array, parse_pr_metadata_json,
};
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use serde::Serialize;
use tracing::{info, warn};

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
        let auth = token
            .or_else(|| std::env::var("GITHUB_TOKEN").ok())
            .filter(|t| !t.trim().is_empty())
            .map(|t| format!("Bearer {}", t.trim()));
        Ok(Self {
            client: build_http_client(auth)?,
        })
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
        Ok(parse_pr_metadata_json(&val, repo, pr_index))
    }

    async fn fetch_pr_comments(&self, repo: &str, pr_index: u64) -> Result<Vec<DiscussionComment>> {
        let mut comments = Vec::new();
        for (endpoint, include_loc) in [("pulls", true), ("issues", false)] {
            let url = format!(
                "https://api.github.com/repos/{}/{}/{}/comments?per_page=100",
                repo, endpoint, pr_index
            );
            if let Ok(res) = self.client.get(&url).send().await
                && res.status().is_success()
                && let Ok(val) = res.json::<serde_json::Value>().await
            {
                comments.extend(parse_comments_array(&val, include_loc));
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

            // Retry as a consolidated body when GitHub rejects inline line positions with 422.
            if status.as_u16() == 422 && !comments.is_empty() {
                warn!(
                    "GitHub rejected inline comments with 422 on PR #{}: {}. Retrying with consolidated review body...",
                    pr_index, err_text
                );

                let fallback_body = format_consolidated_review_body(summary, comments);
                let fallback_req = CreateReviewRequest {
                    body: &fallback_body,
                    event: "COMMENT",
                    comments: Vec::new(),
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
