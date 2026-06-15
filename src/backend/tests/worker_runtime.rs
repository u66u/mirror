use mirror_backend::{
    assets::promote_verified_upload,
    jobs::{JobKind, JobSpec, enqueue_in_tx},
    media::RustImageProcessor,
    worker::{WorkerStep, run_once},
};
use serde_json::json;
use time::Duration;
use uuid::Uuid;

mod support;
use support::{FakeImageProcessor, TestResult, create_verified_jpeg_upload, storage_test_deps};

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
        "worker-a",
        Duration::minutes(30),
    )
    .await?;
    let second = run_once(
        &deps.pool,
        &deps.storage,
        &FakeImageProcessor,
        "worker-a",
        Duration::minutes(30),
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
        "worker-a",
        Duration::minutes(30),
    )
    .await?;

    assert_eq!(step, WorkerStep::Failed);
    assert_eq!(job_count_by_status(&deps.pool, "dead").await?, 1);

    Ok(())
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
