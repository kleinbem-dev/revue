use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

pub mod forgejo;
pub mod github;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InlineReviewComment {
    pub path: String,
    pub line: u64,
    pub body: String,
}

#[async_trait]
pub trait Forge: Send + Sync {
    /// Fetch the raw unified diff for a pull request
    async fn fetch_pr_diff(&self, repo: &str, pr_index: u64) -> Result<String>;

    /// Post an inline code review with comments and summary
    async fn post_review(
        &self,
        repo: &str,
        pr_index: u64,
        summary: &str,
        comments: &[InlineReviewComment],
    ) -> Result<()>;
}
