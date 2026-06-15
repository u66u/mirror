use mirror_backend::jobs::{
    JobKind, JobSpec, complete, enqueue_in_tx, fail, heartbeat, lease_next,
};
use serde_json::json;
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

mod support;
use support::{TestResult, fresh_owner_pool};

#[tokio::test]
#[ignore = "requires MIRROR_TEST_DATABASE_URL pointing at a dedicated test database"]
async fn leasing_respects_run_after_and_priority() -> TestResult {
    let pool = fresh_owner_pool().await?;
    enqueue_job(
        &pool,
        JobSpec {
            kind: JobKind::ExtractMetadata,
            payload: json!({ "name": "future" }),
            idempotency_key: "future".to_owned(),
            priority: 100,
            run_after: Some(OffsetDateTime::now_utc() + Duration::hours(1)),
        },
    )
    .await?;
    let low = enqueue_job(
        &pool,
        JobSpec {
            kind: JobKind::GenerateDerivatives,
            payload: json!({ "name": "low" }),
            idempotency_key: "low".to_owned(),
            priority: 0,
            run_after: None,
        },
    )
    .await?;
    let high = enqueue_job(
        &pool,
        JobSpec {
            kind: JobKind::ExtractMetadata,
            payload: json!({ "name": "high" }),
            idempotency_key: "high".to_owned(),
            priority: 10,
            run_after: None,
        },
    )
    .await?;

    let first = lease_next(&pool, "worker-a", stale_cutoff()).await?;
    assert_eq!(first.map(|job| job.id), Some(high));

    let second = lease_next(&pool, "worker-a", stale_cutoff()).await?;
    assert_eq!(second.map(|job| job.id), Some(low));

    let third = lease_next(&pool, "worker-a", stale_cutoff()).await?;
    assert!(third.is_none());

    Ok(())
}

#[tokio::test]
#[ignore = "requires MIRROR_TEST_DATABASE_URL pointing at a dedicated test database"]
async fn leased_job_is_exclusive_until_completed() -> TestResult {
    let pool = fresh_owner_pool().await?;
    let job_id = enqueue_job(
        &pool,
        JobSpec::immediate(
            JobKind::ExtractMetadata,
            json!({ "asset_id": Uuid::now_v7() }),
            "exclusive".to_owned(),
        ),
    )
    .await?;

    let leased = lease_next(&pool, "worker-a", stale_cutoff()).await?;
    assert_eq!(leased.map(|job| job.id), Some(job_id));
    assert!(
        lease_next(&pool, "worker-b", stale_cutoff())
            .await?
            .is_none()
    );
    assert!(heartbeat(&pool, job_id, "worker-a").await?);
    assert!(!heartbeat(&pool, job_id, "worker-b").await?);
    assert!(complete(&pool, job_id, "worker-a").await?);
    assert!(
        lease_next(&pool, "worker-a", stale_cutoff())
            .await?
            .is_none()
    );

    Ok(())
}

#[tokio::test]
#[ignore = "requires MIRROR_TEST_DATABASE_URL pointing at a dedicated test database"]
async fn expired_lease_can_be_reclaimed_and_failure_retries() -> TestResult {
    let pool = fresh_owner_pool().await?;
    let job_id = enqueue_job(
        &pool,
        JobSpec::immediate(
            JobKind::GenerateDerivatives,
            json!({ "asset_id": Uuid::now_v7() }),
            "reclaim".to_owned(),
        ),
    )
    .await?;

    assert_eq!(
        lease_next(&pool, "worker-a", stale_cutoff())
            .await?
            .map(|job| job.id),
        Some(job_id)
    );
    sqlx::query("UPDATE jobs SET heartbeat_at = now() - interval '2 hours' WHERE id = $1")
        .bind(job_id)
        .execute(&pool)
        .await?;

    let reclaimed = lease_next(&pool, "worker-b", stale_cutoff()).await?;

    assert_eq!(reclaimed.as_ref().map(|job| job.id), Some(job_id));
    assert_eq!(reclaimed.as_ref().map(|job| job.attempts), Some(2));
    assert!(!fail(&pool, job_id, "worker-a", "stale worker failed late").await?);
    assert!(fail(&pool, job_id, "worker-b", "transient failure").await?);
    assert_eq!(job_status(&pool, job_id).await?, "queued");

    Ok(())
}

#[tokio::test]
#[ignore = "requires MIRROR_TEST_DATABASE_URL pointing at a dedicated test database"]
async fn failure_dead_letters_after_max_attempts() -> TestResult {
    let pool = fresh_owner_pool().await?;
    let job_id = enqueue_job(
        &pool,
        JobSpec::immediate(
            JobKind::ExtractMetadata,
            json!({ "asset_id": Uuid::now_v7() }),
            "dead".to_owned(),
        ),
    )
    .await?;
    sqlx::query("UPDATE jobs SET max_attempts = 1 WHERE id = $1")
        .bind(job_id)
        .execute(&pool)
        .await?;

    assert_eq!(
        lease_next(&pool, "worker-a", stale_cutoff())
            .await?
            .map(|job| job.id),
        Some(job_id)
    );
    assert!(fail(&pool, job_id, "worker-a", "fatal failure").await?);

    let (status, message) = sqlx::query_as::<_, (String, String)>(
        "SELECT status, last_error->>'message' FROM jobs WHERE id = $1",
    )
    .bind(job_id)
    .fetch_one(&pool)
    .await?;

    assert_eq!(status, "dead");
    assert_eq!(message, "fatal failure");

    Ok(())
}

async fn enqueue_job(pool: &sqlx::PgPool, spec: JobSpec) -> TestResult<Uuid> {
    let idempotency_key = spec.idempotency_key.clone();
    let mut tx = pool.begin().await?;
    enqueue_in_tx(&mut tx, spec).await?;
    tx.commit().await?;

    let job_id = sqlx::query_scalar("SELECT id FROM jobs WHERE idempotency_key = $1")
        .bind(idempotency_key)
        .fetch_one(pool)
        .await?;
    Ok(job_id)
}

async fn job_status(pool: &sqlx::PgPool, job_id: Uuid) -> TestResult<String> {
    Ok(sqlx::query_scalar("SELECT status FROM jobs WHERE id = $1")
        .bind(job_id)
        .fetch_one(pool)
        .await?)
}

fn stale_cutoff() -> OffsetDateTime {
    OffsetDateTime::now_utc() - Duration::minutes(30)
}
