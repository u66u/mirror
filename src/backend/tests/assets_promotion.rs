use mirror_backend::{
    assets::{PromoteError, detect_original_orphan, promote_verified_upload},
    storage::StorageKey,
    uploads::{CreateUploadInput, create_upload, put_part},
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
    let returned_id_is_public =
        sqlx::query_scalar::<_, bool>("SELECT EXISTS (SELECT 1 FROM assets WHERE public_id = $1)")
            .bind(first.asset_id)
            .fetch_one(&deps.pool)
            .await?;
    assert!(returned_id_is_public);
    assert_eq!(original_count(&deps.pool).await?, 1);
    assert_eq!(asset_count(&deps.pool).await?, 2);
    assert_eq!(job_count(&deps.pool).await?, 4);

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
