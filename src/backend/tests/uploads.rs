use mirror_backend::storage::StorageKey;
use mirror_backend::uploads::{
    CreateUploadInput, UPLOAD_PART_SIZE_BYTES, UploadError, UploadStatus, cancel_upload,
    complete_upload, create_upload, get_upload, put_part,
};
use uuid::Uuid;

mod support;
use support::{TestResult, jpeg_bytes, storage_test_deps};

#[tokio::test]
#[ignore = "requires MIRROR_TEST_DATABASE_URL pointing at a dedicated test database"]
async fn upload_resume_and_complete_verify_size_hash_and_media_signature() -> TestResult {
    let deps = storage_test_deps().await?;
    let mut bytes = vec![0_u8; UPLOAD_PART_SIZE_BYTES + 17];
    bytes[..3].copy_from_slice(&[0xff, 0xd8, 0xff]);
    let hash = blake3::hash(&bytes).to_hex().to_string();

    let created = create_upload(
        &deps.pool,
        CreateUploadInput {
            owner_id: 1,
            original_filename: "photo.jpg".to_owned(),
            expected_size: i64::try_from(bytes.len())?,
            expected_blake3: hash,
            media_type: "image/jpeg".to_owned(),
            client_upload_key: None,
        },
    )
    .await?;

    put_part(
        &deps.pool,
        &deps.storage,
        1,
        created.upload_id,
        1,
        bytes[UPLOAD_PART_SIZE_BYTES..].to_vec(),
    )
    .await?;
    put_part(
        &deps.pool,
        &deps.storage,
        1,
        created.upload_id,
        0,
        bytes[..UPLOAD_PART_SIZE_BYTES].to_vec(),
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
async fn invalid_part_framing_creates_no_part_rows_or_storage_objects() -> TestResult {
    let deps = storage_test_deps().await?;
    let created = create_upload(
        &deps.pool,
        CreateUploadInput {
            owner_id: 1,
            original_filename: "framing.jpg".to_owned(),
            expected_size: i64::try_from(UPLOAD_PART_SIZE_BYTES + 17)?,
            expected_blake3: "0".repeat(64),
            media_type: "image/jpeg".to_owned(),
            client_upload_key: None,
        },
    )
    .await?;

    let short_non_final = put_part(
        &deps.pool,
        &deps.storage,
        1,
        created.upload_id,
        0,
        vec![0_u8; UPLOAD_PART_SIZE_BYTES - 1],
    )
    .await;
    assert!(matches!(
        short_non_final,
        Err(UploadError::PartLengthMismatch)
    ));

    let short_final = put_part(
        &deps.pool,
        &deps.storage,
        1,
        created.upload_id,
        1,
        vec![0_u8; 16],
    )
    .await;
    assert!(matches!(short_final, Err(UploadError::PartLengthMismatch)));

    let out_of_range = put_part(
        &deps.pool,
        &deps.storage,
        1,
        created.upload_id,
        2,
        vec![0_u8; 1],
    )
    .await;
    assert!(matches!(out_of_range, Err(UploadError::PartOutOfRange)));

    let part_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM upload_parts WHERE upload_id = $1")
            .bind(created.upload_id)
            .fetch_one(&deps.pool)
            .await?;
    assert_eq!(part_count, 0);

    for part_index in 0..=2 {
        let key = StorageKey::staging_upload(created.upload_id, &format!("part-{part_index:08}"))?;
        assert!(!deps.storage.exists(&key).await?);
    }

    Ok(())
}

#[tokio::test]
#[ignore = "requires MIRROR_TEST_DATABASE_URL pointing at a dedicated test database"]
async fn upload_creation_retry_is_idempotent_and_rejects_key_reuse() -> TestResult {
    let deps = storage_test_deps().await?;
    let bytes = jpeg_bytes();
    let client_upload_key = Uuid::new_v4();
    let expected_blake3 = blake3::hash(&bytes).to_hex().to_string();
    let expected_size = i64::try_from(bytes.len())?;
    let input = || CreateUploadInput {
        owner_id: 1,
        original_filename: "retry.jpg".to_owned(),
        expected_size,
        expected_blake3: expected_blake3.clone(),
        media_type: "image/jpeg".to_owned(),
        client_upload_key: Some(client_upload_key),
    };

    let first = create_upload(&deps.pool, input()).await?;
    let retried = create_upload(&deps.pool, input()).await?;
    assert_eq!(first.upload_id, retried.upload_id);

    let mismatched = create_upload(
        &deps.pool,
        CreateUploadInput {
            original_filename: "different.jpg".to_owned(),
            ..input()
        },
    )
    .await;
    assert!(matches!(mismatched, Err(UploadError::InvalidInput)));

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
            client_upload_key: None,
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
            client_upload_key: None,
        },
    )
    .await?;

    cancel_upload(&deps.pool, 1, created.upload_id).await?;
    let result = put_part(&deps.pool, &deps.storage, 1, created.upload_id, 0, bytes).await;

    assert!(matches!(result, Err(UploadError::NotOpen)));

    Ok(())
}
