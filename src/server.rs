use crate::config::RevueConfig;
use crate::reviewer::run_pr_review;
use anyhow::Result;
use axum::{
    body::Bytes,
    extract::{Request, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    middleware::{self, Next},
    response::{Html, IntoResponse, Json, Response},
    routing::{get, post},
    Router,
};
use hmac::{Hmac, KeyInit, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::{ServeDir, ServeFile};
use tracing::{error, info, warn};

type HmacSha256 = Hmac<Sha256>;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Finding {
    pub file: String,
    pub line: u64,
    pub severity: String,
    pub title: String,
    #[serde(alias = "comment")]
    pub body: String,
    #[serde(default, alias = "suggestion")]
    pub snippet: Option<String>,
    pub confidence: Option<u8>,
    #[serde(default)]
    pub feedback: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ReviewRecord {
    pub id: String,
    pub repo: String,
    pub pr_number: u64,
    pub title: Option<String>,
    pub status: String,
    pub pass1_candidates: u32,
    pub pass2_verified: u32,
    pub duration_secs: u64,
    pub provider_used: String,
    pub created_at: String,
    pub summary: String,
    pub forge: String,
    pub findings: Vec<Finding>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DaemonMetrics {
    pub total_reviews: u64,
    pub pass1_candidates: u64,
    pub pass2_filtered: u64,
    pub verified_findings: u64,
    pub avg_cost_saved: String,
    pub uptime: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ProviderStatus {
    pub name: String,
    pub r#type: String,
    pub model: String,
    pub status: String,
    pub cost_per_token: String,
    pub latency_avg: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DaemonStatusResponse {
    pub status: String,
    pub version: String,
    pub engine: String,
    pub uptime_secs: u64,
    pub metrics: DaemonMetrics,
    pub providers: Vec<ProviderStatus>,
}

#[derive(Deserialize, Debug)]
pub struct TriggerRequest {
    pub repo: String,
    #[serde(alias = "number")]
    pub pr: u64,
    #[serde(default)]
    pub publish: Option<bool>,
}

#[derive(Deserialize, Debug)]
pub struct FeedbackRequest {
    pub review_id: Option<String>,
    pub repo: String,
    pub file_path: String,
    pub issue_title: String,
    pub feedback_type: String,
    pub reason: Option<String>,
}

#[derive(Deserialize, Debug)]
pub struct DeleteFeedbackRequest {
    pub id: i64,
}

#[derive(Clone)]
struct ServerState {
    config_path: Option<String>,
    webhook_secret: Option<String>,
    start_time: Instant,
    db: Option<crate::db::DbPool>,
    reviews: Arc<RwLock<Vec<ReviewRecord>>>,
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

/// Health check endpoint returning simple plain text confirmation.
async fn health_check() -> impl IntoResponse {
    (StatusCode::OK, "revue daemon is healthy\n")
}

/// Returns daemon health, uptime, provider status, and aggregate review telemetry.
async fn get_status(State(state): State<Arc<ServerState>>) -> impl IntoResponse {
    let uptime_secs = state.start_time.elapsed().as_secs();
    let hours = uptime_secs / 3600;
    let mins = (uptime_secs % 3600) / 60;
    let uptime_str = if hours > 0 {
        format!("{}h {}m", hours, mins)
    } else {
        format!("{}m", mins)
    };

    let (total_reviews, pass1, pass2, filtered, cost_saved_str) = if let Some(db) = &state.db {
        match db.get_metrics().await {
            Ok(m) => {
                let total = m.total_reviews as u64;
                let p1 = m.pass1_candidates as u64;
                let p2 = m.verified_findings as u64;
                let filt = m.pass2_filtered as u64;
                let cost = format!("${:.2}", m.total_cost_saved);
                (total, p1, p2, filt, cost)
            }
            Err(e) => {
                warn!("Failed to query DB metrics: {}", e);
                let reviews = state.reviews.read().await;
                let total = reviews.len() as u64;
                let p1: u64 = reviews.iter().map(|r| r.pass1_candidates as u64).sum();
                let p2: u64 = reviews.iter().map(|r| r.pass2_verified as u64).sum();
                let filt = p1.saturating_sub(p2);
                (total, p1, p2, filt, "$0.00".to_string())
            }
        }
    } else {
        let reviews = state.reviews.read().await;
        let total = reviews.len() as u64;
        let p1: u64 = reviews.iter().map(|r| r.pass1_candidates as u64).sum();
        let p2: u64 = reviews.iter().map(|r| r.pass2_verified as u64).sum();
        let filt = p1.saturating_sub(p2);
        (total, p1, p2, filt, "$0.00".to_string())
    };

    let metrics = DaemonMetrics {
        total_reviews,
        pass1_candidates: pass1,
        pass2_filtered: filtered,
        verified_findings: pass2,
        avg_cost_saved: cost_saved_str,
        uptime: uptime_str,
    };

    let providers = vec![
        ProviderStatus {
            name: "Gemini (Subscription)".to_string(),
            r#type: "subscription".to_string(),
            model: "gemini-2.5-pro / flash".to_string(),
            status: "active".to_string(),
            cost_per_token: "$0 (Flat)".to_string(),
            latency_avg: "2.1s".to_string(),
        },
        ProviderStatus {
            name: "Mesh-LLM (Fleet P2P)".to_string(),
            r#type: "mesh-gpu".to_string(),
            model: "Qwen3-4B-Q4_K_M (CUDA)".to_string(),
            status: "ready".to_string(),
            cost_per_token: "$0 (P2P GPU)".to_string(),
            latency_avg: "1.8s".to_string(),
        },
        ProviderStatus {
            name: "Gemini API".to_string(),
            r#type: "api".to_string(),
            model: "gemini-2.5-flash".to_string(),
            status: "ready".to_string(),
            cost_per_token: "Pay-as-you-go".to_string(),
            latency_avg: "1.4s".to_string(),
        },
        ProviderStatus {
            name: "Local Ollama".to_string(),
            r#type: "local".to_string(),
            model: "qwen2.5-coder:14b".to_string(),
            status: "ready".to_string(),
            cost_per_token: "$0 (Air-gapped)".to_string(),
            latency_avg: "4.8s".to_string(),
        },
        ProviderStatus {
            name: "Claude Code".to_string(),
            r#type: "quota-shield".to_string(),
            model: "claude-3-7-sonnet".to_string(),
            status: "quota-shield".to_string(),
            cost_per_token: "Shielded".to_string(),
            latency_avg: "N/A".to_string(),
        },
    ];

    Json(DaemonStatusResponse {
        status: "healthy".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        engine: "revue".to_string(),
        uptime_secs,
        metrics,
        providers,
    })
}

/// Returns the list of recent pull request reviews from database and in-memory states.
async fn get_reviews(State(state): State<Arc<ServerState>>) -> impl IntoResponse {
    // If no DB is configured, return all in-memory records directly.
    if state.db.is_none() {
        let reviews = state.reviews.read().await;
        return Json(reviews.clone());
    }

    let mut records: Vec<ReviewRecord> = Vec::new();

    // 1. In-memory running reviews
    let running_records: Vec<ReviewRecord> = {
        let reviews = state.reviews.read().await;
        reviews
            .iter()
            .filter(|r| r.status == "running")
            .cloned()
            .collect()
    };
    records.extend(running_records.iter().cloned());

    // 2. Persistent reviews from DB
    if let Some(db) = &state.db {
        if let Ok(db_reviews) = db.list_recent_reviews(50).await {
            for r in db_reviews {
                // Skip DB record only if the same PR is actively running in-memory
                if running_records
                    .iter()
                    .any(|rec| rec.repo == r.repo && rec.pr_number == r.pr_number as u64)
                {
                    continue;
                }
                let findings: Vec<Finding> =
                    serde_json::from_str(&r.findings_json).unwrap_or_else(|err| {
                        warn!("Failed to parse findings for review {}: {}", r.id, err);
                        Vec::new()
                    });
                records.push(ReviewRecord {
                    id: r.id.to_string(),
                    repo: r.repo.clone(),
                    pr_number: r.pr_number as u64,
                    title: Some(r.title),
                    status: r.status,
                    pass1_candidates: r.pass1_candidates as u32,
                    pass2_verified: r.verified_findings as u32,
                    duration_secs: (r.duration_ms / 1000) as u64,
                    provider_used: r.provider_used,
                    created_at: r.created_at,
                    summary: r.summary,
                    forge: if r.repo.contains("git.") {
                        "forgejo".to_string()
                    } else {
                        "github".to_string()
                    },
                    findings,
                });
            }
        }
    }

    // 3. Fall back to all in-memory state if no records were found
    if records.is_empty() {
        let reviews = state.reviews.read().await;
        return Json(reviews.clone());
    }

    Json(records)
}

async fn record_finding_feedback(
    State(state): State<Arc<ServerState>>,
    Json(req): Json<FeedbackRequest>,
) -> impl IntoResponse {
    info!(
        "Received feedback '{}' for {} [{}: {}]",
        req.feedback_type, req.repo, req.file_path, req.issue_title
    );

    // 1. Record into persistent DB if available
    if let Some(db) = &state.db {
        if let Err(e) = db
            .record_feedback(
                &req.repo,
                &req.file_path,
                &req.issue_title,
                &req.feedback_type,
                req.reason.as_deref(),
            )
            .await
        {
            error!("Failed to record feedback to db: {:?}", e);
        }
    }

    // 2. Update in-memory reviews state
    let mut reviews = state.reviews.write().await;
    for review in reviews.iter_mut() {
        if req.review_id.as_deref() == Some(&review.id) || review.repo == req.repo {
            for finding in review.findings.iter_mut() {
                if finding.file == req.file_path && finding.title == req.issue_title {
                    finding.feedback = Some(req.feedback_type.clone());
                }
            }
        }
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "status": "success",
            "message": format!("Feedback '{}' recorded for {}", req.feedback_type, req.issue_title)
        })),
    )
}

async fn get_all_feedback(State(state): State<Arc<ServerState>>) -> impl IntoResponse {
    if let Some(db) = &state.db {
        match db.list_all_feedback(100).await {
            Ok(feedback) => Json(serde_json::json!({
                "status": "success",
                "feedback": feedback
            })),
            Err(e) => {
                error!("Failed to query feedback: {:?}", e);
                Json(serde_json::json!({
                    "status": "error",
                    "feedback": []
                }))
            }
        }
    } else {
        Json(serde_json::json!({
            "status": "success",
            "feedback": []
        }))
    }
}

async fn delete_feedback_entry(
    State(state): State<Arc<ServerState>>,
    Json(req): Json<DeleteFeedbackRequest>,
) -> impl IntoResponse {
    if let Some(db) = &state.db {
        match db.delete_feedback(req.id).await {
            Ok(true) => (
                StatusCode::OK,
                Json(serde_json::json!({ "status": "success", "message": "Feedback deleted" })),
            ),
            Ok(false) => (
                StatusCode::NOT_FOUND,
                Json(
                    serde_json::json!({ "status": "not_found", "message": "Feedback ID not found" }),
                ),
            ),
            Err(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "status": "error", "message": e.to_string() })),
            ),
        }
    } else {
        (
            StatusCode::OK,
            Json(
                serde_json::json!({ "status": "success", "message": "No persistent db configured" }),
            ),
        )
    }
}

async fn get_available_skills() -> impl IntoResponse {
    let skills_dir = crate::skills::find_skills_dir(None);
    let skills = crate::skills::list_all_skills(skills_dir.as_deref());
    Json(serde_json::json!({
        "skills_dir": skills_dir,
        "skills": skills
    }))
}

async fn handle_synthesize_rule(
    State(state): State<Arc<ServerState>>,
    Json(req): Json<crate::synthesizer::RuleSynthesizeRequest>,
) -> impl IntoResponse {
    let repo_clean = req.repo.trim();
    if repo_clean.is_empty()
        || repo_clean.contains("..")
        || repo_clean.starts_with('/')
        || repo_clean.starts_with('\\')
        || repo_clean.split('/').count() > 2
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(crate::synthesizer::RuleSynthesizeResponse {
                success: false,
                rule_id: String::new(),
                yaml: String::new(),
                validated: false,
                saved_path: None,
                error: Some(format!(
                    "Invalid or untrusted repository name: '{}'",
                    req.repo
                )),
            }),
        );
    }

    if req.commit && crate::synthesizer::resolve_repo_root(&req.repo).is_none() {
        return (
            StatusCode::FORBIDDEN,
            Json(crate::synthesizer::RuleSynthesizeResponse {
                success: false,
                rule_id: String::new(),
                yaml: String::new(),
                validated: false,
                saved_path: None,
                error: Some(format!(
                    "Repository '{}' is not recognized or permitted for local rule persistence",
                    req.repo
                )),
            }),
        );
    }

    let config =
        crate::config::RevueConfig::find_and_load(state.config_path.as_deref()).unwrap_or_default();
    let chain = crate::reviewer::build_provider_chain(&config);
    let resp = crate::synthesizer::synthesize_rule(req, &chain).await;
    if resp.success {
        (StatusCode::OK, Json(resp))
    } else {
        (StatusCode::UNPROCESSABLE_ENTITY, Json(resp))
    }
}

/// Triggers an asynchronous code review for the specified pull request.
async fn trigger_review(
    State(state): State<Arc<ServerState>>,
    Json(req): Json<TriggerRequest>,
) -> impl IntoResponse {
    let repo = req.repo.clone();
    let pr = req.pr;

    let publish = req.publish.unwrap_or(false);
    info!(
        "Received manual review trigger for {} #{} (publish: {})",
        repo, pr, publish
    );

    let review_id = format!("rev-manual-{}", pr);
    let pending_review = ReviewRecord {
        id: review_id.clone(),
        repo: repo.clone(),
        pr_number: pr,
        title: Some(format!("PR #{}: Inspecting pull request changes", pr)),
        status: "running".to_string(),
        pass1_candidates: 0,
        pass2_verified: 0,
        duration_secs: 0,
        provider_used: "Cascade (Gemini 2.5 Pro → Mesh-LLM P2P)".to_string(),
        created_at: chrono::Utc::now().to_rfc3339(),
        summary: if publish {
            "Autonomous two-pass review executing (live publishing enabled): Pass 1 Detective broad discovery, Pass 2 Judge verification with direct forge commenting.".to_string()
        } else {
            "Autonomous two-pass review executing: Pass 1 Detective broad anomaly discovery, Pass 2 Judge verification with episodic brain constraints.".to_string()
        },
        forge: if repo.contains("git.") {
            "forgejo".to_string()
        } else {
            "github".to_string()
        },
        findings: Vec::new(),
    };

    {
        let mut reviews = state.reviews.write().await;
        reviews.retain(|r| r.pr_number != pr || r.repo != repo);
        reviews.insert(0, pending_review);
    }

    let config_path = state.config_path.clone();
    let repo_clone = repo.clone();
    let state_clone = state.clone();

    tokio::spawn(async move {
        let start = std::time::Instant::now();
        let dry_run = !publish;
        let result = run_pr_review(
            pr,
            Some(&repo_clone),
            dry_run,
            config_path.as_deref(),
            state_clone.db.as_ref(),
        )
        .await;
        let duration_ms = start.elapsed().as_millis() as u64;

        let mut reviews = state_clone.reviews.write().await;
        if let Some(r) = reviews.iter_mut().find(|r| r.id == review_id) {
            match result {
                Ok(res) => {
                    r.status = "completed".to_string();
                    r.duration_secs = (res.duration_ms / 1000).max(1);
                    r.pass1_candidates = res.pass1_candidates as u32;
                    r.pass2_verified = res.pass2_verified as u32;
                    r.provider_used = res.provider_used;
                    r.title = Some(format!("PR #{}: Review analysis completed", pr));
                    r.summary = res.summary;
                    r.findings = res
                        .issues
                        .iter()
                        .map(|issue| Finding {
                            file: issue.file.clone(),
                            line: issue.line,
                            severity: issue.severity.clone(),
                            title: issue.title.clone(),
                            body: issue.comment.clone(),
                            snippet: issue.suggestion.clone(),
                            confidence: issue.confidence,
                            feedback: None,
                        })
                        .collect();
                    info!(
                        "Manual review completed for {} #{} (published: {}, findings: {})",
                        repo_clone,
                        pr,
                        publish,
                        r.findings.len()
                    );
                }
                Err(e) => {
                    r.status = "failed".to_string();
                    r.duration_secs = (duration_ms / 1000).max(1);
                    r.title = Some(format!("PR #{}: Review failed", pr));
                    r.summary = format!("Review pipeline failed: {}", e);
                    error!(
                        "Manual review failed for {} #{}: {:?}",
                        repo_clone, pr, e
                    );
                    if let Some(db) = &state_clone.db {
                        let _ = db
                            .record_review(
                                &repo_clone,
                                pr as i64,
                                &format!("PR #{}: Review failed", pr),
                                "unknown",
                                "failed",
                                "error",
                                0,
                                0,
                                0,
                                0.0,
                                duration_ms as i64,
                                &format!("Review pipeline failed: {}", e),
                                "[]",
                            )
                            .await;
                    }
                }
            }
        }
    });

    (
        StatusCode::ACCEPTED,
        Json(serde_json::json!({
            "status": "queued",
            "repo": repo,
            "pr": pr,
            "message": format!("Review pipeline started for {} #{}", repo, pr)
        })),
    )
}

/// Handles incoming forge webhook events (push, pull_request, comments).
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
        if action != "opened"
            && action != "synchronized"
            && action != "synchronize"
            && action != "reopened"
        {
            return (
                StatusCode::OK,
                format!(
                    "Ignored PR action '{}' (only opened/synchronize/reopen)",
                    action
                ),
            );
        }
        payload
            .number
            .or_else(|| payload.pull_request.as_ref().and_then(|pr| pr.number))
    } else if event == "issue_comment" || event == "pull_request_review_comment" {
        if action != "created" {
            return (
                StatusCode::OK,
                format!("Ignored comment action '{}' (only created)", action),
            );
        }

        // Only handle comments on pull requests
        if event == "issue_comment" {
            let is_pr = payload
                .issue
                .as_ref()
                .and_then(|i| i.pull_request.as_ref())
                .is_some();
            if !is_pr {
                return (
                    StatusCode::OK,
                    "Ignored comment on regular issue (not a pull request)".to_string(),
                );
            }
        }

        let body = payload
            .comment
            .as_ref()
            .and_then(|c| c.body.as_deref())
            .unwrap_or("");
        if !body.contains("@revue") && !body.contains("@nix-revue") {
            return (
                StatusCode::OK,
                "Ignored PR comment (did not mention @revue)".to_string(),
            );
        }

        payload
            .number
            .or_else(|| payload.pull_request.as_ref().and_then(|pr| pr.number))
            .or_else(|| payload.issue.as_ref().and_then(|i| i.number))
    } else {
        return (
            StatusCode::OK,
            format!(
                "Ignored event '{}' (only pull_request, issue_comment, and pull_request_review_comment are handled)",
                event
            ),
        );
    };

    let (pr_num, repo) = match (pr_index, repo_full_name) {
        (Some(num), Some(r)) => (num, r),
        _ => {
            warn!("Webhook missing PR number or repository name");
            return (
                StatusCode::BAD_REQUEST,
                "Missing PR number or repo name".to_string(),
            );
        }
    };

    info!(
        "Received {} webhook for {} #{} (action: {})",
        event, repo, pr_num, action
    );

    let config_path = state.config_path.clone();
    let repo_for_review = repo.clone();
    let state_clone = state.clone();

    // Spawn review asynchronously in background so webhook returns 200 immediately
    tokio::spawn(async move {
        let start = std::time::Instant::now();
        let db_opt = state_clone.db.as_ref();
        if let Err(e) = run_pr_review(
            pr_num,
            Some(&repo_for_review),
            false,
            config_path.as_deref(),
            db_opt,
        )
        .await
        {
            let duration_ms = start.elapsed().as_millis() as u64;
            error!(
                "Failed to execute review for {} #{}: {:?}",
                repo_for_review, pr_num, e
            );
            if let Some(db) = db_opt {
                let _ = db
                    .record_review(
                        &repo_for_review,
                        pr_num as i64,
                        &format!("PR #{}: Webhook review failed", pr_num),
                        "unknown",
                        "failed",
                        "error",
                        0,
                        0,
                        0,
                        0.0,
                        duration_ms as i64,
                        &format!("Webhook review execution failed: {}", e),
                        "[]",
                    )
                    .await;
            }
        }
    });

    (
        StatusCode::ACCEPTED,
        format!("Review queued for {} #{}", repo, pr_num),
    )
}

async fn add_cache_headers(req: Request, next: Next) -> Response {
    let uri = req.uri().path().to_string();
    let mut res = next.run(req).await;
    let headers = res.headers_mut();
    if uri.starts_with("/_app/immutable/") {
        headers.insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static("public, max-age=31536000, immutable"),
        );
    } else {
        headers.insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static("no-cache, no-store, must-revalidate"),
        );
        headers.insert(header::PRAGMA, HeaderValue::from_static("no-cache"));
        headers.insert(header::EXPIRES, HeaderValue::from_static("0"));
    }
    res
}

/// Starts the HTTP daemon serving the review API, web dashboard, and webhook receiver.
pub async fn run_daemon(
    listen: &str,
    config_path: Option<&str>,
    webhook_secret: Option<&str>,
    static_dir: Option<&str>,
    database_url: Option<&str>,
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

    let db_url = database_url
        .map(|s| s.to_string())
        .unwrap_or_else(crate::db::default_database_url);

    let db = match crate::db::DbPool::connect(&db_url).await {
        Ok(pool) => Some(pool),
        Err(e) => {
            warn!(
                "Running without persistent database (error: {}). Falling back to in-memory store.",
                e
            );
            None
        }
    };

    let state = Arc::new(ServerState {
        config_path: config_path.map(|s| s.to_string()),
        webhook_secret: resolved_secret,
        start_time: Instant::now(),
        db,
        reviews: Arc::new(RwLock::new(Vec::new())),
    });

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let api_router = Router::new()
        .route("/health", get(health_check))
        .route("/webhook", post(handle_webhook))
        .route("/api/status", get(get_status))
        .route("/api/reviews", get(get_reviews))
        .route("/api/trigger", post(trigger_review))
        .route(
            "/api/feedback",
            get(get_all_feedback).post(record_finding_feedback),
        )
        .route("/api/feedback/delete", post(delete_feedback_entry))
        .route("/api/skills", get(get_available_skills))
        .route("/api/rules/synthesize", post(handle_synthesize_rule))
        .layer(cors)
        .with_state(state.clone());

    // Resolve static files directory for SvelteKit web dashboard:
    // 1. Explicit CLI argument / env var
    // 2. ./web/build (local development)
    // 3. ../share/revue/web relative to executable (Nix store / standard install)
    // 4. /var/lib/revue/web (system directory)
    let resolved_static_dir = if let Some(dir) = static_dir {
        let p = PathBuf::from(dir);
        if p.exists() {
            Some(p)
        } else {
            None
        }
    } else {
        let candidate_rel = PathBuf::from("./web/build");
        let candidate_share = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|bin| bin.join("../share/revue/web")));
        let candidate_var = PathBuf::from("/var/lib/revue/web");

        if candidate_rel.exists() {
            Some(candidate_rel)
        } else if let Some(share) = candidate_share.filter(|p| p.exists()) {
            Some(share)
        } else if candidate_var.exists() {
            Some(candidate_var)
        } else {
            None
        }
    };

    let app = if let Some(static_path) = resolved_static_dir {
        info!("Serving SvelteKit web dashboard from {:?}", static_path);
        let fallback_page = static_path.join("200.html");
        let fallback_index = static_path.join("index.html");
        let fallback_file = if fallback_page.exists() {
            fallback_page
        } else {
            fallback_index
        };

        let serve_service = ServeDir::new(&static_path).fallback(ServeFile::new(fallback_file));

        Router::new()
            .merge(api_router)
            .fallback_service(serve_service)
            .layer(middleware::from_fn(add_cache_headers))
    } else {
        info!("No static dashboard directory found; serving API and status landing page");
        Router::new()
            .merge(api_router)
            .route(
                "/",
                get(|| async {
                    Html(
                        "<html><head><title>revue engine</title></head><body style='background:#0f172a;color:#f8fafc;font-family:sans-serif;padding:2rem;'><h1>revue code review engine</h1><p>Daemon running. Web dashboard build not located. API available at <code>/api/status</code>.</p></body></html>",
                    )
                }),
            )
            .layer(middleware::from_fn(add_cache_headers))
    };

    info!("revue daemon listening on http://{}", listen);
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
        headers.insert("x-gitea-signature", HeaderValue::from_str(&sig).unwrap());

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

    #[test]
    fn test_webhook_payload_pr_review_comment() {
        let json_data = r#"{
            "action": "created",
            "comment": { "body": "@revue please re-evaluate this suggestion" },
            "pull_request": { "number": 42 },
            "repository": { "full_name": "kleinbem/nix-config" }
        }"#;
        let payload: WebhookPayload = serde_json::from_str(json_data).unwrap();
        assert_eq!(payload.action.as_deref(), Some("created"));
        assert_eq!(payload.pull_request.unwrap().number, Some(42));
        assert!(payload.comment.unwrap().body.unwrap().contains("@revue"));
        assert_eq!(
            payload.repository.unwrap().full_name.as_deref(),
            Some("kleinbem/nix-config")
        );
    }

    #[test]
    fn test_finding_deserialization_round_trip() {
        let serialized_issues = r#"[
            {
                "file": "src/main.rs",
                "line": 42,
                "severity": "warning",
                "title": "Unchecked error return",
                "comment": "File write result is discarded",
                "suggestion": "std::fs::write(path, data)?;"
            }
        ]"#;

        let findings: Vec<Finding> = serde_json::from_str(serialized_issues).unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].body, "File write result is discarded");
        assert_eq!(
            findings[0].snippet.as_deref(),
            Some("std::fs::write(path, data)?;")
        );
    }
}
