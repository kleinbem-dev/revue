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

/// An existing review comment or thread comment on a PR.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscussionComment {
    pub author: String,
    pub path: Option<String>,
    pub line: Option<u64>,
    pub body: String,
}

/// Metadata and author intent for a Pull Request.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PrMetadata {
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub head_sha: String,
    #[serde(default)]
    pub head_ref: String,
    #[serde(default)]
    pub base_ref: String,
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default)]
    pub repo: String,
    #[serde(default)]
    pub pr_number: u64,
    #[serde(default)]
    pub is_draft: bool,
}

#[async_trait]
pub trait Forge: Send + Sync {
    /// Fetch the raw unified diff for a pull request
    async fn fetch_pr_diff(&self, repo: &str, pr_index: u64) -> Result<String>;

    /// Fetch pull request metadata including intent, author, and head SHA
    async fn fetch_pr_metadata(&self, repo: &str, pr_index: u64) -> Result<PrMetadata>;

    /// Fetch existing review comments and threads on the pull request
    async fn fetch_pr_comments(&self, _repo: &str, _pr_index: u64) -> Result<Vec<DiscussionComment>> {
        Ok(Vec::new())
    }

    /// Post an inline code review with comments and summary
    async fn post_review(
        &self,
        repo: &str,
        pr_index: u64,
        summary: &str,
        comments: &[InlineReviewComment],
    ) -> Result<()>;
}
