use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool, PgPool};
use std::path::{Path, PathBuf};
use tracing::info;

#[derive(Clone)]
pub enum DbPool {
    Sqlite(SqlitePool),
    Postgres(PgPool),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregateMetrics {
    pub total_reviews: i64,
    pub pass1_candidates: i64,
    pub pass2_filtered: i64,
    pub verified_findings: i64,
    pub total_cost_saved: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FindingFeedback {
    pub id: i64,
    pub repo: String,
    pub file_path: String,
    pub issue_title: String,
    pub feedback_type: String,
    pub reason: Option<String>,
    pub created_at: String,
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
        sqlx::query("PRAGMA journal_mode = WAL;").execute(pool).await?;
        sqlx::query("PRAGMA foreign_keys = ON;").execute(pool).await?;

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
            "#
        )
        .execute(pool)
        .await
        .context("Failed to initialize SQLite schema")?;

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
            "#
        )
        .execute(pool)
        .await
        .context("Failed to initialize PostgreSQL schema")?;

        Ok(())
    }

    pub async fn record_review(
        &self,
        repo: &str,
        pr_number: i64,
        title: &str,
        author: &str,
        status: &str,
        provider_used: &str,
        pass1: i64,
        pass2: i64,
        verified: i64,
        cost_saved: f64,
        duration_ms: i64,
    ) -> Result<i64> {
        let now = Utc::now().to_rfc3339();

        match self {
            DbPool::Sqlite(pool) => {
                let res = sqlx::query(
                    r#"
                    INSERT INTO reviews (repo, pr_number, title, author, status, provider_used,
                                         pass1_candidates, pass2_filtered, verified_findings,
                                         cost_saved_usd, duration_ms, created_at)
                    VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                    "#
                )
                .bind(repo)
                .bind(pr_number)
                .bind(title)
                .bind(author)
                .bind(status)
                .bind(provider_used)
                .bind(pass1)
                .bind(pass2)
                .bind(verified)
                .bind(cost_saved)
                .bind(duration_ms)
                .bind(now)
                .execute(pool)
                .await?;

                Ok(res.last_insert_rowid())
            }
            DbPool::Postgres(pool) => {
                let row = sqlx::query(
                    r#"
                    INSERT INTO reviews (repo, pr_number, title, author, status, provider_used,
                                         pass1_candidates, pass2_filtered, verified_findings,
                                         cost_saved_usd, duration_ms, created_at)
                    VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
                    RETURNING id
                    "#
                )
                .bind(repo)
                .bind(pr_number)
                .bind(title)
                .bind(author)
                .bind(status)
                .bind(provider_used)
                .bind(pass1)
                .bind(pass2)
                .bind(verified)
                .bind(cost_saved)
                .bind(duration_ms)
                .bind(now)
                .fetch_one(pool)
                .await?;

                Ok(row.get("id"))
            }
        }
    }

    pub async fn get_metrics(&self) -> Result<AggregateMetrics> {
        match self {
            DbPool::Sqlite(pool) => {
                let row = sqlx::query(
                    r#"
                    SELECT
                        COUNT(*) as total_reviews,
                        COALESCE(SUM(pass1_candidates), 0) as pass1_candidates,
                        COALESCE(SUM(pass2_filtered), 0) as pass2_filtered,
                        COALESCE(SUM(verified_findings), 0) as verified_findings,
                        COALESCE(SUM(cost_saved_usd), 0.0) as total_cost_saved
                    FROM reviews
                    "#
                )
                .fetch_one(pool)
                .await?;

                Ok(AggregateMetrics {
                    total_reviews: row.get("total_reviews"),
                    pass1_candidates: row.get("pass1_candidates"),
                    pass2_filtered: row.get("pass2_filtered"),
                    verified_findings: row.get("verified_findings"),
                    total_cost_saved: row.get("total_cost_saved"),
                })
            }
            DbPool::Postgres(pool) => {
                let row = sqlx::query(
                    r#"
                    SELECT
                        COUNT(*) as total_reviews,
                        COALESCE(SUM(pass1_candidates), 0) as pass1_candidates,
                        COALESCE(SUM(pass2_filtered), 0) as pass2_filtered,
                        COALESCE(SUM(verified_findings), 0) as verified_findings,
                        COALESCE(SUM(cost_saved_usd), 0.0) as total_cost_saved
                    FROM reviews
                    "#
                )
                .fetch_one(pool)
                .await?;

                Ok(AggregateMetrics {
                    total_reviews: row.get("total_reviews"),
                    pass1_candidates: row.get("pass1_candidates"),
                    pass2_filtered: row.get("pass2_filtered"),
                    verified_findings: row.get("verified_findings"),
                    total_cost_saved: row.get("total_cost_saved"),
                })
            }
        }
    }

    pub async fn list_recent_reviews(&self, limit: i64) -> Result<Vec<StoredReview>> {
        let mut reviews = Vec::new();

        match self {
            DbPool::Sqlite(pool) => {
                let rows = sqlx::query(
                    r#"
                    SELECT id, repo, pr_number, title, author, status, provider_used,
                           pass1_candidates, pass2_filtered, verified_findings,
                           cost_saved_usd, duration_ms, created_at
                    FROM reviews
                    ORDER BY id DESC
                    LIMIT ?
                    "#
                )
                .bind(limit)
                .fetch_all(pool)
                .await?;

                for r in rows {
                    reviews.push(StoredReview {
                        id: r.get("id"),
                        repo: r.get("repo"),
                        pr_number: r.get("pr_number"),
                        title: r.get("title"),
                        author: r.get("author"),
                        status: r.get("status"),
                        provider_used: r.get("provider_used"),
                        pass1_candidates: r.get("pass1_candidates"),
                        pass2_filtered: r.get("pass2_filtered"),
                        verified_findings: r.get("verified_findings"),
                        cost_saved_usd: r.get("cost_saved_usd"),
                        duration_ms: r.get("duration_ms"),
                        created_at: r.get("created_at"),
                    });
                }
            }
            DbPool::Postgres(pool) => {
                let rows = sqlx::query(
                    r#"
                    SELECT id, repo, pr_number, title, author, status, provider_used,
                           pass1_candidates, pass2_filtered, verified_findings,
                           cost_saved_usd, duration_ms, created_at
                    FROM reviews
                    ORDER BY id DESC
                    LIMIT $1
                    "#
                )
                .bind(limit)
                .fetch_all(pool)
                .await?;

                for r in rows {
                    reviews.push(StoredReview {
                        id: r.get("id"),
                        repo: r.get("repo"),
                        pr_number: r.get("pr_number"),
                        title: r.get("title"),
                        author: r.get("author"),
                        status: r.get("status"),
                        provider_used: r.get("provider_used"),
                        pass1_candidates: r.get("pass1_candidates"),
                        pass2_filtered: r.get("pass2_filtered"),
                        verified_findings: r.get("verified_findings"),
                        cost_saved_usd: r.get("cost_saved_usd"),
                        duration_ms: r.get("duration_ms"),
                        created_at: r.get("created_at"),
                    });
                }
            }
        }

        Ok(reviews)
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

        match self {
            DbPool::Sqlite(pool) => {
                let res = sqlx::query(
                    r#"
                    INSERT INTO findings_feedback (repo, file_path, issue_title, feedback_type, reason, created_at)
                    VALUES (?, ?, ?, ?, ?, ?)
                    "#
                )
                .bind(repo)
                .bind(file_path)
                .bind(issue_title)
                .bind(feedback_type)
                .bind(reason)
                .bind(now)
                .execute(pool)
                .await?;

                Ok(res.last_insert_rowid())
            }
            DbPool::Postgres(pool) => {
                let row = sqlx::query(
                    r#"
                    INSERT INTO findings_feedback (repo, file_path, issue_title, feedback_type, reason, created_at)
                    VALUES ($1, $2, $3, $4, $5, $6)
                    RETURNING id
                    "#
                )
                .bind(repo)
                .bind(file_path)
                .bind(issue_title)
                .bind(feedback_type)
                .bind(reason)
                .bind(now)
                .fetch_one(pool)
                .await?;

                Ok(row.get(0))
            }
        }
    }

    pub async fn get_false_positives(&self, repo: &str, limit: i64) -> Result<Vec<FindingFeedback>> {
        match self {
            DbPool::Sqlite(pool) => {
                let rows = sqlx::query(
                    r#"
                    SELECT id, repo, file_path, issue_title, feedback_type, reason, created_at
                    FROM findings_feedback
                    WHERE repo = ? AND feedback_type = 'false_positive'
                    ORDER BY id DESC
                    LIMIT ?
                    "#
                )
                .bind(repo)
                .bind(limit)
                .fetch_all(pool)
                .await?;

                let feedback = rows
                    .into_iter()
                    .map(|r| FindingFeedback {
                        id: r.get(0),
                        repo: r.get(1),
                        file_path: r.get(2),
                        issue_title: r.get(3),
                        feedback_type: r.get(4),
                        reason: r.get(5),
                        created_at: r.get(6),
                    })
                    .collect();

                Ok(feedback)
            }
            DbPool::Postgres(pool) => {
                let rows = sqlx::query(
                    r#"
                    SELECT id, repo, file_path, issue_title, feedback_type, reason, created_at
                    FROM findings_feedback
                    WHERE repo = $1 AND feedback_type = 'false_positive'
                    ORDER BY id DESC
                    LIMIT $2
                    "#
                )
                .bind(repo)
                .bind(limit)
                .fetch_all(pool)
                .await?;

                let feedback = rows
                    .into_iter()
                    .map(|r| FindingFeedback {
                        id: r.get(0),
                        repo: r.get(1),
                        file_path: r.get(2),
                        issue_title: r.get(3),
                        feedback_type: r.get(4),
                        reason: r.get(5),
                        created_at: r.get(6),
                    })
                    .collect();

                Ok(feedback)
            }
        }
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
            .record_review(
                "kleinbem/nix-config",
                506,
                "feat: update revue web",
                "martin",
                "completed",
                "Mesh-LLM (Fleet P2P GPU)",
                12,
                8,
                4,
                75.50,
                1420,
            )
            .await
            .expect("Should record review");

        assert!(id > 0);

        let metrics = db.get_metrics().await.expect("Should fetch metrics");
        assert_eq!(metrics.total_reviews, 1);
        assert_eq!(metrics.pass1_candidates, 12);
        assert_eq!(metrics.pass2_filtered, 8);
        assert_eq!(metrics.verified_findings, 4);
        assert!((metrics.total_cost_saved - 75.50).abs() < f64::EPSILON);

        let recent = db.list_recent_reviews(10).await.expect("Should list reviews");
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].repo, "kleinbem/nix-config");
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
        assert_eq!(fps[0].reason.as_deref(), Some("Intentionally read-only in nspawn containers"));
    }
}
