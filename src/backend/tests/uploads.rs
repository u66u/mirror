use mirror_backend::uploads::{
    CreateUploadInput, UploadError, UploadStatus, cancel_upload, complete_upload, create_upload,
    get_upload, put_part,
};

mod support;
use support::{TestResult, jpeg_bytes, storage_test_deps};

#[tokio::test]
#[ignore = "requires MIRROR_TEST_DATABASE_URL pointing at a dedicated test database"]
async fn upload_resume_and_complete_verify_size_hash_and_media_signature() -> TestResult {
    let deps = storage_test_deps().await?;
    let bytes = jpeg_bytes();
    let hash = blake3::hash(&bytes).to_hex().to_string();

    let created = create_upload(
        &deps.pool,
        CreateUploadInput {
            owner_id: 1,
            original_filename: "photo.jpg".to_owned(),
            expected_size: i64::try_from(bytes.len())?,
            expected_blake3: hash,
            media_type: "image/jpeg".to_owned(),
        },
    )
    .await?;

    put_part(
        &deps.pool,
        &deps.storage,
        1,
        created.upload_id,
        1,
        bytes[5..].to_vec(),
    )
    .await?;
    put_part(
        &deps.pool,
        &deps.storage,
        1,
        created.upload_id,
        0,
        bytes[..5].to_vec(),
    )
    .await?;

    let resume = get_upload(&deps.pool, 1, created.upload_id).await?;
    assert_eq!(resume.committed_parts, vec![0, 1]);

    let completed = complete_upload(&deps.pool, &deps.storage, 1, created.upload_id).await?;
    assert_eq!(completed.status, UploadStatus::Verified);

    let completed_again = complete_upload(&deps.pool, &deps.storage, 1, created.upload_id).await?;
    assert_eq!(completed_again.status, UploadStatus::Verified);

    Ok(())
}

#[tokio::test]
#[ignore = "requires MIRROR_TEST_DATABASE_URL pointing at a dedicated test database"]
async fn failed_upload_verification_does_not_mark_session_verified() -> TestResult {
    let deps = storage_test_deps().await?;
    let bytes = jpeg_bytes();

    let created = create_upload(
        &deps.pool,
        CreateUploadInput {
            owner_id: 1,
            original_filename: "photo.jpg".to_owned(),
            expected_size: i64::try_from(bytes.len())?,
            expected_blake3: blake3::hash(b"different").to_hex().to_string(),
            media_type: "image/jpeg".to_owned(),
        },
    )
    .await?;

    put_part(&deps.pool, &deps.storage, 1, created.upload_id, 0, bytes).await?;
    let result = complete_upload(&deps.pool, &deps.storage, 1, created.upload_id).await;

    assert!(matches!(result, Err(UploadError::VerificationFailed)));
    assert_eq!(
        get_upload(&deps.pool, 1, created.upload_id).await?.status,
        UploadStatus::Open
    );

    Ok(())
}

#[tokio::test]
#[ignore = "requires MIRROR_TEST_DATABASE_URL pointing at a dedicated test database"]
async fn cancelled_upload_rejects_new_parts() -> TestResult {
    let deps = storage_test_deps().await?;
    let bytes = jpeg_bytes();
    let created = create_upload(
        &deps.pool,
        CreateUploadInput {
            owner_id: 1,
            original_filename: "photo.jpg".to_owned(),
            expected_size: i64::try_from(bytes.len())?,
            expected_blake3: blake3::hash(&bytes).to_hex().to_string(),
            media_type: "image/jpeg".to_owned(),
        },
    )
    .await?;

    cancel_upload(&deps.pool, 1, created.upload_id).await?;
    let result = put_part(&deps.pool, &deps.storage, 1, created.upload_id, 0, bytes).await;

    assert!(matches!(result, Err(UploadError::NotOpen)));

    Ok(())
}
