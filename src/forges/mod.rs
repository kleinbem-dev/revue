use anyhow::{Result, anyhow};
use async_trait::async_trait;
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue, USER_AGENT};
use serde::{Deserialize, Serialize};
use std::time::Duration;

pub mod forgejo;
pub mod github;

pub(crate) const USER_AGENT_VALUE: &str = concat!("revue/", env!("CARGO_PKG_VERSION"));

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
    async fn fetch_pr_comments(
        &self,
        _repo: &str,
        _pr_index: u64,
    ) -> Result<Vec<DiscussionComment>> {
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

pub(crate) fn build_http_client(auth_header_value: Option<String>) -> Result<reqwest::Client> {
    let mut headers = HeaderMap::new();
    headers.insert(USER_AGENT, HeaderValue::from_static(USER_AGENT_VALUE));
    if let Some(auth) = auth_header_value {
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&auth).map_err(|e| anyhow!("Invalid auth token: {}", e))?,
        );
    }

    reqwest::Client::builder()
        .default_headers(headers)
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| anyhow!("Failed to build HTTP client: {}", e))
}

pub(crate) fn parse_pr_metadata_json(
    val: &serde_json::Value,
    repo: &str,
    pr_index: u64,
) -> PrMetadata {
    let str_field =
        |v: Option<&serde_json::Value>| v.and_then(|x| x.as_str()).unwrap_or("").to_string();
    let author = val
        .get("user")
        .and_then(|u| u.get("login").or_else(|| u.get("username")))
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();
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

    PrMetadata {
        title: str_field(val.get("title")),
        description: str_field(val.get("body")),
        author,
        head_sha: str_field(val.get("head").and_then(|h| h.get("sha"))),
        head_ref: str_field(val.get("head").and_then(|h| h.get("ref"))),
        base_ref: str_field(val.get("base").and_then(|b| b.get("ref"))),
        labels,
        repo: repo.to_string(),
        pr_number: pr_index,
        is_draft: val.get("draft").and_then(|d| d.as_bool()).unwrap_or(false),
    }
}

pub(crate) fn parse_comments_array(
    val: &serde_json::Value,
    include_location: bool,
) -> Vec<DiscussionComment> {
    let Some(arr) = val.as_array() else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|item| {
            let body = item.get("body").and_then(|b| b.as_str())?.to_string();
            let author = item
                .get("user")
                .and_then(|u| u.get("login").or_else(|| u.get("username")))
                .and_then(|l| l.as_str())
                .unwrap_or("unknown")
                .to_string();
            let (path, line) = if include_location {
                let path = item.get("path").and_then(|p| p.as_str()).map(String::from);
                let line = item
                    .get("line")
                    .or_else(|| item.get("position"))
                    .or_else(|| item.get("original_line"))
                    .or_else(|| item.get("original_position"))
                    .and_then(|l| l.as_u64());
                (path, line)
            } else {
                (None, None)
            };
            Some(DiscussionComment {
                author,
                path,
                line,
                body,
            })
        })
        .collect()
}

pub(crate) fn format_consolidated_review_body(
    summary: &str,
    comments: &[InlineReviewComment],
) -> String {
    let mut fallback_body = summary.to_string();
    fallback_body.push_str("\n\n### 📝 Inline Findings (Consolidated)\n\n");
    for c in comments {
        fallback_body.push_str(&format!("- **`{}:{}`**\n{}\n\n", c.path, c.line, c.body));
    }
    fallback_body
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_pr_metadata_and_comments() {
        let pr_json = serde_json::json!({
            "title": "Fix race condition",
            "body": "Closes #12",
            "draft": true,
            "user": { "username": "alice" },
            "head": { "sha": "deadbeef1234", "ref": "fix/race" },
            "base": { "ref": "main" },
            "labels": [{ "name": "bug" }, { "name": "urgent" }]
        });
        let meta = parse_pr_metadata_json(&pr_json, "owner/repo", 12);
        assert_eq!(meta.title, "Fix race condition");
        assert_eq!(meta.author, "alice");
        assert_eq!(meta.head_sha, "deadbeef1234");
        assert_eq!(meta.labels, vec!["bug", "urgent"]);
        assert!(meta.is_draft);

        let comments_json = serde_json::json!([
            { "user": { "login": "bob" }, "path": "src/lib.rs", "position": 18, "body": "Check bounds" }
        ]);
        let inline = parse_comments_array(&comments_json, true);
        assert_eq!(inline.len(), 1);
        assert_eq!(inline[0].author, "bob");
        assert_eq!(inline[0].path.as_deref(), Some("src/lib.rs"));
        assert_eq!(inline[0].line, Some(18));

        let general = parse_comments_array(&comments_json, false);
        assert_eq!(general[0].path, None);
        assert_eq!(general[0].line, None);
    }
}
