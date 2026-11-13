use super::{
    DiscussionComment, Forge, InlineReviewComment, PrMetadata, build_http_client,
    format_consolidated_review_body, parse_comments_array, parse_pr_metadata_json,
};
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use serde::Serialize;
use tracing::{info, warn};

pub struct ForgejoClient {
    base_url: String,
    client: reqwest::Client,
}

#[derive(Serialize)]
struct CreateReviewRequest<'a> {
    body: &'a str,
    event: &'a str,
    comments: Vec<ForgejoReviewComment<'a>>,
}

#[derive(Serialize)]
struct ForgejoReviewComment<'a> {
    path: &'a str,
    new_position: u64,
    body: &'a str,
}

impl ForgejoClient {
    pub fn new(base_url: String, token: Option<String>) -> Result<Self> {
        let clean_url = base_url.trim().trim_end_matches('/').to_string();
        if clean_url.is_empty() {
            return Err(anyhow!(
                "Forgejo/Gitea base URL is empty; set forge.url in .revue.yaml or FORGEJO_URL"
            ));
        }
        let auth = token
            .filter(|t| !t.trim().is_empty())
            .map(|t| format!("token {}", t.trim()));
        Ok(Self {
            base_url: clean_url,
            client: build_http_client(auth)?,
        })
    }
}

#[async_trait]
impl Forge for ForgejoClient {
    async fn fetch_pr_diff(&self, repo: &str, pr_index: u64) -> Result<String> {
        let url = format!(
            "{}/api/v1/repos/{}/pulls/{}.diff",
            self.base_url, repo, pr_index
        );
        let res = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| anyhow!("Failed to fetch PR diff from {}: {}", url, e))?;

        if !res.status().is_success() {
            return Err(anyhow!(
                "Failed to get diff from Forgejo (status {}): {}",
                res.status(),
                res.text().await.unwrap_or_default()
            ));
        }

        res.text()
            .await
            .map_err(|e| anyhow!("Failed to read diff response: {}", e))
    }

    async fn fetch_pr_metadata(&self, repo: &str, pr_index: u64) -> Result<PrMetadata> {
        let url = format!("{}/api/v1/repos/{}/pulls/{}", self.base_url, repo, pr_index);
        let res = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| anyhow!("Failed to fetch PR metadata from {}: {}", url, e))?;

        if !res.status().is_success() {
            return Err(anyhow!(
                "Failed to get PR metadata from Forgejo (status {}): {}",
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
                "{}/api/v1/repos/{}/{}/{}/comments?limit=100",
                self.base_url, repo, endpoint, pr_index
            );
            if let Ok(res) = self.client.get(&url).send().await {
                if res.status().is_success() {
                    if let Ok(val) = res.json::<serde_json::Value>().await {
                        comments.extend(parse_comments_array(&val, include_loc));
                    }
                } else {
                    warn!(
                        "Failed to fetch Forgejo PR #{} {} comments: HTTP {}",
                        pr_index,
                        endpoint,
                        res.status()
                    );
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
            "{}/api/v1/repos/{}/pulls/{}/reviews",
            self.base_url, repo, pr_index
        );

        let forgejo_comments: Vec<_> = comments
            .iter()
            .map(|c| ForgejoReviewComment {
                path: &c.path,
                new_position: c.line,
                body: &c.body,
            })
            .collect();

        let req = CreateReviewRequest {
            body: summary,
            event: "COMMENT",
            comments: forgejo_comments,
        };

        let res = self
            .client
            .post(&url)
            .json(&req)
            .send()
            .await
            .map_err(|e| anyhow!("Failed to post review to {}: {}", url, e))?;

        if !res.status().is_success() {
            let status = res.status();
            let err_text = res.text().await.unwrap_or_default();

            if (status.as_u16() == 422 || status.as_u16() == 400) && !comments.is_empty() {
                warn!(
                    "Forgejo rejected inline comments (status {}): {}. Retrying with consolidated review body...",
                    status, err_text
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
                    .map_err(|e| anyhow!("Failed to post fallback review to Forgejo: {}", e))?;

                if !fallback_res.status().is_success() {
                    return Err(anyhow!(
                        "Forgejo review creation fallback also failed (status {}): {}",
                        fallback_res.status(),
                        fallback_res.text().await.unwrap_or_default()
                    ));
                }

                info!(
                    "Successfully submitted fallback review to Forgejo PR #{}",
                    pr_index
                );
                return Ok(());
            }

            return Err(anyhow!(
                "Forgejo review creation failed (status {}): {}",
                status,
                err_text
            ));
        }

        info!("Successfully submitted review to Forgejo PR #{}", pr_index);
        Ok(())
    }
}
