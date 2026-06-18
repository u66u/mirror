use std::io;

use mirror_backend::{
    integrity::{IntegrityError, remediate_original_orphans, scan_original_storage},
    storage::StorageKey,
};
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
