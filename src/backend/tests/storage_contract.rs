use mirror_backend::storage::{ObjectStorage, StorageKey};
use tempfile::TempDir;
use uuid::Uuid;

mod support;
use support::TestResult;

#[tokio::test]
async fn local_storage_contract_put_get_list_delete_and_promote() -> TestResult {
    let temp_dir = TempDir::new()?;
    let storage = ObjectStorage::local(temp_dir.path())?;
    let upload_id = Uuid::now_v7();
    let staged = StorageKey::staging_upload(upload_id, "part-0001")?;
    let prefix = StorageKey::staging_upload_prefix(upload_id);
    let hash = blake3::hash(b"photo bytes").to_hex().to_string();
    let final_key = StorageKey::original_blake3(&hash)?;

    storage.write(&staged, b"photo bytes".to_vec()).await?;

    assert_eq!(storage.read(&staged).await?, b"photo bytes");
    assert_eq!(
        storage.list(&prefix).await?,
        vec![staged.as_str().to_owned()]
    );

    storage.promote(&staged, &final_key).await?;

    assert!(!storage.exists(&staged).await?);
    assert_eq!(storage.read(&final_key).await?, b"photo bytes");

    storage.delete(&final_key).await?;

    assert!(!storage.exists(&final_key).await?);

    Ok(())
}

#[tokio::test]
async fn bounded_storage_copy_rejects_oversize_without_partial_file() -> TestResult {
    let temp_dir = TempDir::new()?;
    let storage = ObjectStorage::local(temp_dir.path().join("objects"))?;
    let key = StorageKey::staging_upload(Uuid::now_v7(), "part-0001")?;
    let destination = temp_dir.path().join("staged-media");
    storage.write(&key, b"0123456789".to_vec()).await?;

    let result = storage.copy_to_path_bounded(&key, &destination, 9).await;

    assert!(matches!(
        result,
        Err(mirror_backend::storage::StorageError::ObjectTooLarge)
    ));
    assert!(!destination.exists());
    Ok(())
}
