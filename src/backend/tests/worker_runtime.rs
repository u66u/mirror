use std::time::Duration;

use mirror_backend::{
    assets::promote_verified_upload,
    jobs::{self, JobKind, JobSpec, enqueue_in_tx},
    media::RustImageProcessor,
    worker::{WorkerPolicy, WorkerPolicyError, WorkerStep, run_once},
};
use serde_json::json;
use time::OffsetDateTime;
use uuid::Uuid;

mod support;
use support::{
    DelayedImageProcessor, FakeImageProcessor, FakeVideoProcessor, TestResult,
    create_verified_jpeg_upload, storage_test_deps,
};

#[tokio::test]
#[ignore = "requires MIRROR_TEST_DATABASE_URL pointing at a dedicated test database"]
async fn worker_runs_queued_media_jobs_to_completion() -> TestResult {
    let deps = storage_test_deps().await?;
    let upload_id = create_verified_jpeg_upload(&deps.pool, &deps.storage, "worker.jpg").await?;
    promote_verified_upload(&deps.pool, &deps.storage, 1, upload_id).await?;

    let first = run_once(
        &deps.pool,
        &deps.storage,
        &FakeImageProcessor,
        &FakeVideoProcessor,
        "worker-a",
        WorkerPolicy::production(),
    )
    .await?;
    let second = run_once(
        &deps.pool,
        &deps.storage,
        &FakeImageProcessor,
        &FakeVideoProcessor,
        "worker-a",
        WorkerPolicy::production(),
    )
    .await?;

    assert_eq!(first, WorkerStep::Completed);
    assert_eq!(second, WorkerStep::Completed);
    assert_eq!(job_count_by_status(&deps.pool, "done").await?, 2);
    assert_eq!(metadata_count(&deps.pool).await?, 1);
    assert_eq!(derivative_count(&deps.pool).await?, 2);

    Ok(())
}

#[tokio::test]
#[ignore = "requires MIRROR_TEST_DATABASE_URL pointing at a dedicated test database"]
async fn worker_records_handler_failure_as_dead_job_at_max_attempts() -> TestResult {
    let deps = storage_test_deps().await?;
    let mut tx = deps.pool.begin().await?;
    enqueue_in_tx(
        &mut tx,
        JobSpec::immediate(
            JobKind::ExtractMetadata,
            json!({ "asset_id": Uuid::now_v7() }),
            "missing-asset".to_owned(),
        ),
    )
    .await?;
    tx.commit().await?;
    sqlx::query("UPDATE jobs SET max_attempts = 1 WHERE idempotency_key = 'missing-asset'")
        .execute(&deps.pool)
        .await?;

    let step = run_once(
        &deps.pool,
        &deps.storage,
        &RustImageProcessor,
        &FakeVideoProcessor,
        "worker-a",
        WorkerPolicy::production(),
    )
    .await?;

    assert_eq!(step, WorkerStep::Failed);
    assert_eq!(job_count_by_status(&deps.pool, "dead").await?, 1);

    Ok(())
}

#[tokio::test]
#[ignore = "requires MIRROR_TEST_DATABASE_URL pointing at a dedicated test database"]
async fn worker_times_out_slow_media_and_records_retry() -> TestResult {
    let deps = storage_test_deps().await?;
    let upload_id = create_verified_jpeg_upload(&deps.pool, &deps.storage, "timeout.jpg").await?;
    promote_verified_upload(&deps.pool, &deps.storage, 1, upload_id).await?;
    sqlx::query("DELETE FROM jobs WHERE kind = 'generate_derivatives'")
        .execute(&deps.pool)
        .await?;
    let policy = WorkerPolicy::new(
        Duration::from_secs(1),
        Duration::from_millis(20),
        Duration::from_millis(5),
    )?;

    let step = run_once(
        &deps.pool,
        &deps.storage,
        &DelayedImageProcessor {
            delay: Duration::from_millis(100),
        },
        &FakeVideoProcessor,
        "worker-timeout",
        policy,
    )
    .await?;
    let (status, message) =
        sqlx::query_as::<_, (String, String)>("SELECT status, last_error->>'message' FROM jobs")
            .fetch_one(&deps.pool)
            .await?;

    assert_eq!(step, WorkerStep::TimedOut);
    assert_eq!(status, "queued");
    assert_eq!(message, "media job timed out");
    Ok(())
}

#[tokio::test]
#[ignore = "requires MIRROR_TEST_DATABASE_URL pointing at a dedicated test database"]
async fn worker_heartbeat_prevents_slow_job_reclaim() -> TestResult {
    let deps = storage_test_deps().await?;
    let upload_id = create_verified_jpeg_upload(&deps.pool, &deps.storage, "heartbeat.jpg").await?;
    promote_verified_upload(&deps.pool, &deps.storage, 1, upload_id).await?;
    sqlx::query("DELETE FROM jobs WHERE kind = 'generate_derivatives'")
        .execute(&deps.pool)
        .await?;
    let pool = deps.pool.clone();
    let storage = deps.storage.clone();
    let policy = WorkerPolicy::new(
        Duration::from_millis(80),
        Duration::from_secs(1),
        Duration::from_millis(10),
    )?;
    let running = tokio::spawn(async move {
        run_once(
            &pool,
            &storage,
            &DelayedImageProcessor {
                delay: Duration::from_millis(150),
            },
            &FakeVideoProcessor,
            "worker-heartbeat",
            policy,
        )
        .await
    });

    tokio::time::sleep(Duration::from_millis(100)).await;
    let reclaim_cutoff = OffsetDateTime::now_utc() - time::Duration::milliseconds(80);
    let reclaimed = jobs::lease_next(&deps.pool, "worker-reclaim", reclaim_cutoff).await?;
    let step = running.await??;

    assert!(reclaimed.is_none());
    assert_eq!(step, WorkerStep::Completed);
    Ok(())
}

#[test]
fn worker_policy_rejects_heartbeat_that_can_expire_its_lease() {
    let result = WorkerPolicy::new(
        Duration::from_secs(30),
        Duration::from_secs(10),
        Duration::from_secs(30),
    );

    assert_eq!(result, Err(WorkerPolicyError::HeartbeatNotShorterThanLease));
}

async fn job_count_by_status(pool: &sqlx::PgPool, status: &str) -> TestResult<i64> {
    Ok(
        sqlx::query_scalar("SELECT count(*) FROM jobs WHERE status = $1")
            .bind(status)
            .fetch_one(pool)
            .await?,
    )
}

async fn metadata_count(pool: &sqlx::PgPool) -> TestResult<i64> {
    Ok(sqlx::query_scalar("SELECT count(*) FROM asset_metadata")
        .fetch_one(pool)
        .await?)
}

async fn derivative_count(pool: &sqlx::PgPool) -> TestResult<i64> {
    Ok(sqlx::query_scalar("SELECT count(*) FROM derivatives")
        .fetch_one(pool)
        .await?)
}
