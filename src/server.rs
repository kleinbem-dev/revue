use crate::config::RevueConfig;
use crate::reviewer::run_pr_review;
use anyhow::Result;
use axum::{
    Router,
    body::Bytes,
    extract::{ConnectInfo, Request, State},
    http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode, header},
    middleware::{self, Next},
    response::{Html, IntoResponse, Json, Response},
    routing::{get, post},
};
use hmac::{Hmac, KeyInit, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;
use subtle::ConstantTimeEq;
use tokio::sync::RwLock;
use tower_http::cors::CorsLayer;
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
pub struct ServerState {
    pub config_path: Option<String>,
    pub webhook_secret: Option<String>,
    pub api_key: Option<String>,
    pub insecure_webhooks: bool,
    pub start_time: Instant,
    pub db: Option<crate::db::DbPool>,
    pub reviews: Arc<RwLock<Vec<ReviewRecord>>>,
    pub manual_review_seq: Arc<AtomicU64>,
}

/// Enforce API key authentication on mutating endpoints.
/// If `api_key` is set in state, require `Authorization: Bearer <key>` or `X-Revue-Key: <key>`.
/// If `api_key` is not configured, permit loopback requests only (127.0.0.1, ::1); reject remote callers.
pub fn check_api_auth(
    headers: &HeaderMap,
    state: &ServerState,
    client_ip: Option<std::net::IpAddr>,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    if let Some(configured_key) = &state.api_key {
        let auth_header = headers
            .get(header::AUTHORIZATION)
            .and_then(|h| h.to_str().ok())
            .and_then(|s| {
                s.strip_prefix("Bearer ")
                    .or_else(|| s.strip_prefix("bearer "))
            })
            .map(|s| s.trim());

        let custom_header = headers
            .get("x-revue-key")
            .and_then(|h| h.to_str().ok())
            .map(|s| s.trim());

        let provided = auth_header.or(custom_header);
        match provided {
            Some(token)
                if token.len() == configured_key.len()
                    && bool::from(token.as_bytes().ct_eq(configured_key.as_bytes())) =>
            {
                Ok(())
            }
            _ => Err((
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({
                    "error": "Unauthorized: valid API key required in Authorization: Bearer <key> or X-Revue-Key header"
                })),
            )),
        }
    } else {
        // No explicit API key: allow loopback clients (localhost / 127.0.0.1 / ::1).
        // A missing client IP fails closed to prevent unintended exposure behind misconfigured proxies.
        let is_loopback = client_ip.map(|ip| ip.is_loopback()).unwrap_or(false);
        if is_loopback {
            Ok(())
        } else {
            Err((
                StatusCode::FORBIDDEN,
                Json(serde_json::json!({
                    "error": "Forbidden: REVUE_API_KEY must be configured for non-loopback access"
                })),
            ))
        }
    }
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

    let cfg = crate::config::RevueConfig::find_and_load(state.config_path.as_deref())
        .unwrap_or_else(|e| {
            warn!("Failed to load config for provider status: {}", e);
            Default::default()
        });
    let mut providers = Vec::new();
    for name in &cfg.providers.cascade {
        match name.as_str() {
            "antigravity" | "gemini-cli" | "antigravity-cli" => {
                providers.push(ProviderStatus {
                    name: "Antigravity CLI".to_string(),
                    r#type: "cli".to_string(),
                    model: "gemini-2.5-pro / flash".to_string(),
                    status: "configured".to_string(),
                    cost_per_token: "$0 (Subscription)".to_string(),
                    latency_avg: "—".to_string(),
                });
            }
            "mesh-llm" | "mesh" => {
                providers.push(ProviderStatus {
                    name: "Mesh-LLM (Fleet P2P)".to_string(),
                    r#type: "mesh-gpu".to_string(),
                    model: cfg.providers.mesh_llm_model.clone(),
                    status: "configured".to_string(),
                    cost_per_token: "$0 (P2P GPU)".to_string(),
                    latency_avg: "—".to_string(),
                });
            }
            "gemini" | "gemini-api" => {
                let has_key = std::env::var("GEMINI_API_KEY").is_ok()
                    || std::env::var("GOOGLE_API_KEY").is_ok();
                providers.push(ProviderStatus {
                    name: "Gemini API".to_string(),
                    r#type: "api".to_string(),
                    model: cfg.providers.gemini_model.clone(),
                    status: if has_key {
                        "configured".to_string()
                    } else {
                        "unconfigured".to_string()
                    },
                    cost_per_token: "Pay-as-you-go".to_string(),
                    latency_avg: "—".to_string(),
                });
            }
            "ollama" => {
                providers.push(ProviderStatus {
                    name: "Local Ollama".to_string(),
                    r#type: "local".to_string(),
                    model: cfg.providers.ollama_model.clone(),
                    status: "configured".to_string(),
                    cost_per_token: "$0 (Air-gapped)".to_string(),
                    latency_avg: "—".to_string(),
                });
            }
            "claude" | "claude-cli" => {
                providers.push(ProviderStatus {
                    name: "Claude CLI".to_string(),
                    r#type: "cli".to_string(),
                    model: "claude-3-7-sonnet".to_string(),
                    status: "configured".to_string(),
                    cost_per_token: "Shielded".to_string(),
                    latency_avg: "—".to_string(),
                });
            }
            other => {
                providers.push(ProviderStatus {
                    name: other.to_string(),
                    r#type: "custom".to_string(),
                    model: "custom".to_string(),
                    status: "configured".to_string(),
                    cost_per_token: "—".to_string(),
                    latency_avg: "—".to_string(),
                });
            }
        }
    }

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
    if let Some(db) = &state.db
        && let Ok(db_reviews) = db.list_recent_reviews(50).await
    {
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

    // 3. Fall back to all in-memory state if no records were found
    if records.is_empty() {
        let reviews = state.reviews.read().await;
        return Json(reviews.clone());
    }

    Json(records)
}

async fn record_finding_feedback(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(req): Json<FeedbackRequest>,
) -> Response {
    let client_ip = Some(addr.ip());
    if let Err((status, msg)) = check_api_auth(&headers, &state, client_ip) {
        return (status, msg).into_response();
    }

    info!(
        "Received feedback '{}' for {} [{}: {}]",
        req.feedback_type, req.repo, req.file_path, req.issue_title
    );

    // 1. Record into persistent DB if available
    if let Some(db) = &state.db
        && let Err(e) = db
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
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "status": "error",
                "message": format!("Database error: {}", e)
            })),
        )
            .into_response();
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
        .into_response()
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
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(req): Json<DeleteFeedbackRequest>,
) -> Response {
    let client_ip = Some(addr.ip());
    if let Err((status, msg)) = check_api_auth(&headers, &state, client_ip) {
        return (status, msg).into_response();
    }

    if let Some(db) = &state.db {
        match db.delete_feedback(req.id).await {
            Ok(true) => (
                StatusCode::OK,
                Json(serde_json::json!({ "status": "success", "message": "Feedback deleted" })),
            )
                .into_response(),
            Ok(false) => (
                StatusCode::NOT_FOUND,
                Json(
                    serde_json::json!({ "status": "not_found", "message": "Feedback ID not found" }),
                ),
            )
                .into_response(),
            Err(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "status": "error", "message": e.to_string() })),
            )
                .into_response(),
        }
    } else {
        (
            StatusCode::OK,
            Json(
                serde_json::json!({ "status": "success", "message": "No persistent db configured" }),
            ),
        )
            .into_response()
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
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(req): Json<crate::synthesizer::RuleSynthesizeRequest>,
) -> Response {
    let client_ip = Some(addr.ip());
    if let Err((status, msg)) = check_api_auth(&headers, &state, client_ip) {
        return (status, msg).into_response();
    }

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
        )
            .into_response();
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
        )
            .into_response();
    }

    let config =
        crate::config::RevueConfig::find_and_load(state.config_path.as_deref()).unwrap_or_default();
    let chain = crate::reviewer::build_provider_chain(&config);
    let resp = crate::synthesizer::synthesize_rule(req, &chain).await;
    if resp.success {
        (StatusCode::OK, Json(resp)).into_response()
    } else {
        (StatusCode::UNPROCESSABLE_ENTITY, Json(resp)).into_response()
    }
}

/// Triggers an asynchronous code review for the specified pull request.
async fn trigger_review(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(req): Json<TriggerRequest>,
) -> Response {
    let client_ip = Some(addr.ip());
    if let Err((status, msg)) = check_api_auth(&headers, &state, client_ip) {
        return (status, msg).into_response();
    }

    let repo = req.repo.clone();
    let pr = req.pr;

    let publish = req.publish.unwrap_or(false);
    info!(
        "Received manual review trigger for {} #{} (publish: {})",
        repo, pr, publish
    );

    let seq = state.manual_review_seq.fetch_add(1, Ordering::Relaxed);
    let clean_repo = repo.replace('/', "-");
    let review_id = format!("rev-manual-{}-{}-{}", clean_repo, pr, seq);
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
        reviews.retain(|r| !(r.pr_number == pr && r.repo == repo && r.id == review_id));
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
                        let rec = crate::db::NewReviewRecord {
                            repo: &repo_clone,
                            pr_number: pr as i64,
                            title: &format!("PR #{}: Review failed", pr),
                            author: "unknown",
                            status: "failed",
                            provider_used: "error",
                            pass1: 0,
                            pass2: 0,
                            verified: 0,
                            cost_saved: 0.0,
                            duration_ms: duration_ms as i64,
                            summary: &format!("Review pipeline failed: {}", e),
                            findings_json: "[]",
                        };
                        let _ = db.record_review(&rec).await;
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
        .into_response()
}

/// Handles incoming forge webhook events (push, pull_request, comments).
async fn handle_webhook(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    // 1. Verify HMAC signature if webhook_secret is configured; fail closed otherwise
    match &state.webhook_secret {
        Some(secret) if !secret.is_empty() => {
            if !verify_signature(secret, &headers, &body) {
                return (
                    StatusCode::UNAUTHORIZED,
                    "Webhook HMAC signature verification failed\n".to_string(),
                );
            }
        }
        _ => {
            if !state.insecure_webhooks {
                return (
                    StatusCode::UNAUTHORIZED,
                    "Webhook secret is not configured; refusing unauthenticated webhook. Set REVUE_WEBHOOK_SECRET or pass --insecure-webhooks to allow.\n".to_string(),
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
                let rec = crate::db::NewReviewRecord {
                    repo: &repo_for_review,
                    pr_number: pr_num as i64,
                    title: &format!("PR #{}: Webhook review failed", pr_num),
                    author: "unknown",
                    status: "failed",
                    provider_used: "error",
                    pass1: 0,
                    pass2: 0,
                    verified: 0,
                    cost_saved: 0.0,
                    duration_ms: duration_ms as i64,
                    summary: &format!("Webhook review execution failed: {}", e),
                    findings_json: "[]",
                };
                let _ = db.record_review(&rec).await;
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
    api_key: Option<&str>,
    insecure_webhooks: bool,
) -> Result<()> {
    let loaded_cfg = RevueConfig::find_and_load(config_path).ok();
    let non_blank = |s: &str| !s.trim().is_empty();

    // Determine webhook secret priority with non-blank filtering per source
    let resolved_secret = webhook_secret
        .filter(|s| non_blank(s))
        .map(str::to_string)
        .or_else(|| {
            loaded_cfg
                .as_ref()
                .and_then(|c| c.forge.webhook_secret.clone())
                .filter(|s| non_blank(s))
        })
        .or_else(|| {
            std::env::var("REVUE_WEBHOOK_SECRET")
                .ok()
                .filter(|s| non_blank(s))
        });

    let resolved_insecure = insecure_webhooks
        || loaded_cfg
            .as_ref()
            .map(|c| c.server.insecure_webhooks)
            .unwrap_or(false);

    if resolved_secret.is_some() {
        info!("Webhook HMAC-SHA256 signature verification is ENABLED");
    } else if resolved_insecure {
        warn!("Webhook signature verification is DISABLED (--insecure-webhooks enabled). Insecure mode!");
    } else {
        info!("Webhook secret is not configured; incoming webhooks will fail closed (401 Unauthorized). Set REVUE_WEBHOOK_SECRET or pass --insecure-webhooks to allow.");
    }

    // Determine API key priority with non-blank filtering per source
    let resolved_api_key = api_key
        .filter(|k| non_blank(k))
        .map(str::to_string)
        .or_else(|| {
            loaded_cfg
                .as_ref()
                .and_then(|c| c.server.api_key.clone())
                .filter(|k| non_blank(k))
        })
        .or_else(|| std::env::var("REVUE_API_KEY").ok().filter(|k| non_blank(k)));

    if resolved_api_key.is_some() {
        info!("API authentication is ENABLED (valid API key required for /api mutating operations)");
    } else {
        info!("API key is not configured; /api mutating operations restricted to loopback (127.0.0.1 / ::1) clients only.");
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
        api_key: resolved_api_key,
        insecure_webhooks: resolved_insecure,
        start_time: Instant::now(),
        db,
        reviews: Arc::new(RwLock::new(Vec::new())),
        manual_review_seq: Arc::new(AtomicU64::new(1)),
    });

    let mut allowed_origins: Vec<HeaderValue> = Vec::new();
    if let Some(cfg) = &loaded_cfg {
        for origin in &cfg.server.cors_origins {
            if let Ok(val) = origin.parse::<HeaderValue>() {
                allowed_origins.push(val);
            }
        }
    }
    // Always permit local development / loopback origins for web UI
    for origin in &[
        "http://localhost:8383",
        "http://127.0.0.1:8383",
        "http://localhost:5173",
        "http://127.0.0.1:5173",
    ] {
        if let Ok(val) = origin.parse::<HeaderValue>()
            && !allowed_origins.contains(&val)
        {
            allowed_origins.push(val);
        }
    }

    let cors = CorsLayer::new()
        .allow_origin(allowed_origins)
        .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
        .allow_headers([
            header::CONTENT_TYPE,
            header::AUTHORIZATION,
            HeaderName::from_static("x-revue-key"),
            HeaderName::from_static("x-forgejo-event"),
            HeaderName::from_static("x-gitea-event"),
            HeaderName::from_static("x-github-event"),
            HeaderName::from_static("x-hub-signature-256"),
            HeaderName::from_static("x-forgejo-signature"),
            HeaderName::from_static("x-gitea-signature"),
        ]);

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
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;

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

    #[test]
    fn test_api_auth_with_configured_key() {
        let state = ServerState {
            config_path: None,
            webhook_secret: None,
            api_key: Some("secret-token-xyz".to_string()),
            insecure_webhooks: false,
            start_time: Instant::now(),
            db: None,
            reviews: Arc::new(RwLock::new(Vec::new())),
            manual_review_seq: Arc::new(AtomicU64::new(0)),
        };

        // Missing header -> 401
        let empty_headers = HeaderMap::new();
        assert!(check_api_auth(&empty_headers, &state, None).is_err());

        // Bearer token -> 200
        let mut bearer_headers = HeaderMap::new();
        bearer_headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer secret-token-xyz"),
        );
        assert!(check_api_auth(&bearer_headers, &state, None).is_ok());

        // X-Revue-Key header -> 200
        let mut custom_headers = HeaderMap::new();
        custom_headers.insert("x-revue-key", HeaderValue::from_static("secret-token-xyz"));
        assert!(check_api_auth(&custom_headers, &state, None).is_ok());

        // Invalid key -> 401
        let mut wrong_headers = HeaderMap::new();
        wrong_headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer bad-token"),
        );
        assert!(check_api_auth(&wrong_headers, &state, None).is_err());
    }

    #[test]
    fn test_api_auth_loopback_restriction() {
        let state = ServerState {
            config_path: None,
            webhook_secret: None,
            api_key: None,
            insecure_webhooks: false,
            start_time: Instant::now(),
            db: None,
            reviews: Arc::new(RwLock::new(Vec::new())),
            manual_review_seq: Arc::new(AtomicU64::new(0)),
        };

        let headers = HeaderMap::new();

        // Loopback IPv4 -> Ok
        let loopback_v4: std::net::IpAddr = "127.0.0.1".parse().unwrap();
        assert!(check_api_auth(&headers, &state, Some(loopback_v4)).is_ok());

        // Loopback IPv6 -> Ok
        let loopback_v6: std::net::IpAddr = "::1".parse().unwrap();
        assert!(check_api_auth(&headers, &state, Some(loopback_v6)).is_ok());

        // Missing client IP without API key configured -> 403 Forbidden (fail closed)
        assert_eq!(
            check_api_auth(&headers, &state, None).unwrap_err().0,
            StatusCode::FORBIDDEN
        );

        // Remote IP without API key configured -> 403 Forbidden
        let remote_ip: std::net::IpAddr = "192.168.1.100".parse().unwrap();
        let res = check_api_auth(&headers, &state, Some(remote_ip));
        assert!(res.is_err());
        assert_eq!(res.unwrap_err().0, StatusCode::FORBIDDEN);
    }
}
