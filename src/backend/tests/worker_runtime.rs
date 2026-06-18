use std::{num::NonZeroUsize, sync::Arc, time::Duration};

use mirror_backend::{
    assets::promote_verified_upload,
    jobs::{self, JobKind, JobSpec, enqueue_in_tx},
    media::RustImageProcessor,
    ml::{
        EmbedImageRequest, EmbedTextRequest, ImageTextEmbedder, MlError, MlRuntime,
        semantic_text_search,
    },
    models::{
        activate_model_pack, install_model_pack, record_model_pack_self_test, start_model_reindex,
    },
    semantic_index::semantic_search,
    storage::StorageKey,
    worker::{WorkerHandlers, WorkerPolicy, WorkerPolicyError, WorkerStep, run_once},
};
use serde_json::json;
use time::OffsetDateTime;
use uuid::Uuid;

mod support;
use support::{
    DelayedImageProcessor, FakeImageProcessor, FakeVideoProcessor, TestResult,
    create_verified_jpeg_upload, fake_ml_runtime, storage_test_deps, valid_model_pack_manifest,
};

#[tokio::test]
#[ignore = "requires MIRROR_TEST_DATABASE_URL pointing at a dedicated test database"]
async fn worker_runs_queued_media_jobs_to_completion() -> TestResult {
    let deps = storage_test_deps().await?;
    let ml_runtime = fake_ml_runtime();
    let upload_id = create_verified_jpeg_upload(&deps.pool, &deps.storage, "worker.jpg").await?;
    promote_verified_upload(&deps.pool, &deps.storage, 1, upload_id).await?;

    let first = run_once(
        &deps.pool,
        WorkerHandlers {
            storage: &deps.storage,
            image_processor: &FakeImageProcessor,
            video_processor: &FakeVideoProcessor,
            ml_runtime: &ml_runtime,
            job_kinds: &MEDIA_JOB_KINDS,
        },
        "worker-a",
        WorkerPolicy::production(),
    )
    .await?;
    let second = run_once(
        &deps.pool,
        WorkerHandlers {
            storage: &deps.storage,
            image_processor: &FakeImageProcessor,
            video_processor: &FakeVideoProcessor,
            ml_runtime: &ml_runtime,
            job_kinds: &MEDIA_JOB_KINDS,
        },
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
    let ml_runtime = fake_ml_runtime();
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
    sqlx::query!("UPDATE jobs SET max_attempts = 1 WHERE idempotency_key = 'missing-asset'")
        .execute(&deps.pool)
        .await?;

    let step = run_once(
        &deps.pool,
        WorkerHandlers {
            storage: &deps.storage,
            image_processor: &RustImageProcessor,
            video_processor: &FakeVideoProcessor,
            ml_runtime: &ml_runtime,
            job_kinds: &MEDIA_JOB_KINDS,
        },
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
    let ml_runtime = fake_ml_runtime();
    let upload_id = create_verified_jpeg_upload(&deps.pool, &deps.storage, "timeout.jpg").await?;
    promote_verified_upload(&deps.pool, &deps.storage, 1, upload_id).await?;
    sqlx::query!("DELETE FROM jobs WHERE kind = 'generate_derivatives'")
        .execute(&deps.pool)
        .await?;
    let policy = WorkerPolicy::new(
        Duration::from_secs(1),
        Duration::from_millis(20),
        Duration::from_millis(5),
    )?;

    let step = run_once(
        &deps.pool,
        WorkerHandlers {
            storage: &deps.storage,
            image_processor: &DelayedImageProcessor {
                delay: Duration::from_millis(100),
            },
            video_processor: &FakeVideoProcessor,
            ml_runtime: &ml_runtime,
            job_kinds: &MEDIA_JOB_KINDS,
        },
        "worker-timeout",
        policy,
    )
    .await?;
    let (status, message) = sqlx::query!("SELECT status, last_error->>'message' as msg FROM jobs")
        .map(|r| (r.status, r.msg.unwrap_or_default()))
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
    sqlx::query!("DELETE FROM jobs WHERE kind = 'generate_derivatives'")
        .execute(&deps.pool)
        .await?;
    let pool = deps.pool.clone();
    let storage = deps.storage.clone();
    let ml_runtime = fake_ml_runtime();
    let policy = WorkerPolicy::new(
        Duration::from_millis(80),
        Duration::from_secs(1),
        Duration::from_millis(10),
    )?;
    let running = tokio::spawn(async move {
        run_once(
            &pool,
            WorkerHandlers {
                storage: &storage,
                image_processor: &DelayedImageProcessor {
                    delay: Duration::from_millis(150),
                },
                video_processor: &FakeVideoProcessor,
                ml_runtime: &ml_runtime,
                job_kinds: &MEDIA_JOB_KINDS,
            },
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

#[tokio::test]
#[ignore = "requires MIRROR_TEST_DATABASE_URL pointing at a dedicated test database"]
async fn worker_runs_embed_asset_job_to_semantic_index() -> TestResult {
    let deps = storage_test_deps().await?;
    let ml_runtime = fake_ml_runtime();
    let upload_id = create_verified_jpeg_upload(&deps.pool, &deps.storage, "embed.jpg").await?;
    let promoted = promote_verified_upload(&deps.pool, &deps.storage, 1, upload_id).await?;
    sqlx::query!("DELETE FROM jobs").execute(&deps.pool).await?;

    let manifest = valid_model_pack_manifest();
    let pack = install_model_pack(&deps.pool, manifest.clone()).await?;
    record_model_pack_self_test(&deps.pool, pack.model_pack_id, true, None).await?;
    activate_model_pack(&deps.pool, pack.model_pack_id).await?;
    let run = start_model_reindex(&deps.pool, pack.model_pack_id).await?;

    let step = run_once(
        &deps.pool,
        WorkerHandlers {
            storage: &deps.storage,
            image_processor: &FakeImageProcessor,
            video_processor: &FakeVideoProcessor,
            ml_runtime: &ml_runtime,
            job_kinds: &ALL_JOB_KINDS,
        },
        "worker-embed",
        WorkerPolicy::production(),
    )
    .await?;
    let hits = semantic_search(
        &deps.pool,
        1,
        pack.model_pack_id,
        &mirror_backend::models::validate_embedding_output(&manifest, vec![1.0; 768])?,
        10,
    )
    .await?;
    let status: String = sqlx::query_scalar!(
        "SELECT status FROM model_reindex_runs WHERE id = $1",
        run.reindex_run_id
    )
    .fetch_one(&deps.pool)
    .await?;

    assert_eq!(step, WorkerStep::Completed);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].asset_public_id, promoted.asset_id);
    assert_eq!(status, "succeeded");
    Ok(())
}

#[tokio::test]
#[ignore = "requires MIRROR_TEST_DATABASE_URL pointing at a dedicated test database"]
async fn worker_dead_lettered_embed_job_records_reindex_failure() -> TestResult {
    let deps = storage_test_deps().await?;
    let upload_id =
        create_verified_jpeg_upload(&deps.pool, &deps.storage, "dead-embed.jpg").await?;
    promote_verified_upload(&deps.pool, &deps.storage, 1, upload_id).await?;
    sqlx::query!("DELETE FROM jobs").execute(&deps.pool).await?;

    let manifest = valid_model_pack_manifest();
    let pack = install_model_pack(&deps.pool, manifest).await?;
    record_model_pack_self_test(&deps.pool, pack.model_pack_id, true, None).await?;
    activate_model_pack(&deps.pool, pack.model_pack_id).await?;
    let run = start_model_reindex(&deps.pool, pack.model_pack_id).await?;
    sqlx::query!("UPDATE jobs SET max_attempts = 1 WHERE kind = 'embed_asset'")
        .execute(&deps.pool)
        .await?;
    let ml_runtime = MlRuntime::new(Arc::new(FailingImageTextEmbedder), NonZeroUsize::MIN);

    let step = run_once(
        &deps.pool,
        WorkerHandlers {
            storage: &deps.storage,
            image_processor: &FakeImageProcessor,
            video_processor: &FakeVideoProcessor,
            ml_runtime: &ml_runtime,
            job_kinds: &ALL_JOB_KINDS,
        },
        "worker-dead-embed",
        WorkerPolicy::production(),
    )
    .await?;
    let (job_status, run_status, failed_assets): (String, String, i32) = sqlx::query!(
        r#"
        SELECT j.status as job_status, rr.status as run_status, rr.failed_assets
        FROM jobs j
        CROSS JOIN model_reindex_runs rr
        WHERE rr.id = $1
          AND j.kind = 'embed_asset'
        "#,
        run.reindex_run_id
    )
    .map(|r| {
        (
            r.job_status,
            r.run_status,
            r.failed_assets,
        )
    })
    .fetch_one(&deps.pool)
    .await?;
    let asset_error: Option<String> = sqlx::query_scalar(
        "SELECT error_message FROM model_reindex_assets WHERE reindex_run_id = $1",
    )
    .bind(run.reindex_run_id)
    .fetch_one(&deps.pool)
    .await?;

    assert_eq!(step, WorkerStep::Failed);
    assert_eq!(job_status, "dead");
    assert_eq!(run_status, "failed");
    assert_eq!(failed_assets, 1);
    assert_eq!(asset_error.as_deref(), Some("ml runtime unavailable"));
    Ok(())
}

#[tokio::test]
#[ignore = "requires MIRROR_TEST_DATABASE_URL pointing at a dedicated test database"]
async fn worker_records_oversized_embed_input_as_terminal_reindex_failure() -> TestResult {
    let deps = storage_test_deps().await?;
    let bytes = vec![0xff; 9];
    let size_bytes = i64::try_from(bytes.len())?;
    let hash = blake3::hash(&bytes).to_hex().to_string();
    let storage_key = StorageKey::new(format!("originals/blake3/{hash}"))?;
    let original_id = Uuid::now_v7();
    let asset_id = Uuid::now_v7();
    let public_id = Uuid::now_v7();
    deps.storage.write(&storage_key, bytes).await?;
    sqlx::query!(
        r#"
        INSERT INTO originals (id, blake3_hash, storage_key, size_bytes, media_type)
        VALUES ($1, $2, $3, $4, 'image/jpeg')
        "#,
        original_id,
        hash,
        storage_key.as_str(),
        size_bytes
    )
    .execute(&deps.pool)
    .await?;
    sqlx::query!(
        r#"
        INSERT INTO assets (id, public_id, owner_id, original_id)
        VALUES ($1, $2, 1, $3)
        "#,
        asset_id,
        public_id,
        original_id
    )
    .execute(&deps.pool)
    .await?;

    let manifest = valid_model_pack_manifest();
    let pack = install_model_pack(&deps.pool, manifest).await?;
    record_model_pack_self_test(&deps.pool, pack.model_pack_id, true, None).await?;
    activate_model_pack(&deps.pool, pack.model_pack_id).await?;
    let run = start_model_reindex(&deps.pool, pack.model_pack_id).await?;
    let ml_runtime = MlRuntime::with_max_image_bytes(
        Arc::new(support::FakeImageTextEmbedder),
        NonZeroUsize::MIN,
        8,
    );

    let step = run_once(
        &deps.pool,
        WorkerHandlers {
            storage: &deps.storage,
            image_processor: &FakeImageProcessor,
            video_processor: &FakeVideoProcessor,
            ml_runtime: &ml_runtime,
            job_kinds: &ALL_JOB_KINDS,
        },
        "worker-large-embed",
        WorkerPolicy::production(),
    )
    .await?;
    let (job_status, run_status, asset_error): (String, String, Option<String>) = sqlx::query!(
        r#"
        SELECT j.status as job_status, rr.status as run_status, ra.error_message
        FROM jobs j
        JOIN model_reindex_runs rr ON rr.id = $1
        JOIN model_reindex_assets ra ON ra.reindex_run_id = rr.id
        WHERE j.kind = 'embed_asset'
        "#,
        run.reindex_run_id
    )
    .map(|r| (r.job_status, r.run_status, r.error_message))
    .fetch_one(&deps.pool)
    .await?;

    assert_eq!(step, WorkerStep::Completed);
    assert_eq!(job_status, "done");
    assert_eq!(run_status, "failed");
    assert_eq!(
        asset_error.as_deref(),
        Some("ml image exceeds embedding byte limit")
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires MIRROR_TEST_DATABASE_URL pointing at a dedicated test database"]
async fn semantic_text_search_embeds_query_with_active_model_pack() -> TestResult {
    let deps = storage_test_deps().await?;
    let manifest = valid_model_pack_manifest();
    let pack = install_model_pack(&deps.pool, manifest.clone()).await?;
    record_model_pack_self_test(&deps.pool, pack.model_pack_id, true, None).await?;
    activate_model_pack(&deps.pool, pack.model_pack_id).await?;
    let upload_id =
        create_verified_jpeg_upload(&deps.pool, &deps.storage, "text-search.jpg").await?;
    let promoted = promote_verified_upload(&deps.pool, &deps.storage, 1, upload_id).await?;
    let internal_id: Uuid = sqlx::query_scalar!(
        "SELECT id FROM assets WHERE public_id = $1",
        promoted.asset_id
    )
    .fetch_one(&deps.pool)
    .await?;

    mirror_backend::semantic_index::upsert_asset_embedding(
        &deps.pool,
        internal_id,
        pack.model_pack_id,
        &mirror_backend::models::validate_embedding_output(&manifest, vec![1.0; 768])?,
    )
    .await?;

    let ml_runtime = fake_ml_runtime();
    let hits = semantic_text_search(&deps.pool, &ml_runtime, 1, "cat", 10).await?;

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].asset_public_id, promoted.asset_id);
    assert!(hits[0].score.is_finite());
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
    Ok(sqlx::query_scalar!(r#"SELECT count(*) as "count!" FROM asset_metadata "#)
        .fetch_one(pool)
        .await?)
}

async fn derivative_count(pool: &sqlx::PgPool) -> TestResult<i64> {
    Ok(sqlx::query_scalar!(r#"SELECT count(*) as "count!" FROM derivatives"#)
        .fetch_one(pool)
        .await?)
}

const MEDIA_JOB_KINDS: [JobKind; 2] = [JobKind::ExtractMetadata, JobKind::GenerateDerivatives];
const ALL_JOB_KINDS: [JobKind; 3] = [
    JobKind::ExtractMetadata,
    JobKind::GenerateDerivatives,
    JobKind::EmbedAsset,
];

struct FailingImageTextEmbedder;

impl ImageTextEmbedder for FailingImageTextEmbedder {
    fn embed_image(&self, _request: EmbedImageRequest<'_>) -> Result<Vec<f32>, MlError> {
        Err(MlError::RuntimeUnavailable)
    }

    fn embed_text(&self, _request: EmbedTextRequest<'_>) -> Result<Vec<f32>, MlError> {
        Err(MlError::RuntimeUnavailable)
    }
}
