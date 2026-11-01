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
    config_path: String,
}

#[derive(Deserialize, Debug)]
struct WebhookPayload {
    action: Option<String>,
    number: Option<u64>,
    pull_request: Option<PullRequestObject>,
    repository: Option<RepositoryObject>,
}

#[derive(Deserialize, Debug)]
struct PullRequestObject {
    number: Option<u64>,
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

    if event != "pull_request" {
        return (
            StatusCode::OK,
            format!("Ignored event '{}' (only pull_request is handled)", event),
        );
    }

    let action = payload.action.as_deref().unwrap_or_default();
    if action != "opened" && action != "synchronized" && action != "reopened" {
        return (
            StatusCode::OK,
            format!("Ignored PR action '{}' (only opened/synchronize/reopen)", action),
        );
    }

    let pr_number = payload
        .number
        .or_else(|| payload.pull_request.as_ref().and_then(|pr| pr.number));

    let repo_full_name = payload
        .repository
        .as_ref()
        .and_then(|r| r.full_name.clone());

    let (pr_index, repo) = match (pr_number, repo_full_name) {
        (Some(num), Some(r)) => (num, r),
        _ => {
            warn!("Webhook missing PR number or repository name");
            return (StatusCode::BAD_REQUEST, "Missing PR number or repo name".to_string());
        }
    };

    info!("Received PR webhook for {} #{} (action: {})", repo, pr_index, action);

    let config_path = state.config_path.clone();
    let repo_for_review = repo.clone();

    // Spawn review asynchronously in background so webhook returns 200 immediately
    tokio::spawn(async move {
        if let Err(e) = run_pr_review(pr_index, Some(&repo_for_review), false, &config_path).await {
            error!("Failed to execute review for {} #{}: {:?}", repo_for_review, pr_index, e);
        }
    });

    (StatusCode::ACCEPTED, format!("Review queued for {} #{}", repo, pr_index))
}

pub async fn run_daemon(listen: &str, config_path: &str) -> Result<()> {
    let state = Arc::new(ServerState {
        config_path: config_path.to_string(),
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
