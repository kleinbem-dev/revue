use crate::config::RevueConfig;
use crate::reviewer::run_pr_review;
use anyhow::Result;
use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Router,
};
use hmac::{Hmac, KeyInit, Mac};
use serde::Deserialize;
use sha2::Sha256;
use std::sync::Arc;
use tracing::{error, info, warn};

type HmacSha256 = Hmac<Sha256>;

#[derive(Clone)]
struct ServerState {
    config_path: Option<String>,
    webhook_secret: Option<String>,
}

/// Verify HMAC-SHA256 signature from GitHub (`X-Hub-Signature-256`) or Forgejo/Gitea (`X-Gitea-Signature` / `X-Forgejo-Signature`)
pub fn verify_signature(secret: &str, headers: &HeaderMap, body: &[u8]) -> bool {
    let signature_header = headers
        .get("x-hub-signature-256")
        .or_else(|| headers.get("x-forgejo-signature"))
        .or_else(|| headers.get("x-gitea-signature"))
        .and_then(|h| h.to_str().ok());

    let Some(raw_sig) = signature_header else {
        warn!("Webhook signature header missing but webhook_secret is configured");
        return false;
    };

    // Strip "sha256=" prefix if present (GitHub style: sha256=<hex>, Forgejo style: <hex>)
    let hex_sig = raw_sig.strip_prefix("sha256=").unwrap_or(raw_sig).trim();

    let Ok(expected_sig) = hex::decode(hex_sig) else {
        warn!("Failed to decode webhook signature hex string: {}", hex_sig);
        return false;
    };

    let Ok(mut mac) = HmacSha256::new_from_slice(secret.as_bytes()) else {
        warn!("Invalid HMAC key length");
        return false;
    };

    mac.update(body);
    if mac.verify_slice(&expected_sig).is_err() {
        warn!("Webhook HMAC signature mismatch");
        return false;
    }

    true
}

#[derive(Deserialize, Debug)]
struct WebhookPayload {
    action: Option<String>,
    number: Option<u64>,
    pull_request: Option<PullRequestObject>,
    issue: Option<IssueObject>,
    comment: Option<CommentObject>,
    repository: Option<RepositoryObject>,
}

#[derive(Deserialize, Debug)]
struct PullRequestObject {
    number: Option<u64>,
}

#[derive(Deserialize, Debug)]
struct IssueObject {
    number: Option<u64>,
    pull_request: Option<serde_json::Value>,
}

#[derive(Deserialize, Debug)]
struct CommentObject {
    body: Option<String>,
}

#[derive(Deserialize, Debug)]
struct RepositoryObject {
    full_name: Option<String>,
}

async fn health_check() -> impl IntoResponse {
    (StatusCode::OK, "nix-revue daemon is healthy\n")
}

async fn handle_webhook(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    // 1. Verify HMAC signature if webhook_secret is configured
    if let Some(secret) = &state.webhook_secret {
        if !secret.is_empty() {
            if !verify_signature(secret, &headers, &body) {
                return (
                    StatusCode::UNAUTHORIZED,
                    "Webhook HMAC signature verification failed\n".to_string(),
                );
            }
        }
    }

    // 2. Parse JSON payload
    let payload: WebhookPayload = match serde_json::from_slice(&body) {
        Ok(p) => p,
        Err(e) => {
            warn!("Failed to parse webhook JSON payload: {:?}", e);
            return (
                StatusCode::BAD_REQUEST,
                format!("Invalid JSON payload: {}\n", e),
            );
        }
    };

    let event = headers
        .get("x-forgejo-event")
        .or_else(|| headers.get("x-gitea-event"))
        .or_else(|| headers.get("x-github-event"))
        .and_then(|h| h.to_str().ok())
        .unwrap_or_default();

    let action = payload.action.as_deref().unwrap_or_default();
    let repo_full_name = payload
        .repository
        .as_ref()
        .and_then(|r| r.full_name.clone());

    let pr_index = if event == "pull_request" {
        if action != "opened" && action != "synchronized" && action != "synchronize" && action != "reopened" {
            return (
                StatusCode::OK,
                format!("Ignored PR action '{}' (only opened/synchronize/reopen)", action),
            );
        }
        payload
            .number
            .or_else(|| payload.pull_request.as_ref().and_then(|pr| pr.number))
    } else if event == "issue_comment" {
        if action != "created" {
            return (
                StatusCode::OK,
                format!("Ignored comment action '{}' (only created)", action),
            );
        }

        // Only handle comments on pull requests
        let is_pr = payload.issue.as_ref().and_then(|i| i.pull_request.as_ref()).is_some();
        if !is_pr {
            return (
                StatusCode::OK,
                "Ignored comment on regular issue (not a pull request)".to_string(),
            );
        }

        let body = payload.comment.as_ref().and_then(|c| c.body.as_deref()).unwrap_or("");
        if !body.contains("@nix-revue") {
            return (
                StatusCode::OK,
                "Ignored PR comment (did not mention @nix-revue)".to_string(),
            );
        }

        payload.issue.as_ref().and_then(|i| i.number)
    } else {
        return (
            StatusCode::OK,
            format!("Ignored event '{}' (only pull_request and issue_comment are handled)", event),
        );
    };

    let (pr_num, repo) = match (pr_index, repo_full_name) {
        (Some(num), Some(r)) => (num, r),
        _ => {
            warn!("Webhook missing PR number or repository name");
            return (StatusCode::BAD_REQUEST, "Missing PR number or repo name".to_string());
        }
    };

    info!("Received {} webhook for {} #{} (action: {})", event, repo, pr_num, action);

    let config_path = state.config_path.clone();
    let repo_for_review = repo.clone();

    // Spawn review asynchronously in background so webhook returns 200 immediately
    tokio::spawn(async move {
        if let Err(e) = run_pr_review(pr_num, Some(&repo_for_review), false, config_path.as_deref()).await {
            error!("Failed to execute review for {} #{}: {:?}", repo_for_review, pr_num, e);
        }
    });

    (StatusCode::ACCEPTED, format!("Review queued for {} #{}", repo, pr_num))
}

pub async fn run_daemon(
    listen: &str,
    config_path: Option<&str>,
    webhook_secret: Option<&str>,
) -> Result<()> {
    // Determine webhook secret priority: CLI flag / env arg > config file > REVUE_WEBHOOK_SECRET env
    let resolved_secret = if let Some(s) = webhook_secret {
        Some(s.to_string())
    } else if let Ok(cfg) = RevueConfig::find_and_load(config_path) {
        cfg.forge.webhook_secret
    } else {
        std::env::var("REVUE_WEBHOOK_SECRET").ok()
    };

    if resolved_secret.is_some() {
        info!("Webhook HMAC-SHA256 signature verification is ENABLED");
    } else {
        warn!("Webhook signature verification is DISABLED (no secret configured). Set REVUE_WEBHOOK_SECRET to enable.");
    }

    let state = Arc::new(ServerState {
        config_path: config_path.map(|s| s.to_string()),
        webhook_secret: resolved_secret,
    });

    let app = Router::new()
        .route("/health", get(health_check))
        .route("/webhook", post(handle_webhook))
        .with_state(state);

    info!("nix-revue webhook listener bound to http://{}", listen);
    let listener = tokio::net::TcpListener::bind(listen).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn compute_hmac(secret: &str, body: &[u8]) -> String {
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(body);
        hex::encode(mac.finalize().into_bytes())
    }

    #[test]
    fn test_github_signature_verification() {
        let secret = "super-secret-key-123";
        let body = b"{\"action\":\"opened\",\"number\":42}";
        let sig = compute_hmac(secret, body);

        let mut headers = HeaderMap::new();
        headers.insert(
            "x-hub-signature-256",
            HeaderValue::from_str(&format!("sha256={}", sig)).unwrap(),
        );

        assert!(verify_signature(secret, &headers, body));
    }

    #[test]
    fn test_forgejo_signature_verification() {
        let secret = "another-secret-456";
        let body = b"{\"action\":\"created\"}";
        let sig = compute_hmac(secret, body);

        let mut headers = HeaderMap::new();
        headers.insert(
            "x-gitea-signature",
            HeaderValue::from_str(&sig).unwrap(),
        );

        assert!(verify_signature(secret, &headers, body));
    }

    #[test]
    fn test_signature_tampered_body_rejected() {
        let secret = "key";
        let body = b"original";
        let sig = compute_hmac(secret, body);

        let mut headers = HeaderMap::new();
        headers.insert(
            "x-hub-signature-256",
            HeaderValue::from_str(&format!("sha256={}", sig)).unwrap(),
        );

        let tampered_body = b"tampered";
        assert!(!verify_signature(secret, &headers, tampered_body));
    }

    #[test]
    fn test_signature_wrong_key_rejected() {
        let body = b"payload";
        let sig = compute_hmac("correct-key", body);

        let mut headers = HeaderMap::new();
        headers.insert(
            "x-hub-signature-256",
            HeaderValue::from_str(&format!("sha256={}", sig)).unwrap(),
        );

        assert!(!verify_signature("wrong-key", &headers, body));
    }

    #[test]
    fn test_missing_header_rejected() {
        let headers = HeaderMap::new();
        assert!(!verify_signature("secret", &headers, b"payload"));
    }
}
