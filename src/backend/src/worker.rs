//! Background worker orchestration.
//!
//! This module owns queue-to-handler control flow only. Job storage stays in
//! `jobs`; media behavior stays in `media`.

use std::time::Duration;
use thiserror::Error;

use time::OffsetDateTime;

use crate::{
    face::{self, SharedFaceRuntime},
    integrity,
    jobs::{self, JobError, JobKind},
    media::{self, ImageProcessor},
    ml::{self, ImageTextEmbedder, MlRuntime},
    storage::ObjectStorage,
    video::VideoProcessor,
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
    /// A job exceeded its wall-clock budget and was marked retry/dead.
    TimedOut,
}

/// Validated worker timing policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkerPolicy {
    lease_timeout: Duration,
    job_timeout: Duration,
    heartbeat_interval: Duration,
}

impl WorkerPolicy {
    /// Builds a timing policy whose heartbeat cannot expire its own lease.
    pub fn new(
        lease_timeout: Duration,
        job_timeout: Duration,
        heartbeat_interval: Duration,
    ) -> Result<Self, WorkerPolicyError> {
        if lease_timeout.is_zero() {
            return Err(WorkerPolicyError::ZeroLeaseTimeout);
        }
        if job_timeout.is_zero() {
            return Err(WorkerPolicyError::ZeroJobTimeout);
        }
        if heartbeat_interval.is_zero() {
            return Err(WorkerPolicyError::ZeroHeartbeatInterval);
        }
        if heartbeat_interval >= lease_timeout {
            return Err(WorkerPolicyError::HeartbeatNotShorterThanLease);
        }
        Ok(Self {
            lease_timeout,
            job_timeout,
            heartbeat_interval,
        })
    }

    /// Production defaults: 30-minute lease, 15-minute job, 30-second heartbeat.
    #[must_use]
    pub const fn production() -> Self {
        Self {
            lease_timeout: Duration::from_secs(30 * 60),
            job_timeout: Duration::from_secs(15 * 60),
            heartbeat_interval: Duration::from_secs(30),
        }
    }
}

/// Invalid worker timing policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum WorkerPolicyError {
    /// Lease expiry must be positive.
    #[error("worker lease timeout must be positive")]
    ZeroLeaseTimeout,
    /// Job wall-clock budget must be positive.
    #[error("worker job timeout must be positive")]
    ZeroJobTimeout,
    /// Heartbeat cadence must be positive.
    #[error("worker heartbeat interval must be positive")]
    ZeroHeartbeatInterval,
    /// Heartbeats must occur before a lease can be reclaimed.
    #[error("worker heartbeat interval must be shorter than lease timeout")]
    HeartbeatNotShorterThanLease,
    /// Lease timeout cannot be represented by database timestamp arithmetic.
    #[error("worker lease timeout is out of range")]
    LeaseTimeoutOutOfRange,
}

/// Worker orchestration failure.
#[derive(Debug, Error)]
pub enum WorkerError {
    /// Queue operation failed.
    #[error("worker queue error: {0}")]
    Jobs(#[from] JobError),
    /// Media handler failed.
    #[error("worker media error: {0}")]
    Media(#[from] media::MediaError),
    /// ML handler failed.
    #[error("worker ml error: {0}")]
    Ml(#[from] ml::MlError),
    /// Face indexing handler failed.
    #[error("worker face error: {0}")]
    Face(#[from] face::FaceIndexError),
    /// Integrity handler failed.
    #[error("worker integrity error: {0}")]
    Integrity(#[from] integrity::IntegrityError),
    /// This worker no longer owns the leased job.
    #[error("worker job lease lost")]
    LeaseLost,
    /// Timing policy cannot be applied.
    #[error("invalid worker timing policy: {0}")]
    Policy(#[from] WorkerPolicyError),
}

/// Handler dependencies for one worker process.
pub struct WorkerHandlers<'a, I, V, E> {
    /// Object storage used by media and ML jobs.
    pub storage: &'a ObjectStorage,
    /// Still-image media processor.
    pub image_processor: &'a I,
    /// Video media processor.
    pub video_processor: &'a V,
    /// Semantic image/text runtime.
    pub ml_runtime: &'a MlRuntime<E>,
    /// Face detection/embedding runtime.
    pub face_runtime: &'a SharedFaceRuntime,
    /// Job kinds this worker is allowed to lease.
    pub job_kinds: &'a [JobKind],
}

/// Job kinds leased by the production worker for the given feature flags.
#[must_use]
pub fn production_job_kinds(face_recognition_enabled: bool) -> Vec<JobKind> {
    let mut kinds = vec![
        JobKind::ExtractMetadata,
        JobKind::GenerateDerivatives,
        JobKind::EmbedAsset,
        JobKind::IntegrityScan,
    ];
    if face_recognition_enabled {
        kinds.push(JobKind::IndexFaces);
    }
    kinds
}

/// Runs at most one ready job.
///
/// Handler failures are recorded in the job row and do not return `Err`; only
/// queue transition failures do.
pub async fn run_once<I, V, E>(
    pool: &sqlx::PgPool,
    handlers: WorkerHandlers<'_, I, V, E>,
    worker_id: &str,
    policy: WorkerPolicy,
) -> Result<WorkerStep, WorkerError>
where
    I: ImageProcessor,
    V: VideoProcessor,
    E: ImageTextEmbedder + Send + Sync + 'static,
{
    let lease_timeout = time::Duration::try_from(policy.lease_timeout)
        .map_err(|_| WorkerError::Policy(WorkerPolicyError::LeaseTimeoutOutOfRange))?;
    let lease_expired_before = OffsetDateTime::now_utc() - lease_timeout;
    let Some(job) =
        jobs::lease_next_for_kinds(pool, worker_id, lease_expired_before, handlers.job_kinds)
            .await
            .map_err(WorkerError::Jobs)?
    else {
        return Ok(WorkerStep::Idle);
    };

    let handler = async {
        match job.kind {
            JobKind::ExtractMetadata | JobKind::GenerateDerivatives => media::run_media_job(
                pool,
                handlers.storage,
                handlers.image_processor,
                handlers.video_processor,
                &job,
            )
            .await
            .map_err(WorkerError::Media),
            JobKind::EmbedAsset => {
                ml::run_ml_job(pool, handlers.storage, handlers.ml_runtime, &job)
                    .await
                    .map_err(WorkerError::Ml)
            }
            JobKind::IndexFaces => {
                face::run_face_index_job(pool, handlers.storage, handlers.face_runtime, &job)
                    .await
                    .map_err(WorkerError::Face)
            }
            JobKind::IntegrityScan => integrity::run_integrity_job(pool, handlers.storage, &job)
                .await
                .map_err(WorkerError::Integrity),
        }
    };
    tokio::pin!(handler);
    let deadline = tokio::time::sleep(policy.job_timeout);
    tokio::pin!(deadline);
    let mut heartbeat = tokio::time::interval_at(
        tokio::time::Instant::now() + policy.heartbeat_interval,
        policy.heartbeat_interval,
    );
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    let handler_result = loop {
        tokio::select! {
            result = &mut handler => break Some(result),
            () = &mut deadline => break None,
            _ = heartbeat.tick() => {
                let still_owned = jobs::heartbeat(pool, job.id, worker_id)
                    .await
                    .map_err(WorkerError::Jobs)?;
                if !still_owned {
                    return Err(WorkerError::LeaseLost);
                }
            }
        }
    };

    match handler_result {
        None => {
            fail_leased_job(pool, worker_id, &job, timeout_message(job.kind)).await?;
            Ok(WorkerStep::TimedOut)
        }
        Some(Ok(())) => {
            let completed = jobs::complete(pool, job.id, worker_id)
                .await
                .map_err(WorkerError::Jobs)?;
            if !completed {
                return Err(WorkerError::LeaseLost);
            }
            Ok(WorkerStep::Completed)
        }
        Some(Err(error)) => {
            let message = failure_message(&error);
            fail_leased_job(pool, worker_id, &job, &message).await?;
            Ok(WorkerStep::Failed)
        }
    }
}

fn failure_message(error: &WorkerError) -> String {
    if let WorkerError::Ml(error) = error {
        return error.to_string();
    }
    error.to_string()
}

fn timeout_message(kind: JobKind) -> &'static str {
    match kind {
        JobKind::ExtractMetadata | JobKind::GenerateDerivatives => "media job timed out",
        JobKind::EmbedAsset => "ml job timed out",
        JobKind::IndexFaces => "face indexing job timed out",
        JobKind::IntegrityScan => "integrity job timed out",
    }
}

async fn fail_leased_job(
    pool: &sqlx::PgPool,
    worker_id: &str,
    job: &jobs::LeasedJob,
    message: &str,
) -> Result<(), WorkerError> {
    let outcome = jobs::fail_with_outcome(pool, job.id, worker_id, message)
        .await
        .map_err(WorkerError::Jobs)?
        .ok_or(WorkerError::LeaseLost)?;

    if !outcome.updated {
        return Err(WorkerError::LeaseLost);
    }

    if outcome.dead && outcome.kind == JobKind::EmbedAsset {
        ml::record_embed_asset_dead_letter_payload(pool, &outcome.payload, message)
            .await
            .map_err(WorkerError::Ml)?;
    }

    Ok(())
}
