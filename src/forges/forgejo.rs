use super::{Forge, InlineReviewComment};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};
use serde::Serialize;
use tracing::info;

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
        let mut headers = HeaderMap::new();
        if let Some(t) = token {
            headers.insert(
                AUTHORIZATION,
                HeaderValue::from_str(&format!("token {}", t))
                    .map_err(|e| anyhow!("Invalid auth token: {}", e))?,
            );
        }

        let client = reqwest::Client::builder()
            .default_headers(headers)
            .build()
            .map_err(|e| anyhow!("Failed to build HTTP client: {}", e))?;

        Ok(Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            client,
        })
    }
}

#[async_trait]
impl Forge for ForgejoClient {
    async fn fetch_pr_diff(&self, repo: &str, pr_index: u64) -> Result<String> {
        let url = format!("{}/api/v1/repos/{}/pulls/{}.diff", self.base_url, repo, pr_index);
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

    async fn post_review(
        &self,
        repo: &str,
        pr_index: u64,
        summary: &str,
        comments: &[InlineReviewComment],
    ) -> Result<()> {
        let url = format!("{}/api/v1/repos/{}/pulls/{}/reviews", self.base_url, repo, pr_index);

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
            return Err(anyhow!(
                "Forgejo review creation failed (status {}): {}",
                res.status(),
                res.text().await.unwrap_or_default()
            ));
        }

        info!("Successfully submitted review to Forgejo PR #{}", pr_index);
        Ok(())
    }
}
