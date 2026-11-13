use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool, SqlitePool};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tracing::info;

#[derive(Clone)]
pub enum DbPool {
    Sqlite(SqlitePool),
    Postgres(PgPool),
}

macro_rules! dispatch_pool {
    ($self:expr, $pool:ident => $body:expr) => {
        match $self {
            DbPool::Sqlite($pool) => $body,
            DbPool::Postgres($pool) => $body,
        }
    };
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct StoredReview {
    pub id: i64,
    pub repo: String,
    pub pr_number: i64,
    pub title: String,
    pub author: String,
    pub status: String,
    pub provider_used: String,
    pub pass1_candidates: i64,
    pub pass2_filtered: i64,
    pub verified_findings: i64,
    pub cost_saved_usd: f64,
    pub duration_ms: i64,
    pub summary: String,
    pub findings_json: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct AggregateMetrics {
    pub total_reviews: i64,
    pub pass1_candidates: i64,
    pub pass2_filtered: i64,
    pub verified_findings: i64,
    pub total_cost_saved: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct FindingFeedback {
    pub id: i64,
    pub repo: String,
    pub file_path: String,
    pub issue_title: String,
    pub feedback_type: String,
    pub reason: Option<String>,
    pub created_at: String,
}

/// Parameters for persisting an executed review record.
#[derive(Debug, Clone)]
pub struct NewReviewRecord<'a> {
    pub repo: &'a str,
    pub pr_number: i64,
    pub title: &'a str,
    pub author: &'a str,
    pub status: &'a str,
    pub provider_used: &'a str,
    pub pass1: i64,
    pub pass2: i64,
    pub verified: i64,
    pub cost_saved: f64,
    pub duration_ms: i64,
    pub summary: &'a str,
    pub findings_json: &'a str,
}

pub fn default_database_url() -> String {
    if let Ok(url) = std::env::var("DATABASE_URL") {
        return url;
    }

    let path = if let Ok(dir) = std::env::var("REVUE_DATA_DIR") {
        PathBuf::from(dir).join("revue.db")
    } else if Path::new("/var/lib/revue").is_dir() {
        PathBuf::from("/var/lib/revue/revue.db")
    } else {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        PathBuf::from(home).join(".local/share/revue/revue.db")
    };

    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    format!("sqlite://{}?mode=rwc", path.to_string_lossy())
}

impl DbPool {
    pub async fn connect(url: &str) -> Result<Self> {
        if url.starts_with("postgres://") || url.starts_with("postgresql://") {
            info!("Connecting to PostgreSQL database...");
            let pool = sqlx::postgres::PgPoolOptions::new()
                .max_connections(10)
                .connect(url)
                .await
                .context("Failed to connect to PostgreSQL database")?;
            Self::init_postgres(&pool).await?;
            info!("PostgreSQL database initialized successfully");
            Ok(DbPool::Postgres(pool))
        } else {
            info!("Connecting to embedded SQLite database ({url})...");
            let pool = sqlx::sqlite::SqlitePoolOptions::new()
                .max_connections(5)
                .connect(url)
                .await
                .context("Failed to connect to SQLite database")?;
            Self::init_sqlite(&pool).await?;
            info!("SQLite database initialized with WAL mode");
            Ok(DbPool::Sqlite(pool))
        }
    }

    async fn init_sqlite(pool: &SqlitePool) -> Result<()> {
        sqlx::query("PRAGMA journal_mode = WAL;")
            .execute(pool)
            .await?;
        sqlx::query("PRAGMA foreign_keys = ON;")
            .execute(pool)
            .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS reviews (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                repo TEXT NOT NULL,
                pr_number INTEGER NOT NULL,
                title TEXT NOT NULL,
                author TEXT NOT NULL,
                status TEXT NOT NULL,
                provider_used TEXT NOT NULL,
                pass1_candidates INTEGER NOT NULL DEFAULT 0,
                pass2_filtered INTEGER NOT NULL DEFAULT 0,
                verified_findings INTEGER NOT NULL DEFAULT 0,
                cost_saved_usd REAL NOT NULL DEFAULT 0.0,
                duration_ms INTEGER NOT NULL DEFAULT 0,
                summary TEXT NOT NULL DEFAULT '',
                findings_json TEXT NOT NULL DEFAULT '[]',
                created_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_reviews_repo_pr ON reviews(repo, pr_number);
            CREATE INDEX IF NOT EXISTS idx_reviews_created ON reviews(created_at DESC);

            CREATE TABLE IF NOT EXISTS findings_feedback (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                repo TEXT NOT NULL,
                file_path TEXT NOT NULL,
                issue_title TEXT NOT NULL,
                feedback_type TEXT NOT NULL,
                reason TEXT,
                created_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_feedback_repo ON findings_feedback(repo, feedback_type);
            "#,
        )
        .execute(pool)
        .await
        .context("Failed to initialize SQLite schema")?;

        // Backfill columns for databases created before `summary` and `findings_json` existed.
        let _ = sqlx::query("ALTER TABLE reviews ADD COLUMN summary TEXT NOT NULL DEFAULT '';")
            .execute(pool)
            .await;
        let _ =
            sqlx::query("ALTER TABLE reviews ADD COLUMN findings_json TEXT NOT NULL DEFAULT '[]';")
                .execute(pool)
                .await;

        Ok(())
    }

    async fn init_postgres(pool: &PgPool) -> Result<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS reviews (
                id BIGSERIAL PRIMARY KEY,
                repo TEXT NOT NULL,
                pr_number BIGINT NOT NULL,
                title TEXT NOT NULL,
                author TEXT NOT NULL,
                status TEXT NOT NULL,
                provider_used TEXT NOT NULL,
                pass1_candidates INTEGER NOT NULL DEFAULT 0,
                pass2_filtered INTEGER NOT NULL DEFAULT 0,
                verified_findings INTEGER NOT NULL DEFAULT 0,
                cost_saved_usd DOUBLE PRECISION NOT NULL DEFAULT 0.0,
                duration_ms BIGINT NOT NULL DEFAULT 0,
                summary TEXT NOT NULL DEFAULT '',
                findings_json TEXT NOT NULL DEFAULT '[]',
                created_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_reviews_repo_pr ON reviews(repo, pr_number);
            CREATE INDEX IF NOT EXISTS idx_reviews_created ON reviews(created_at DESC);

            CREATE TABLE IF NOT EXISTS findings_feedback (
                id BIGSERIAL PRIMARY KEY,
                repo TEXT NOT NULL,
                file_path TEXT NOT NULL,
                issue_title TEXT NOT NULL,
                feedback_type TEXT NOT NULL,
                reason TEXT,
                created_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_feedback_repo ON findings_feedback(repo, feedback_type);
            "#,
        )
        .execute(pool)
        .await
        .context("Failed to initialize PostgreSQL schema")?;

        sqlx::query(
            "ALTER TABLE reviews ADD COLUMN IF NOT EXISTS summary TEXT NOT NULL DEFAULT '';",
        )
        .execute(pool)
        .await
        .context("Failed to migrate reviews.summary column")?;
        sqlx::query(
            "ALTER TABLE reviews ADD COLUMN IF NOT EXISTS findings_json TEXT NOT NULL DEFAULT '[]';",
        )
        .execute(pool)
        .await
        .context("Failed to migrate reviews.findings_json column")?;

        Ok(())
    }

    /// Records an executed review run and returns its primary key.
    pub async fn record_review(&self, rec: &NewReviewRecord<'_>) -> Result<i64> {
        let now = Utc::now().to_rfc3339();
        let sql = r#"
            INSERT INTO reviews (repo, pr_number, title, author, status, provider_used,
                                 pass1_candidates, pass2_filtered, verified_findings,
                                 cost_saved_usd, duration_ms, summary, findings_json, created_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)
            RETURNING id
        "#;

        dispatch_pool!(self, pool => {
            let id: i64 = sqlx::query_scalar(sql)
                .bind(rec.repo)
                .bind(rec.pr_number)
                .bind(rec.title)
                .bind(rec.author)
                .bind(rec.status)
                .bind(rec.provider_used)
                .bind(rec.pass1)
                .bind(rec.pass2)
                .bind(rec.verified)
                .bind(rec.cost_saved)
                .bind(rec.duration_ms)
                .bind(rec.summary)
                .bind(rec.findings_json)
                .bind(&now)
                .fetch_one(pool)
                .await?;
            Ok(id)
        })
    }

    pub async fn get_metrics(&self) -> Result<AggregateMetrics> {
        let sql = r#"
            SELECT
                COUNT(*) AS total_reviews,
                CAST(COALESCE(SUM(pass1_candidates), 0) AS BIGINT) AS pass1_candidates,
                CAST(COALESCE(SUM(pass2_filtered), 0) AS BIGINT) AS pass2_filtered,
                CAST(COALESCE(SUM(verified_findings), 0) AS BIGINT) AS verified_findings,
                CAST(COALESCE(SUM(cost_saved_usd), 0.0) AS DOUBLE PRECISION) AS total_cost_saved
            FROM reviews
        "#;

        dispatch_pool!(self, pool => {
            Ok(sqlx::query_as::<_, AggregateMetrics>(sql).fetch_one(pool).await?)
        })
    }

    /// Lists the most recently recorded reviews up to `limit`.
    pub async fn list_recent_reviews(&self, limit: i64) -> Result<Vec<StoredReview>> {
        let sql = r#"
            SELECT id, repo, pr_number, title, author, status, provider_used,
                   pass1_candidates, pass2_filtered, verified_findings,
                   cost_saved_usd, duration_ms, summary, findings_json, created_at
            FROM reviews
            ORDER BY id DESC
            LIMIT $1
        "#;

        dispatch_pool!(self, pool => {
            Ok(sqlx::query_as::<_, StoredReview>(sql)
                .bind(limit)
                .fetch_all(pool)
                .await?)
        })
    }

    pub async fn record_feedback(
        &self,
        repo: &str,
        file_path: &str,
        issue_title: &str,
        feedback_type: &str,
        reason: Option<&str>,
    ) -> Result<i64> {
        let now = Utc::now().to_rfc3339();
        let sql = r#"
            INSERT INTO findings_feedback (repo, file_path, issue_title, feedback_type, reason, created_at)
            VALUES ($1, $2, $3, $4, $5, $6)
            RETURNING id
        "#;

        dispatch_pool!(self, pool => {
            let id: i64 = sqlx::query_scalar(sql)
                .bind(repo)
                .bind(file_path)
                .bind(issue_title)
                .bind(feedback_type)
                .bind(reason)
                .bind(&now)
                .fetch_one(pool)
                .await?;
            Ok(id)
        })
    }

    pub async fn get_false_positives(
        &self,
        repo: &str,
        limit: i64,
    ) -> Result<Vec<FindingFeedback>> {
        let sql = r#"
            SELECT id, repo, file_path, issue_title, feedback_type, reason, created_at
            FROM findings_feedback
            WHERE repo = $1 AND feedback_type = 'false_positive'
            ORDER BY id DESC
            LIMIT $2
        "#;

        dispatch_pool!(self, pool => {
            Ok(sqlx::query_as::<_, FindingFeedback>(sql)
                .bind(repo)
                .bind(limit)
                .fetch_all(pool)
                .await?)
        })
    }

    pub async fn list_all_feedback(&self, limit: i64) -> Result<Vec<FindingFeedback>> {
        let sql = r#"
            SELECT id, repo, file_path, issue_title, feedback_type, reason, created_at
            FROM findings_feedback
            ORDER BY id DESC
            LIMIT $1
        "#;

        dispatch_pool!(self, pool => {
            Ok(sqlx::query_as::<_, FindingFeedback>(sql)
                .bind(limit)
                .fetch_all(pool)
                .await?)
        })
    }

    /// Maps `(repo, file_path, issue_title) -> feedback_type` for `repos` so dashboard reloads retain dismissals.
    pub async fn feedback_lookup_for_repos(
        &self,
        repos: &[String],
    ) -> Result<HashMap<(String, String, String), String>> {
        let mut seen_repos = std::collections::HashSet::new();
        let mut map = HashMap::new();
        let sql = r#"
            SELECT id, repo, file_path, issue_title, feedback_type, reason, created_at
            FROM findings_feedback
            WHERE repo = $1
            ORDER BY id ASC
        "#;

        for repo in repos {
            if !seen_repos.insert(repo.as_str()) {
                continue;
            }
            let entries: Vec<FindingFeedback> = dispatch_pool!(self, pool => {
                sqlx::query_as::<_, FindingFeedback>(sql)
                    .bind(repo)
                    .fetch_all(pool)
                    .await?
            });
            for entry in entries {
                map.insert(
                    (entry.repo, entry.file_path, entry.issue_title),
                    entry.feedback_type,
                );
            }
        }
        Ok(map)
    }

    pub async fn delete_feedback(&self, id: i64) -> Result<bool> {
        let sql = "DELETE FROM findings_feedback WHERE id = $1";
        dispatch_pool!(self, pool => {
            let res = sqlx::query(sql).bind(id).execute(pool).await?;
            Ok(res.rows_affected() > 0)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_sqlite_in_memory_lifecycle() {
        let db = DbPool::connect("sqlite::memory:")
            .await
            .expect("Should connect to in-memory sqlite");

        let id = db
            .record_review(&NewReviewRecord {
                repo: "kleinbem/nix-config",
                pr_number: 506,
                title: "feat: update revue web",
                author: "martin",
                status: "completed",
                provider_used: "Mesh-LLM (Fleet P2P GPU)",
                pass1: 12,
                pass2: 8,
                verified: 4,
                cost_saved: 75.50,
                duration_ms: 1420,
                summary: "Automated review verified 4 defects.",
                findings_json: "[]",
            })
            .await
            .expect("Should record review");

        assert!(id > 0);

        let metrics = db.get_metrics().await.expect("Should fetch metrics");
        assert_eq!(metrics.total_reviews, 1);
        assert_eq!(metrics.pass1_candidates, 12);
        assert_eq!(metrics.pass2_filtered, 8);
        assert_eq!(metrics.verified_findings, 4);
        assert!((metrics.total_cost_saved - 75.50).abs() < f64::EPSILON);

        let recent = db
            .list_recent_reviews(10)
            .await
            .expect("Should list reviews");
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].repo, "kleinbem/nix-config");
        assert_eq!(recent[0].summary, "Automated review verified 4 defects.");
        assert_eq!(recent[0].findings_json, "[]");
    }

    #[tokio::test]
    async fn test_sqlite_feedback_lifecycle() {
        let db = DbPool::connect("sqlite::memory:")
            .await
            .expect("Should connect to in-memory sqlite");

        let fid = db
            .record_feedback(
                "kleinbem/nix-config",
                "hosts/nixos-nvme/default.nix",
                "ProtectHome read-only warning",
                "false_positive",
                Some("Intentionally read-only in nspawn containers"),
            )
            .await
            .expect("Should record feedback");

        assert!(fid > 0);

        let fps = db
            .get_false_positives("kleinbem/nix-config", 5)
            .await
            .expect("Should fetch false positives");

        assert_eq!(fps.len(), 1);
        assert_eq!(fps[0].issue_title, "ProtectHome read-only warning");
        assert_eq!(fps[0].feedback_type, "false_positive");
        assert_eq!(
            fps[0].reason.as_deref(),
            Some("Intentionally read-only in nspawn containers")
        );

        let lookup = db
            .feedback_lookup_for_repos(&["kleinbem/nix-config".to_string()])
            .await
            .unwrap();
        assert_eq!(
            lookup
                .get(&(
                    "kleinbem/nix-config".to_string(),
                    "hosts/nixos-nvme/default.nix".to_string(),
                    "ProtectHome read-only warning".to_string(),
                ))
                .map(String::as_str),
            Some("false_positive")
        );
    }

    #[tokio::test]
    async fn test_sqlite_legacy_reviews_migration() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::query(
            r#"
            CREATE TABLE reviews (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                repo TEXT NOT NULL,
                pr_number INTEGER NOT NULL,
                title TEXT NOT NULL,
                author TEXT NOT NULL,
                status TEXT NOT NULL,
                provider_used TEXT NOT NULL,
                pass1_candidates INTEGER NOT NULL DEFAULT 0,
                pass2_filtered INTEGER NOT NULL DEFAULT 0,
                verified_findings INTEGER NOT NULL DEFAULT 0,
                cost_saved_usd REAL NOT NULL DEFAULT 0.0,
                duration_ms INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL
            );
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();

        DbPool::init_sqlite(&pool).await.unwrap();
        let db = DbPool::Sqlite(pool);
        let id = db
            .record_review(&NewReviewRecord {
                repo: "owner/legacy",
                pr_number: 1,
                title: "legacy db test",
                author: "dev",
                status: "completed",
                provider_used: "mock",
                pass1: 1,
                pass2: 1,
                verified: 1,
                cost_saved: 0.0,
                duration_ms: 10,
                summary: "migrated",
                findings_json: "[]",
            })
            .await
            .unwrap();
        assert!(id > 0);
        let recent = db.list_recent_reviews(5).await.unwrap();
        assert_eq!(recent[0].summary, "migrated");
    }
}
