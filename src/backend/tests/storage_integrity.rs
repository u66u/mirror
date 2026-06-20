use std::io;

use mirror_backend::{
    integrity::{
        IntegrityError, enqueue_integrity_scan, remediate_original_orphans, run_integrity_job,
        scan_durable_objects, scan_original_storage,
    },
    jobs::{self, JobKind},
    storage::StorageKey,
};
use serde_json::Value;
use uuid::Uuid;

mod support;
use support::{TestResult, storage_test_deps};

#[tokio::test]
#[ignore = "requires MIRROR_TEST_DATABASE_URL pointing at a dedicated test database"]
async fn scan_reports_recursively_listed_orphan_without_deleting_it() -> TestResult {
    let deps = storage_test_deps().await?;
    let bytes = b"orphan original".to_vec();
    let hash = blake3::hash(&bytes).to_hex().to_string();
    let key = StorageKey::original_blake3(&hash)?;
    deps.storage.write(&key, bytes).await?;

    let report = scan_original_storage(&deps.pool, &deps.storage)
        .await
        .map_err(io::Error::other)?;

    assert_eq!(report.orphan_objects, vec![key.clone()]);
    assert!(report.missing_objects.is_empty());
    assert!(deps.storage.exists(&key).await?);

    Ok(())
}

#[tokio::test]
#[ignore = "requires MIRROR_TEST_DATABASE_URL pointing at a dedicated test database"]
async fn scan_reports_database_original_with_missing_object() -> TestResult {
    let deps = storage_test_deps().await?;
    let hash = blake3::hash(b"missing original").to_hex().to_string();
    let key = StorageKey::original_blake3(&hash)?;
    insert_original(&deps.pool, &hash, key.as_str()).await?;

    let report = scan_original_storage(&deps.pool, &deps.storage)
        .await
        .map_err(io::Error::other)?;

    assert!(report.orphan_objects.is_empty());
    assert_eq!(report.missing_objects.len(), 1);
    assert_eq!(report.missing_objects[0].blake3_hash, hash);
    assert_eq!(report.missing_objects[0].storage_key, key.as_str());

    Ok(())
}

#[tokio::test]
#[ignore = "requires MIRROR_TEST_DATABASE_URL pointing at a dedicated test database"]
async fn remediation_deletes_only_selected_orphans() -> TestResult {
    let deps = storage_test_deps().await?;
    let selected = original_key_for(b"selected orphan")?;
    let unselected = original_key_for(b"unselected orphan")?;
    deps.storage.write(&selected, b"selected".to_vec()).await?;
    deps.storage
        .write(&unselected, b"unselected".to_vec())
        .await?;

    let remediation =
        remediate_original_orphans(&deps.pool, &deps.storage, std::slice::from_ref(&selected))
            .await
            .map_err(io::Error::other)?;

    assert_eq!(remediation.deleted_objects, vec![selected.clone()]);
    assert!(remediation.retained_db_backed_objects.is_empty());
    assert!(remediation.already_missing_objects.is_empty());
    assert!(!deps.storage.exists(&selected).await?);
    assert!(deps.storage.exists(&unselected).await?);

    Ok(())
}

#[tokio::test]
#[ignore = "requires MIRROR_TEST_DATABASE_URL pointing at a dedicated test database"]
async fn remediation_rechecks_database_before_delete() -> TestResult {
    let deps = storage_test_deps().await?;
    let bytes = b"racing original".to_vec();
    let hash = blake3::hash(&bytes).to_hex().to_string();
    let key = StorageKey::original_blake3(&hash)?;
    deps.storage.write(&key, bytes).await?;

    let scan = scan_original_storage(&deps.pool, &deps.storage)
        .await
        .map_err(io::Error::other)?;
    assert_eq!(scan.orphan_objects, vec![key.clone()]);

    insert_original(&deps.pool, &hash, key.as_str()).await?;
    let remediation = remediate_original_orphans(&deps.pool, &deps.storage, &scan.orphan_objects)
        .await
        .map_err(io::Error::other)?;

    assert!(remediation.deleted_objects.is_empty());
    assert_eq!(remediation.retained_db_backed_objects, vec![key.clone()]);
    assert!(remediation.already_missing_objects.is_empty());
    assert!(deps.storage.exists(&key).await?);

    Ok(())
}

#[tokio::test]
#[ignore = "requires MIRROR_TEST_DATABASE_URL pointing at a dedicated test database"]
async fn remediation_rejects_non_content_addressed_original_descendant() -> TestResult {
    let deps = storage_test_deps().await?;
    let key = StorageKey::new("originals/blake3/arbitrary/object")?;
    deps.storage.write(&key, b"do not delete".to_vec()).await?;

    let result =
        remediate_original_orphans(&deps.pool, &deps.storage, std::slice::from_ref(&key)).await;

    assert!(matches!(
        result,
        Err(IntegrityError::InvalidRemediationKey(ref invalid_key))
            if invalid_key == key.as_str()
    ));
    assert!(deps.storage.exists(&key).await?);

    Ok(())
}

#[tokio::test]
#[ignore = "requires MIRROR_TEST_DATABASE_URL pointing at a dedicated test database"]
async fn durable_object_scan_reports_missing_corrupt_and_healthy_objects() -> TestResult {
    let deps = storage_test_deps().await?;
    let healthy_original = insert_original_object(&deps, b"healthy original", true).await?;
    let missing_original = insert_original_object(&deps, b"missing original", false).await?;
    let corrupt_original = insert_corrupt_original_object(&deps).await?;
    let derivative_source = insert_original_object(&deps, b"derivative source", true).await?;
    let corrupt_derivative_source =
        insert_original_object(&deps, b"corrupt derivative source", true).await?;
    let asset_id = insert_asset_for_original(&deps.pool, healthy_original.original_id).await?;
    let missing_derivative_asset_id =
        insert_asset_for_original(&deps.pool, derivative_source.original_id).await?;
    let second_asset_id =
        insert_asset_for_original(&deps.pool, corrupt_derivative_source.original_id).await?;
    let healthy_derivative = insert_derivative_object(
        &deps,
        asset_id,
        &healthy_original.hash,
        "thumbnail",
        b"ok",
        true,
    )
    .await?;
    let missing_derivative = insert_derivative_object(
        &deps,
        missing_derivative_asset_id,
        &derivative_source.hash,
        "preview",
        b"missing derivative",
        false,
    )
    .await?;
    let corrupt_derivative =
        insert_corrupt_derivative_object(&deps, second_asset_id, &corrupt_derivative_source.hash)
            .await?;

    let report = scan_durable_objects(&deps.pool, &deps.storage)
        .await
        .map_err(io::Error::other)?;

    assert_eq!(report.missing_originals.len(), 1);
    assert_eq!(
        report.missing_originals[0].storage_key,
        missing_original.key.as_str()
    );
    assert_eq!(report.corrupt_originals.len(), 1);
    assert_eq!(
        report.corrupt_originals[0].storage_key,
        corrupt_original.key.as_str()
    );
    assert_eq!(report.missing_derivatives.len(), 1);
    assert_eq!(
        report.missing_derivatives[0].storage_key,
        missing_derivative.key.as_str()
    );
    assert_eq!(report.corrupt_derivatives.len(), 1);
    assert_eq!(
        report.corrupt_derivatives[0].storage_key,
        corrupt_derivative.key.as_str()
    );
    assert!(deps.storage.exists(&healthy_original.key).await?);
    assert!(deps.storage.exists(&healthy_derivative.key).await?);
    assert!(deps.storage.exists(&corrupt_original.key).await?);
    assert!(deps.storage.exists(&corrupt_derivative.key).await?);

    Ok(())
}

#[tokio::test]
#[ignore = "requires MIRROR_TEST_DATABASE_URL pointing at a dedicated test database"]
async fn integrity_scan_job_persists_report_without_deleting_objects() -> TestResult {
    let deps = storage_test_deps().await?;
    let healthy_original = insert_original_object(&deps, b"job healthy", true).await?;
    let corrupt_original = insert_corrupt_original_object(&deps).await?;
    let corrupt_original_asset_id =
        insert_asset_for_original(&deps.pool, corrupt_original.original_id).await?;
    let asset_id = insert_asset_for_original(&deps.pool, healthy_original.original_id).await?;
    let corrupt_derivative =
        insert_corrupt_derivative_object(&deps, asset_id, &healthy_original.hash).await?;
    let run = enqueue_integrity_scan(&deps.pool)
        .await
        .map_err(io::Error::other)?;
    let job = jobs::lease_next_for_kinds(
        &deps.pool,
        "integrity-test-worker",
        time::OffsetDateTime::now_utc() - time::Duration::hours(1),
        &[JobKind::IntegrityScan],
    )
    .await?
    .ok_or_else(|| io::Error::other("integrity job not leased"))?;

    run_integrity_job(&deps.pool, &deps.storage, &job)
        .await
        .map_err(io::Error::other)?;
    jobs::complete(&deps.pool, job.id, "integrity-test-worker").await?;

    let row = sqlx::query!(
        r#"
        SELECT status, missing_originals, corrupt_originals, missing_derivatives, corrupt_derivatives
        FROM integrity_scan_runs
        WHERE id = $1
        "#,
        run.integrity_scan_run_id
    )
    .fetch_one(&deps.pool)
    .await?;
    assert_eq!(row.status, "succeeded");
    assert_eq!(json_len(&row.missing_originals), 0);
    assert_eq!(json_len(&row.corrupt_originals), 1);
    assert_eq!(json_len(&row.missing_derivatives), 0);
    assert_eq!(json_len(&row.corrupt_derivatives), 1);
    assert!(deps.storage.exists(&corrupt_original.key).await?);
    assert!(deps.storage.exists(&corrupt_derivative.key).await?);
    assert_eq!(
        asset_status(&deps.pool, corrupt_original_asset_id).await?,
        "corrupt"
    );

    Ok(())
}

fn original_key_for(bytes: &[u8]) -> TestResult<StorageKey> {
    Ok(StorageKey::original_blake3(
        blake3::hash(bytes).to_hex().as_ref(),
    )?)
}

async fn insert_original(pool: &sqlx::PgPool, hash: &str, storage_key: &str) -> TestResult {
    sqlx::query!(
        r#"
        INSERT INTO originals (id, blake3_hash, storage_key, size_bytes, media_type)
        VALUES ($1, $2, $3, 1, 'image/jpeg')
        "#,
        Uuid::now_v7(),
        hash,
        storage_key
    )
    .execute(pool)
    .await?;
    Ok(())
}

struct InsertedOriginal {
    original_id: Uuid,
    hash: String,
    key: StorageKey,
}

struct InsertedDerivative {
    key: StorageKey,
}

async fn insert_original_object(
    deps: &support::StorageTestDeps,
    bytes: &[u8],
    write_object: bool,
) -> TestResult<InsertedOriginal> {
    let hash = blake3::hash(bytes).to_hex().to_string();
    let key = StorageKey::original_blake3(&hash)?;
    if write_object {
        deps.storage.write(&key, bytes.to_vec()).await?;
    }
    let original_id = Uuid::now_v7();
    sqlx::query!(
        r#"
        INSERT INTO originals (id, blake3_hash, storage_key, size_bytes, media_type)
        VALUES ($1, $2, $3, $4, 'image/jpeg')
        "#,
        original_id,
        hash,
        key.as_str(),
        i64::try_from(bytes.len())?
    )
    .execute(&deps.pool)
    .await?;
    Ok(InsertedOriginal {
        original_id,
        hash,
        key,
    })
}

async fn insert_corrupt_original_object(
    deps: &support::StorageTestDeps,
) -> TestResult<InsertedOriginal> {
    let expected = b"expected original";
    let actual = b"corrupt original";
    let inserted = insert_original_object(deps, expected, false).await?;
    deps.storage.write(&inserted.key, actual.to_vec()).await?;
    Ok(inserted)
}

async fn insert_asset_for_original(pool: &sqlx::PgPool, original_id: Uuid) -> TestResult<Uuid> {
    let asset_id = Uuid::now_v7();
    sqlx::query!(
        r#"
        INSERT INTO assets (id, public_id, owner_id, original_id)
        VALUES ($1, $2, 1, $3)
        "#,
        asset_id,
        Uuid::now_v7(),
        original_id
    )
    .execute(pool)
    .await?;
    Ok(asset_id)
}

async fn asset_status(pool: &sqlx::PgPool, asset_id: Uuid) -> TestResult<String> {
    Ok(
        sqlx::query_scalar!("SELECT status FROM assets WHERE id = $1", asset_id)
            .fetch_one(pool)
            .await?,
    )
}

async fn insert_derivative_object(
    deps: &support::StorageTestDeps,
    asset_id: Uuid,
    source_hash: &str,
    kind: &str,
    bytes: &[u8],
    write_object: bool,
) -> TestResult<InsertedDerivative> {
    let key = StorageKey::derivative(source_hash, kind, "webp", "integrity")?;
    if write_object {
        deps.storage.write(&key, bytes.to_vec()).await?;
    }
    sqlx::query!(
        r#"
        INSERT INTO derivatives (
            id,
            asset_id,
            kind,
            format,
            generator_version,
            source_blake3,
            blake3_hash,
            storage_key,
            width,
            height,
            size_bytes
        )
        VALUES ($1, $2, $3, 'webp', 'integrity', $4, $5, $6, 1, 1, $7)
        "#,
        Uuid::now_v7(),
        asset_id,
        kind,
        source_hash,
        blake3::hash(bytes).to_hex().to_string(),
        key.as_str(),
        i64::try_from(bytes.len())?
    )
    .execute(&deps.pool)
    .await?;
    Ok(InsertedDerivative { key })
}

async fn insert_corrupt_derivative_object(
    deps: &support::StorageTestDeps,
    asset_id: Uuid,
    source_hash: &str,
) -> TestResult<InsertedDerivative> {
    let inserted =
        insert_derivative_object(deps, asset_id, source_hash, "preview", b"expected", false)
            .await?;
    deps.storage
        .write(&inserted.key, b"actual".to_vec())
        .await?;
    Ok(inserted)
}

fn json_len(value: &Value) -> usize {
    value.as_array().map(Vec::len).unwrap_or_default()
}
