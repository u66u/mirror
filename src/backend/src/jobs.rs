//! Durable Postgres job queue.
//!
//! The queue owns storage/leasing state only. Job handlers stay in feature
//! modules, so this module remains a small reliability kernel instead of an
//! event bus or task framework.

use serde_json::{Value, json};
use sqlx::{PgPool, Postgres, Transaction, types::Json};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;
use thiserror::Error;

/// Job kinds currently emitted by backend feature modules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobKind {
    /// Extract owner-visible metadata from an original.
    ExtractMetadata,
    /// Generate reproducible thumbnail/preview derivatives.
    GenerateDerivatives,
    /// Compute an asset embedding for one validated model pack.
    EmbedAsset,
}

impl JobKind {
    /// Stable database representation.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ExtractMetadata => "extract_metadata",
            Self::GenerateDerivatives => "generate_derivatives",
            Self::EmbedAsset => "embed_asset",
        }
    }

    fn from_str(value: &str) -> Result<Self, JobError> {
        match value {
            "extract_metadata" => Ok(Self::ExtractMetadata),
            "generate_derivatives" => Ok(Self::GenerateDerivatives),
            "embed_asset" => Ok(Self::EmbedAsset),
            _ => Err(JobError::InvalidKind(value.to_owned())),
        }
    }
}

/// Queue row to insert.
#[derive(Debug)]
pub struct JobSpec {
    /// Handler kind.
    pub kind: JobKind,
    /// Structured handler input.
    pub payload: Value,
    /// Stable idempotency key for this side effect.
    pub idempotency_key: String,
    /// Higher priority leases first.
    pub priority: i32,
    /// Earliest time the job may be leased.
    pub run_after: Option<OffsetDateTime>,
}

impl JobSpec {
    /// Creates an immediate normal-priority job.
    pub fn immediate(kind: JobKind, payload: Value, idempotency_key: String) -> Self {
        Self {
            kind,
            payload,
            idempotency_key,
            priority: 0,
            run_after: None,
        }
    }
}

/// Leased job returned to a worker.
#[derive(Debug, PartialEq)]
pub struct LeasedJob {
    /// Job row ID.
    pub id: Uuid,
    /// Handler kind.
    pub kind: JobKind,
    /// Structured handler input.
    pub payload: Value,
    /// Number of started attempts including this lease.
    pub attempts: i32,
    /// Maximum attempts before dead-lettering.
    pub max_attempts: i32,
}

/// Result of recording a leased job failure.
#[derive(Debug, PartialEq)]
pub struct JobFailureOutcome {
    /// Whether the leased job row was updated.
    pub updated: bool,
    /// Whether the job reached its terminal dead-letter state.
    pub dead: bool,
    /// Job row ID.
    pub job_id: Uuid,
    /// Handler kind.
    pub kind: JobKind,
    /// Structured handler input.
    pub payload: Value,
    /// Number of attempts that have started.
    pub attempts: i32,
    /// Maximum attempts before dead-lettering.
    pub max_attempts: i32,
}

/// Job operation failure.
#[derive(Debug, Error)]
pub enum JobError {
    /// Job kind was not recognized by this binary.
    #[error("invalid job kind")]
    InvalidKind(String),
    /// Database failed.
    #[error("job database error")]
    Database(#[from] sqlx::Error),
}

/// Enqueues a job in the caller's transaction.
///
/// Idempotency is enforced by the `jobs.idempotency_key` uniqueness constraint.
pub async fn enqueue_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    spec: JobSpec,
) -> Result<(), JobError> {
    sqlx::query(
        r#"
        INSERT INTO jobs (id, kind, payload, idempotency_key, priority, run_after)
        VALUES ($1, $2, $3, $4, $5, COALESCE($6::timestamptz, now()))
        ON CONFLICT (idempotency_key) DO NOTHING
        "#,
    )
    .bind(Uuid::now_v7())
    .bind(spec.kind.as_str())
    .bind(Json(spec.payload))
    .bind(spec.idempotency_key)
    .bind(spec.priority)
    .bind(spec.run_after)
    .execute(&mut **tx)
    .await
    .map_err(JobError::Database)?;

    Ok(())
}

/// Leases one ready job or one expired leased job.
///
/// `lease_expired_before` is computed by the worker from its configured lease
/// timeout. Using an absolute cutoff keeps tests deterministic.
pub async fn lease_next(
    pool: &PgPool,
    worker_id: &str,
    lease_expired_before: OffsetDateTime,
) -> Result<Option<LeasedJob>, JobError> {
    lease_next_for_kinds(
        pool,
        worker_id,
        lease_expired_before,
        &[
            JobKind::ExtractMetadata,
            JobKind::GenerateDerivatives,
            JobKind::EmbedAsset,
        ],
    )
    .await
}

/// Leases one ready or expired job whose kind this worker can run.
pub async fn lease_next_for_kinds(
    pool: &PgPool,
    worker_id: &str,
    lease_expired_before: OffsetDateTime,
    kinds: &[JobKind],
) -> Result<Option<LeasedJob>, JobError> {
    if kinds.is_empty() {
        return Ok(None);
    }

    let kind_names: Vec<&str> = kinds.iter().map(|kind| kind.as_str()).collect();

    let row = sqlx::query_as::<_, (Uuid, String, Json<Value>, i32, i32)>(
        r#"
        UPDATE jobs
        SET
            status = 'leased',
            lease_owner = $1,
            leased_at = now(),
            heartbeat_at = now(),
            attempts = attempts + 1,
            updated_at = now()
        WHERE id = (
            SELECT id
            FROM jobs
            WHERE ((
                    status = 'queued'
                    AND run_after <= now()
                )
                OR (
                    status = 'leased'
                    AND COALESCE(heartbeat_at, leased_at, created_at) < $2
                ))
              AND kind = ANY($3)
            ORDER BY priority DESC, run_after ASC, created_at ASC
            FOR UPDATE SKIP LOCKED
            LIMIT 1
        )
        RETURNING id, kind, payload, attempts, max_attempts
        "#,
    )
    .bind(worker_id)
    .bind(lease_expired_before)
    .bind(kind_names)
    .fetch_optional(pool)
    .await
    .map_err(JobError::Database)?;

    row.map(|(id, kind, payload, attempts, max_attempts)| {
        Ok(LeasedJob {
            id,
            kind: JobKind::from_str(&kind)?,
            payload: payload.0,
            attempts,
            max_attempts,
        })
    })
    .transpose()
}

/// Records that a worker still owns a leased job.
pub async fn heartbeat(pool: &PgPool, job_id: Uuid, worker_id: &str) -> Result<bool, JobError> {
    let updated = sqlx::query(
        r#"
        UPDATE jobs
        SET heartbeat_at = now(), updated_at = now()
        WHERE id = $1
          AND status = 'leased'
          AND lease_owner = $2
        "#,
    )
    .bind(job_id)
    .bind(worker_id)
    .execute(pool)
    .await
    .map_err(JobError::Database)?
    .rows_affected();

    Ok(updated == 1)
}

/// Marks a leased job complete.
pub async fn complete(pool: &PgPool, job_id: Uuid, worker_id: &str) -> Result<bool, JobError> {
    let updated = sqlx::query(
        r#"
        UPDATE jobs
        SET
            status = 'done',
            lease_owner = NULL,
            leased_at = NULL,
            heartbeat_at = NULL,
            updated_at = now()
        WHERE id = $1
          AND status = 'leased'
          AND lease_owner = $2
        "#,
    )
    .bind(job_id)
    .bind(worker_id)
    .execute(pool)
    .await
    .map_err(JobError::Database)?
    .rows_affected();

    Ok(updated == 1)
}

/// Records a leased job failure and either retries it or dead-letters it.
///
/// This preserves the old boolean API. New worker loops should prefer
/// `fail_with_outcome` so feature modules can react to dead-lettering.
pub async fn fail(
    pool: &PgPool,
    job_id: Uuid,
    worker_id: &str,
    message: &str,
) -> Result<bool, JobError> {
    Ok(
        match fail_with_outcome(pool, job_id, worker_id, message).await? {
            Some(outcome) => outcome.updated,
            None => false,
        },
    )
}

/// Records a leased job failure and returns whether it dead-lettered.
///
/// Retry delay is exponential from the started attempt count, capped at one
/// hour. The failure payload is structured for future admin diagnostics.
pub async fn fail_with_outcome(
    pool: &PgPool,
    job_id: Uuid,
    worker_id: &str,
    message: &str,
) -> Result<Option<JobFailureOutcome>, JobError> {
    let mut tx = pool.begin().await.map_err(JobError::Database)?;

    let Some((kind, payload, attempts, max_attempts)) =
        sqlx::query_as::<_, (String, Json<Value>, i32, i32)>(
            r#"
            SELECT kind, payload, attempts, max_attempts
            FROM jobs
            WHERE id = $1
              AND status = 'leased'
              AND lease_owner = $2
            FOR UPDATE
            "#,
        )
        .bind(job_id)
        .bind(worker_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(JobError::Database)?
    else {
        return Ok(None);
    };

    let kind = JobKind::from_str(&kind)?;
    let payload = payload.0;
    let dead = attempts >= max_attempts;
    let next_status = if dead { "dead" } else { "queued" };
    let run_after = if dead {
        None
    } else {
        Some(OffsetDateTime::now_utc() + retry_backoff(attempts))
    };
    let last_error = json!({
        "message": message,
        "attempts": attempts,
        "dead": dead,
    });

    let updated = sqlx::query(
        r#"
        UPDATE jobs
        SET
            status = $3,
            run_after = COALESCE($4, run_after),
            lease_owner = NULL,
            leased_at = NULL,
            heartbeat_at = NULL,
            last_error = $5,
            updated_at = now()
        WHERE id = $1
          AND lease_owner = $2
          AND status = 'leased'
        "#,
    )
    .bind(job_id)
    .bind(worker_id)
    .bind(next_status)
    .bind(run_after)
    .bind(Json(last_error))
    .execute(&mut *tx)
    .await
    .map_err(JobError::Database)?
    .rows_affected()
        == 1;

    tx.commit().await.map_err(JobError::Database)?;

    Ok(Some(JobFailureOutcome {
        updated,
        dead,
        job_id,
        kind,
        payload,
        attempts,
        max_attempts,
    }))
}

fn retry_backoff(attempts: i32) -> Duration {
    let exponent = attempts.saturating_sub(1).clamp(0, 7) as u32;
    let multiplier = 2_i64.pow(exponent);
    Duration::seconds((30 * multiplier).min(3600))
}
