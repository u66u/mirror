//! Background worker orchestration.
//!
//! This module owns queue-to-handler control flow only. Job storage stays in
//! `jobs`; media behavior stays in `media`.

use time::{Duration, OffsetDateTime};

use crate::{
    jobs::{self, JobError},
    media::{self, ImageProcessor},
    storage::ObjectStorage,
};

/// Result of one worker polling iteration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerStep {
    /// No ready or expired job was available.
    Idle,
    /// A job ran and was marked done.
    Completed,
    /// A job ran unsuccessfully and was marked retry/dead.
    Failed,
}

/// Worker orchestration failure.
#[derive(Debug)]
pub enum WorkerError {
    /// Queue operation failed.
    Jobs(JobError),
}

impl std::fmt::Display for WorkerError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Jobs(_) => formatter.write_str("worker queue error"),
        }
    }
}

impl std::error::Error for WorkerError {}

/// Runs at most one ready job.
///
/// Handler failures are recorded in the job row and do not return `Err`; only
/// queue transition failures do.
pub async fn run_once(
    pool: &sqlx::PgPool,
    storage: &ObjectStorage,
    processor: &impl ImageProcessor,
    worker_id: &str,
    lease_timeout: Duration,
) -> Result<WorkerStep, WorkerError> {
    let lease_expired_before = OffsetDateTime::now_utc() - lease_timeout;
    let Some(job) = jobs::lease_next(pool, worker_id, lease_expired_before)
        .await
        .map_err(WorkerError::Jobs)?
    else {
        return Ok(WorkerStep::Idle);
    };

    match media::run_media_job(pool, storage, processor, &job).await {
        Ok(()) => {
            jobs::complete(pool, job.id, worker_id)
                .await
                .map_err(WorkerError::Jobs)?;
            Ok(WorkerStep::Completed)
        }
        Err(error) => {
            jobs::fail(pool, job.id, worker_id, &error.to_string())
                .await
                .map_err(WorkerError::Jobs)?;
            Ok(WorkerStep::Failed)
        }
    }
}
