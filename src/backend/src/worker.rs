//! Background worker orchestration.
//!
//! This module owns queue-to-handler control flow only. Job storage stays in
//! `jobs`; media behavior stays in `media`.

use std::time::Duration;

use time::OffsetDateTime;

use crate::{
    jobs::{self, JobError},
    media::{self, ImageProcessor},
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerPolicyError {
    /// Lease expiry must be positive.
    ZeroLeaseTimeout,
    /// Job wall-clock budget must be positive.
    ZeroJobTimeout,
    /// Heartbeat cadence must be positive.
    ZeroHeartbeatInterval,
    /// Heartbeats must occur before a lease can be reclaimed.
    HeartbeatNotShorterThanLease,
    /// Lease timeout cannot be represented by database timestamp arithmetic.
    LeaseTimeoutOutOfRange,
}

impl std::fmt::Display for WorkerPolicyError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::ZeroLeaseTimeout => "worker lease timeout must be positive",
            Self::ZeroJobTimeout => "worker job timeout must be positive",
            Self::ZeroHeartbeatInterval => "worker heartbeat interval must be positive",
            Self::HeartbeatNotShorterThanLease => {
                "worker heartbeat interval must be shorter than lease timeout"
            }
            Self::LeaseTimeoutOutOfRange => "worker lease timeout is out of range",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for WorkerPolicyError {}

/// Worker orchestration failure.
#[derive(Debug)]
pub enum WorkerError {
    /// Queue operation failed.
    Jobs(JobError),
    /// This worker no longer owns the leased job.
    LeaseLost,
    /// Timing policy cannot be applied.
    Policy(WorkerPolicyError),
}

impl std::fmt::Display for WorkerError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Jobs(_) => formatter.write_str("worker queue error"),
            Self::LeaseLost => formatter.write_str("worker job lease lost"),
            Self::Policy(_) => formatter.write_str("invalid worker timing policy"),
        }
    }
}

impl std::error::Error for WorkerError {}

/// Runs at most one ready job.
///
/// Handler failures are recorded in the job row and do not return `Err`; only
/// queue transition failures do.
pub async fn run_once<I, V>(
    pool: &sqlx::PgPool,
    storage: &ObjectStorage,
    image_processor: &I,
    video_processor: &V,
    worker_id: &str,
    policy: WorkerPolicy,
) -> Result<WorkerStep, WorkerError>
where
    I: ImageProcessor,
    V: VideoProcessor,
{
    let lease_timeout = time::Duration::try_from(policy.lease_timeout)
        .map_err(|_| WorkerError::Policy(WorkerPolicyError::LeaseTimeoutOutOfRange))?;
    let lease_expired_before = OffsetDateTime::now_utc() - lease_timeout;
    let Some(job) = jobs::lease_next(pool, worker_id, lease_expired_before)
        .await
        .map_err(WorkerError::Jobs)?
    else {
        return Ok(WorkerStep::Idle);
    };

    let handler = media::run_media_job(pool, storage, image_processor, video_processor, &job);
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
            let failed = jobs::fail(pool, job.id, worker_id, "media job timed out")
                .await
                .map_err(WorkerError::Jobs)?;
            if !failed {
                return Err(WorkerError::LeaseLost);
            }
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
            let failed = jobs::fail(pool, job.id, worker_id, &error.to_string())
                .await
                .map_err(WorkerError::Jobs)?;
            if !failed {
                return Err(WorkerError::LeaseLost);
            }
            Ok(WorkerStep::Failed)
        }
    }
}
