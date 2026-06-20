use mirror_backend::{
    assets::{PromoteError, detect_original_orphan, promote_verified_upload},
    storage::StorageKey,
    uploads::{
        CreateUploadInput, UPLOAD_PART_SIZE_BYTES, complete_upload, create_upload, put_part,
    },
};

mod support;
use support::{
    TestResult, asset_count, create_verified_jpeg_upload, job_count, jpeg_bytes, original_count,
    storage_test_deps,
};

#[tokio::test]
#[ignore = "requires MIRROR_TEST_DATABASE_URL pointing at a dedicated test database"]
async fn promotion_deduplicates_originals_but_keeps_distinct_assets() -> TestResult {
    let deps = storage_test_deps().await?;
    let first_upload = create_verified_jpeg_upload(&deps.pool, &deps.storage, "first.jpg").await?;
    let second_upload =
        create_verified_jpeg_upload(&deps.pool, &deps.storage, "second.jpg").await?;

    let first = promote_verified_upload(&deps.pool, &deps.storage, 1, first_upload).await?;
    let first_again = promote_verified_upload(&deps.pool, &deps.storage, 1, first_upload).await?;
    let second = promote_verified_upload(&deps.pool, &deps.storage, 1, second_upload).await?;

    assert_eq!(first, first_again);
    assert_ne!(first.asset_id, second.asset_id);
    let returned_id_is_public = sqlx::query_scalar!(
        "SELECT EXISTS (SELECT 1 FROM assets WHERE public_id = $1)",
        first.asset_id
    )
    .fetch_one(&deps.pool)
    .await?;
    assert!(returned_id_is_public.unwrap_or(false));
    assert_eq!(original_count(&deps.pool).await?, 1);
    assert_eq!(asset_count(&deps.pool).await?, 2);
    assert_eq!(job_count(&deps.pool).await?, 4);
    assert_eq!(
        asset_status(&deps.pool, first.asset_id).await?,
        "original_available"
    );
    assert_eq!(upload_status(&deps.pool, first_upload).await?, "completed");

    Ok(())
}

#[tokio::test]
#[ignore = "requires MIRROR_TEST_DATABASE_URL pointing at a dedicated test database"]
async fn concurrent_promotion_of_one_upload_is_idempotent() -> TestResult {
    let deps = storage_test_deps().await?;
    let upload_id = create_verified_jpeg_upload(&deps.pool, &deps.storage, "parallel.jpg").await?;

    let (first, second) = tokio::join!(
        promote_verified_upload(&deps.pool, &deps.storage, 1, upload_id),
        promote_verified_upload(&deps.pool, &deps.storage, 1, upload_id),
    );
    let first = first?;
    let second = second?;

    assert_eq!(first, second);
    assert_eq!(original_count(&deps.pool).await?, 1);
    assert_eq!(asset_count(&deps.pool).await?, 1);
    assert_eq!(job_count(&deps.pool).await?, 2);
    assert_eq!(upload_status(&deps.pool, upload_id).await?, "completed");

    Ok(())
}

#[tokio::test]
#[ignore = "requires MIRROR_TEST_DATABASE_URL pointing at a dedicated test database"]
async fn promotion_writes_multipart_upload_to_original_storage() -> TestResult {
    let deps = storage_test_deps().await?;
    let mut bytes = vec![0_u8; UPLOAD_PART_SIZE_BYTES + 17];
    bytes[..3].copy_from_slice(&[0xff, 0xd8, 0xff]);
    let expected_blake3 = blake3::hash(&bytes).to_hex().to_string();
    let upload = create_upload(
        &deps.pool,
        CreateUploadInput {
            owner_id: 1,
            original_filename: "multipart.jpg".to_owned(),
            expected_size: i64::try_from(bytes.len())?,
            expected_blake3: expected_blake3.clone(),
            media_type: "image/jpeg".to_owned(),
            client_upload_key: None,
        },
    )
    .await?;

    put_part(
        &deps.pool,
        &deps.storage,
        1,
        upload.upload_id,
        0,
        bytes[..UPLOAD_PART_SIZE_BYTES].to_vec(),
    )
    .await?;
    put_part(
        &deps.pool,
        &deps.storage,
        1,
        upload.upload_id,
        1,
        bytes[UPLOAD_PART_SIZE_BYTES..].to_vec(),
    )
    .await?;
    complete_upload(&deps.pool, &deps.storage, 1, upload.upload_id).await?;

    promote_verified_upload(&deps.pool, &deps.storage, 1, upload.upload_id).await?;

    let final_key = StorageKey::original_blake3(&expected_blake3)?;
    assert_eq!(deps.storage.read(&final_key).await?, bytes);
    assert_eq!(original_count(&deps.pool).await?, 1);
    assert_eq!(asset_count(&deps.pool).await?, 1);
    assert_eq!(
        upload_status(&deps.pool, upload.upload_id).await?,
        "completed"
    );

    Ok(())
}

#[tokio::test]
#[ignore = "requires MIRROR_TEST_DATABASE_URL pointing at a dedicated test database"]
async fn promotion_adopts_existing_final_object_when_staging_was_cleaned() -> TestResult {
    let deps = storage_test_deps().await?;
    let bytes = jpeg_bytes();
    let expected_blake3 = blake3::hash(&bytes).to_hex().to_string();
    let upload_id = create_verified_jpeg_upload(&deps.pool, &deps.storage, "adopt.jpg").await?;
    let final_key = StorageKey::original_blake3(&expected_blake3)?;
    let staged_key = StorageKey::staging_upload(upload_id, "part-00000000")?;
    deps.storage.write(&final_key, bytes).await?;
    deps.storage.delete(&staged_key).await?;

    let promoted = promote_verified_upload(&deps.pool, &deps.storage, 1, upload_id).await?;

    assert_eq!(
        asset_status(&deps.pool, promoted.asset_id).await?,
        "original_available"
    );
    assert_eq!(upload_status(&deps.pool, upload_id).await?, "completed");
    assert_eq!(original_count(&deps.pool).await?, 1);
    assert_eq!(asset_count(&deps.pool).await?, 1);
    Ok(())
}

#[tokio::test]
#[ignore = "requires MIRROR_TEST_DATABASE_URL pointing at a dedicated test database"]
async fn unverified_upload_cannot_create_assets_or_jobs() -> TestResult {
    let deps = storage_test_deps().await?;
    let bytes = jpeg_bytes();
    let upload = create_upload(
        &deps.pool,
        CreateUploadInput {
            owner_id: 1,
            original_filename: "open.jpg".to_owned(),
            expected_size: i64::try_from(bytes.len())?,
            expected_blake3: blake3::hash(&bytes).to_hex().to_string(),
            media_type: "image/jpeg".to_owned(),
            client_upload_key: None,
        },
    )
    .await?;
    put_part(&deps.pool, &deps.storage, 1, upload.upload_id, 0, bytes).await?;

    let result = promote_verified_upload(&deps.pool, &deps.storage, 1, upload.upload_id).await;

    assert!(matches!(result, Err(PromoteError::UploadNotVerified)));
    assert_eq!(original_count(&deps.pool).await?, 0);
    assert_eq!(asset_count(&deps.pool).await?, 0);
    assert_eq!(job_count(&deps.pool).await?, 0);

    Ok(())
}

async fn asset_status(pool: &sqlx::PgPool, asset_public_id: uuid::Uuid) -> TestResult<String> {
    Ok(sqlx::query_scalar!(
        "SELECT status FROM assets WHERE public_id = $1",
        asset_public_id
    )
    .fetch_one(pool)
    .await?)
}

async fn upload_status(pool: &sqlx::PgPool, upload_id: uuid::Uuid) -> TestResult<String> {
    Ok(sqlx::query_scalar!(
        "SELECT status FROM upload_sessions WHERE id = $1",
        upload_id
    )
    .fetch_one(pool)
    .await?)
}

#[tokio::test]
#[ignore = "requires MIRROR_TEST_DATABASE_URL pointing at a dedicated test database"]
async fn orphan_detection_finds_promoted_object_without_original_row() -> TestResult {
    let deps = storage_test_deps().await?;
    let bytes = jpeg_bytes();
    let hash = blake3::hash(&bytes).to_hex().to_string();
    let key = StorageKey::original_blake3(&hash)?;

    deps.storage.write(&key, bytes).await?;

    assert!(detect_original_orphan(&deps.pool, &deps.storage, &hash).await?);

    Ok(())
}
