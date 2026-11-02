use crate::reviewer::run_pr_review;
use anyhow::Result;
use axum::{
    extract::{Json, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Router,
};
use serde::Deserialize;
use std::sync::Arc;
use tracing::{error, info, warn};

#[derive(Clone)]
struct ServerState {
    config_path: Option<String>,
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
    Json(payload): Json<WebhookPayload>,
) -> impl IntoResponse {
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

pub async fn run_daemon(listen: &str, config_path: Option<&str>) -> Result<()> {
    let state = Arc::new(ServerState {
        config_path: config_path.map(|s| s.to_string()),
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
