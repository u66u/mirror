use mirror_backend::storage::{StorageKey, StorageKeyError};
use uuid::Uuid;

mod support;
use support::TestResult;

#[test]
fn storage_keys_reject_absolute_paths_parent_traversal_and_backslashes() {
    assert!(matches!(
        StorageKey::new("/absolute"),
        Err(StorageKeyError::UnsafePath)
    ));
    assert!(matches!(
        StorageKey::new("safe/../escape"),
        Err(StorageKeyError::UnsafePath)
    ));
    assert!(matches!(
        StorageKey::new("safe\\escape"),
        Err(StorageKeyError::UnsafePath)
    ));
}

#[test]
fn original_key_is_content_addressed_by_blake3_hash() -> TestResult {
    let hash = blake3::hash(b"photo bytes").to_hex().to_string();
    let key = StorageKey::original_blake3(&hash)?;

    assert_eq!(
        key.as_str(),
        format!("originals/blake3/{}/{}/{}", &hash[0..2], &hash[2..4], hash)
    );

    Ok(())
}

#[test]
fn generated_staging_keys_do_not_accept_user_path_shape() {
    let upload_id = Uuid::now_v7();

    assert!(StorageKey::staging_upload(upload_id, "part-0001").is_ok());
    assert!(matches!(
        StorageKey::staging_upload(upload_id, "../part-0001"),
        Err(StorageKeyError::UnsafePath)
    ));
}

#[test]
fn derivative_keys_reject_untrusted_generator_segments() -> TestResult {
    let hash = blake3::hash(b"photo bytes").to_hex().to_string();
    let key = StorageKey::derivative(&hash, "thumbnail", "webp", "media-v1")?;

    assert_eq!(
        key.as_str(),
        format!(
            "derivatives/media-v1/thumbnail/webp/{}/{}/{}.webp",
            &hash[0..2],
            &hash[2..4],
            hash
        )
    );
    assert!(matches!(
        StorageKey::derivative(&hash, "../thumbnail", "webp", "media-v1"),
        Err(StorageKeyError::UnsafePath)
    ));
    assert!(matches!(
        StorageKey::derivative(&hash, "thumbnail", "webp", "media/v1"),
        Err(StorageKeyError::UnsafePath)
    ));

    Ok(())
}
