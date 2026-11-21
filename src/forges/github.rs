use super::{
    DiscussionComment, Forge, InlineReviewComment, PrMetadata, base64_encode, build_http_client,
    format_consolidated_review_body, parse_comments_array, parse_pr_metadata_json,
};
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::io::AsyncWriteExt;
use tokio::sync::RwLock;
use tracing::{info, warn};

const TOKEN_REFRESH_MARGIN_SECS: i64 = 120;

#[derive(Clone)]
struct CachedToken {
    token: String,
    expires_at: i64,
}

struct GitHubAppAuth {
    app_id: String,
    private_key_pem: String,
    installation_ids: RwLock<HashMap<String, u64>>,
    tokens: RwLock<HashMap<String, CachedToken>>,
}

enum GitHubAuth {
    Static(Option<String>),
    App(Box<GitHubAppAuth>),
}

struct TempKeyFile {
    path: PathBuf,
}

impl Drop for TempKeyFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

pub struct GitHubClient {
    client: reqwest::Client,
    auth: GitHubAuth,
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

fn non_empty_env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn load_app_credentials() -> Result<Option<(String, String)>> {
    let Some(app_id) = non_empty_env("GITHUB_APP_ID") else {
        return Ok(None);
    };
    if let Some(key_file) = non_empty_env("GITHUB_APP_PRIVATE_KEY_FILE") {
        let pem = std::fs::read_to_string(&key_file).map_err(|e| {
            anyhow!(
                "Failed to read GITHUB_APP_PRIVATE_KEY_FILE '{}': {}",
                key_file,
                e
            )
        })?;
        return Ok(Some((app_id, pem)));
    }
    if let Some(key_or_path) = non_empty_env("GITHUB_APP_PRIVATE_KEY") {
        let pem = if key_or_path.contains("-----BEGIN") {
            key_or_path
        } else {
            std::fs::read_to_string(&key_or_path).map_err(|e| {
                anyhow!(
                    "Failed to read GITHUB_APP_PRIVATE_KEY path '{}': {}",
                    key_or_path,
                    e
                )
            })?
        };
        return Ok(Some((app_id, pem)));
    }
    Ok(None)
}

async fn write_temp_pem(pem: &str) -> Result<TempKeyFile> {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = if Path::new("/dev/shm").is_dir() {
        PathBuf::from("/dev/shm")
    } else {
        std::env::temp_dir()
    };
    let path = dir.join(format!("revue-gh-app-{}-{}.pem", std::process::id(), n));
    #[cfg(unix)]
    {
        let mut opts = tokio::fs::OpenOptions::new();
        opts.write(true).create_new(true).mode(0o600);
        let mut f = opts.open(&path).await?;
        f.write_all(pem.as_bytes()).await?;
        if !pem.ends_with('\n') {
            f.write_all(b"\n").await?;
        }
        f.flush().await?;
    }
    #[cfg(not(unix))]
    {
        tokio::fs::write(&path, pem.as_bytes()).await?;
    }
    Ok(TempKeyFile { path })
}

async fn mint_app_jwt(app_id: &str, private_key_pem: &str, now_secs: i64) -> Result<String> {
    let hdr = base64_encode(br#"{"alg":"RS256","typ":"JWT"}"#, true);
    let payload_json = serde_json::json!({
        "iat": now_secs - 30,
        "exp": now_secs + 120,
        "iss": app_id,
    })
    .to_string();
    let payload = base64_encode(payload_json.as_bytes(), true);
    let signing_input = format!("{}.{}", hdr, payload);

    let key_file = write_temp_pem(private_key_pem).await?;
    let mut child = tokio::process::Command::new("openssl")
        .args(["dgst", "-sha256", "-sign"])
        .arg(&key_file.path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| anyhow!("Failed to spawn openssl for GitHub App JWT: {}", e))?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(signing_input.as_bytes()).await?;
    }

    let output = tokio::time::timeout(std::time::Duration::from_secs(10), child.wait_with_output())
        .await
        .map_err(|_| anyhow!("Timed out signing GitHub App JWT with openssl"))??;

    if !output.status.success() {
        return Err(anyhow!(
            "openssl JWT signing failed ({}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    let sig = base64_encode(&output.stdout, true);
    Ok(format!("{}.{}", signing_input, sig))
}

impl GitHubClient {
    pub fn new(token: Option<String>) -> Result<Self> {
        let auth = if let Some((app_id, private_key_pem)) = load_app_credentials()? {
            info!(
                "GitHub forge configured with GitHub App auth (app_id={})",
                app_id
            );
            GitHubAuth::App(Box::new(GitHubAppAuth {
                app_id,
                private_key_pem,
                installation_ids: RwLock::new(HashMap::new()),
                tokens: RwLock::new(HashMap::new()),
            }))
        } else {
            let resolved = token
                .or_else(|| std::env::var("GITHUB_TOKEN").ok())
                .map(|t| t.trim().to_string())
                .filter(|t| !t.is_empty());
            GitHubAuth::Static(resolved)
        };
        Ok(Self {
            client: build_http_client(None)?,
            auth,
        })
    }

    async fn resolve_token(&self, repo: &str) -> Result<Option<String>> {
        match &self.auth {
            GitHubAuth::Static(tok) => Ok(tok.clone()),
            GitHubAuth::App(app) => {
                let GitHubAppAuth {
                    app_id,
                    private_key_pem,
                    installation_ids,
                    tokens,
                } = app.as_ref();
                let owner = repo.split('/').next().unwrap_or(repo);
                let now = jiff::Timestamp::now().as_second();
                if let Some(cached) = tokens.read().await.get(owner)
                    && now + TOKEN_REFRESH_MARGIN_SECS < cached.expires_at
                {
                    return Ok(Some(cached.token.clone()));
                }

                let jwt = mint_app_jwt(app_id, private_key_pem, now).await?;
                let installation_id = if let Some(id) =
                    installation_ids.read().await.get(owner).copied()
                {
                    id
                } else {
                    let inst_url = format!("https://api.github.com/repos/{}/installation", repo);
                    let res = self
                        .client
                        .get(&inst_url)
                        .header("Authorization", format!("Bearer {}", jwt))
                        .header("Accept", "application/vnd.github+json")
                        .send()
                        .await
                        .map_err(|e| {
                            anyhow!(
                                "Failed to look up GitHub App installation for {}: {}",
                                repo,
                                e
                            )
                        })?;
                    if !res.status().is_success() {
                        return Err(anyhow!(
                            "GitHub App installation lookup failed for {} (status {}): {}",
                            repo,
                            res.status(),
                            res.text().await.unwrap_or_default()
                        ));
                    }
                    let val: serde_json::Value = res.json().await?;
                    let id = val.get("id").and_then(|v| v.as_u64()).ok_or_else(|| {
                        anyhow!("Missing installation id in GitHub response for {}", repo)
                    })?;
                    installation_ids.write().await.insert(owner.to_string(), id);
                    id
                };

                let token_url = format!(
                    "https://api.github.com/app/installations/{}/access_tokens",
                    installation_id
                );
                let res = self
                    .client
                    .post(&token_url)
                    .header("Authorization", format!("Bearer {}", jwt))
                    .header("Accept", "application/vnd.github+json")
                    .send()
                    .await
                    .map_err(|e| anyhow!("Failed to mint GitHub App installation token: {}", e))?;
                if !res.status().is_success() {
                    return Err(anyhow!(
                        "GitHub App access_tokens failed for {} (status {}): {}",
                        repo,
                        res.status(),
                        res.text().await.unwrap_or_default()
                    ));
                }
                let val: serde_json::Value = res.json().await?;
                let token = val
                    .get("token")
                    .and_then(|v| v.as_str())
                    .map(String::from)
                    .ok_or_else(|| {
                        anyhow!("Missing installation token in GitHub response for {}", repo)
                    })?;
                let expires_at = val
                    .get("expires_at")
                    .and_then(|v| v.as_str())
                    .and_then(|s| s.parse::<jiff::Timestamp>().ok())
                    .map(|t| t.as_second())
                    .unwrap_or(now + 3300);

                tokens.write().await.insert(
                    owner.to_string(),
                    CachedToken {
                        token: token.clone(),
                        expires_at,
                    },
                );
                Ok(Some(token))
            }
        }
    }

    async fn authed_request(
        &self,
        method: reqwest::Method,
        url: &str,
        repo: &str,
    ) -> Result<reqwest::RequestBuilder> {
        let mut req = self.client.request(method, url);
        if let Some(tok) = self.resolve_token(repo).await? {
            req = req.header("Authorization", format!("Bearer {}", tok));
        }
        Ok(req)
    }
}

#[async_trait]
impl Forge for GitHubClient {
    async fn fetch_pr_diff(&self, repo: &str, pr_index: u64) -> Result<String> {
        let url = format!("https://api.github.com/repos/{}/pulls/{}", repo, pr_index);
        let res = self
            .authed_request(reqwest::Method::GET, &url, repo)
            .await?
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
            .authed_request(reqwest::Method::GET, &url, repo)
            .await?
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
            if let Ok(req) = self.authed_request(reqwest::Method::GET, &url, repo).await
                && let Ok(res) = req.send().await
                && res.status().is_success()
                && let Ok(val) = res.json::<serde_json::Value>().await
            {
                comments.extend(parse_comments_array(&val, include_loc));
            }
        }
        Ok(comments)
    }

    async fn git_token(&self, repo: &str) -> Option<String> {
        match self.resolve_token(repo).await {
            Ok(tok) => tok,
            Err(e) => {
                warn!("Failed to resolve GitHub git_token for {}: {}", repo, e);
                None
            }
        }
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
            .authed_request(reqwest::Method::POST, &url, repo)
            .await?
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
                    .authed_request(reqwest::Method::POST, &url, repo)
                    .await?
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_encode_matches_rfc4648() {
        assert_eq!(base64_encode(b"foobar", false), "Zm9vYmFy");
        assert_eq!(base64_encode(b"foo", false), "Zm9v");
        assert_eq!(base64_encode(b"f", false), "Zg==");
        assert_eq!(base64_encode(b"f", true), "Zg");
        assert_eq!(
            base64_encode(br#"{"alg":"RS256","typ":"JWT"}"#, true),
            "eyJhbGciOiJSUzI1NiIsInR5cCI6IkpXVCJ9"
        );
    }

    #[tokio::test]
    async fn static_token_falls_back_when_no_app_env() {
        let client = GitHubClient {
            client: build_http_client(None).unwrap(),
            auth: GitHubAuth::Static(Some("  tok-123  ".trim().to_string())),
        };
        assert_eq!(
            client.git_token("kleinbem/nix-config").await.as_deref(),
            Some("tok-123")
        );
    }
}
