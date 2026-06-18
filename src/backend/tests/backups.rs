use mirror_backend::{
    backups::{
        BackupError, CreateBackupRunInput, backup_manifest_summary, create_backup_run,
        durable_storage_manifest, mark_backup_succeeded, mark_restore_check, postgres_dump_plan,
        postgres_restore_plan, restic_backup_plan, restic_restore_plan, restic_retention_plan,
    },
    storage::{ObjectStorage, StorageKey},
};
use tempfile::TempDir;
use uuid::Uuid;

mod support;
use support::{TestResult, fresh_owner_pool};

#[tokio::test]
async fn durable_storage_manifest_includes_only_durable_namespaces() -> TestResult {
    let temp_dir = TempDir::new()?;
    let storage = ObjectStorage::local(temp_dir.path())?;
    let original = StorageKey::original_blake3(
        "1111111111111111111111111111111111111111111111111111111111111111",
    )?;
    let derivative = StorageKey::derivative(
        "2222222222222222222222222222222222222222222222222222222222222222",
        "thumbnail",
        "webp",
        "media-v1-image-webp-1",
    )?;
    let model_file = StorageKey::model_pack_file(Uuid::now_v7(), "models/image_encoder.onnx")?;
    let staged = StorageKey::staging_upload(Uuid::now_v7(), "part-00000000")?;

    storage.write(&original, b"original".to_vec()).await?;
    storage.write(&derivative, b"derivative".to_vec()).await?;
    storage.write(&model_file, b"model".to_vec()).await?;
    storage.write(&staged, b"staged".to_vec()).await?;

    let manifest = durable_storage_manifest(&storage).await?;
    let keys = manifest
        .objects
        .iter()
        .map(|object| object.storage_key.as_str())
        .collect::<Vec<_>>();

    assert_eq!(
        manifest.manifest_version,
        "mirror-durable-storage-backup-v1"
    );
    assert_eq!(
        keys,
        vec![derivative.as_str(), model_file.as_str(), original.as_str()]
    );

    Ok(())
}

#[test]
fn postgres_dump_plan_uses_custom_format_without_secret_argv() -> TestResult {
    let plan = postgres_dump_plan(std::path::Path::new("/vault/backup/postgres.dump"))?;

    assert_eq!(plan.program, "pg_dump");
    assert_eq!(plan.args[0], "--format=custom");
    assert!(plan.args.contains(&"--file".to_owned()));
    assert!(
        plan.args
            .contains(&"/vault/backup/postgres.dump".to_owned())
    );
    assert_eq!(
        plan.required_env,
        vec!["PGHOST", "PGPORT", "PGUSER", "PGDATABASE"]
    );
    assert!(!format!("{plan:?}").contains("postgres://secret"));

    Ok(())
}

#[test]
fn postgres_dump_plan_rejects_parent_relative_paths() {
    let result = postgres_dump_plan(std::path::Path::new("/vault/../postgres.dump"));

    assert!(matches!(result, Err(BackupError::InvalidPath)));
}

#[test]
fn restic_restore_plan_uses_snapshot_target_and_secret_env() -> TestResult {
    let plan = restic_restore_plan("snapshot-123", std::path::Path::new("/restore/target"))?;

    assert_eq!(plan.program, "restic");
    assert_eq!(
        plan.args.iter().map(String::as_str).collect::<Vec<_>>(),
        vec!["restore", "snapshot-123", "--target", "/restore/target"]
    );
    assert_eq!(
        plan.required_env,
        vec!["RESTIC_REPOSITORY", "RESTIC_PASSWORD_FILE"]
    );

    Ok(())
}

#[test]
fn restic_restore_plan_rejects_shell_like_snapshot_ids() {
    let result = restic_restore_plan("snapshot-123;rm", std::path::Path::new("/restore/target"));

    assert!(matches!(result, Err(BackupError::InvalidSnapshotId)));
}

#[test]
fn postgres_restore_plan_uses_env_database_and_clean_restore_args() -> TestResult {
    let plan = postgres_restore_plan(std::path::Path::new("/restore/postgres.dump"))?;

    assert_eq!(plan.program, "pg_restore");
    assert!(plan.args.contains(&"--clean".to_owned()));
    assert!(plan.args.contains(&"--if-exists".to_owned()));
    assert!(plan.args.contains(&"--no-owner".to_owned()));
    assert!(plan.args.contains(&"/restore/postgres.dump".to_owned()));
    assert_eq!(
        plan.required_env,
        vec!["PGHOST", "PGPORT", "PGUSER", "PGDATABASE"]
    );
    assert!(!format!("{plan:?}").contains("postgres://secret"));

    Ok(())
}

#[test]
fn restic_backup_plan_uses_explicit_durable_paths_and_secret_env() -> TestResult {
    let plan = restic_backup_plan(
        std::path::Path::new("/vault/storage"),
        std::path::Path::new("/vault/backup/postgres.dump"),
    )?;

    assert_eq!(plan.program, "restic");
    assert_eq!(plan.args[0], "backup");
    assert!(plan.args.contains(&"/vault/storage/originals".to_owned()));
    assert!(plan.args.contains(&"/vault/storage/derivatives".to_owned()));
    assert!(plan.args.contains(&"/vault/storage/model-packs".to_owned()));
    assert!(
        plan.args
            .contains(&"/vault/backup/postgres.dump".to_owned())
    );
    assert!(plan.args.contains(&"/vault/storage/staging".to_owned()));
    assert!(!plan.args.contains(&"/vault/storage".to_owned()));
    assert_eq!(
        plan.required_env,
        vec!["RESTIC_REPOSITORY", "RESTIC_PASSWORD_FILE"]
    );
    assert!(!format!("{plan:?}").contains("password-value"));

    Ok(())
}

#[test]
fn restic_backup_plan_rejects_parent_relative_paths() {
    let result = restic_backup_plan(
        std::path::Path::new("/vault/../storage"),
        std::path::Path::new("/vault/backup/postgres.dump"),
    );

    assert!(matches!(result, Err(BackupError::InvalidPath)));
}

#[test]
fn restic_retention_plan_uses_conservative_prune_policy_without_secret_argv() {
    let plan = restic_retention_plan();

    assert_eq!(plan.program, "restic");
    assert_eq!(plan.args[0], "forget");
    assert!(plan.args.contains(&"--prune".to_owned()));
    assert!(plan.args.contains(&"--keep-last".to_owned()));
    assert!(plan.args.contains(&"3".to_owned()));
    assert!(plan.args.contains(&"--keep-hourly".to_owned()));
    assert!(plan.args.contains(&"24".to_owned()));
    assert!(plan.args.contains(&"--keep-daily".to_owned()));
    assert!(plan.args.contains(&"30".to_owned()));
    assert!(plan.args.contains(&"--keep-weekly".to_owned()));
    assert!(plan.args.contains(&"12".to_owned()));
    assert!(plan.args.contains(&"--keep-monthly".to_owned()));
    assert!(plan.args.contains(&"--keep-yearly".to_owned()));
    assert!(plan.args.contains(&"--retry-lock".to_owned()));
    assert_eq!(
        plan.required_env,
        vec!["RESTIC_REPOSITORY", "RESTIC_PASSWORD_FILE"]
    );
    assert!(!format!("{plan:?}").contains("password-value"));
}

#[tokio::test]
#[ignore = "requires MIRROR_TEST_DATABASE_URL pointing at a dedicated test database"]
async fn backup_run_records_snapshot_and_restore_check_without_secrets() -> TestResult {
    let pool = fresh_owner_pool().await?;
    let run = create_backup_run(
        &pool,
        CreateBackupRunInput {
            repository_hint: Some("local-restic-repo".to_owned()),
            manifest: backup_manifest_summary(2),
        },
    )
    .await?;
    assert_eq!(run.status, "planned");

    let succeeded = mark_backup_succeeded(&pool, run.backup_run_id, "snapshot-123").await?;
    assert_eq!(succeeded.status, "succeeded");
    assert_eq!(succeeded.snapshot_id.as_deref(), Some("snapshot-123"));

    let restore_checked = mark_restore_check(&pool, run.backup_run_id, true, None).await?;
    assert_eq!(restore_checked.status, "restore_check_succeeded");

    let row: (String, serde_json::Value, Option<String>) = sqlx::query_as(
        r#"
        SELECT repository_hint, manifest, error_message
        FROM backup_runs
        WHERE id = $1
        "#,
    )
    .bind(run.backup_run_id)
    .fetch_one(&pool)
    .await?;
    assert_eq!(row.0, "local-restic-repo");
    assert_eq!(row.1["durable_object_count"], 2);
    assert!(row.2.is_none());
    assert!(!row.1.to_string().contains("RESTIC_PASSWORD"));

    let audit_rows: Vec<(String, String, serde_json::Value)> = sqlx::query_as(
        r#"
        SELECT action, outcome, metadata
        FROM audit_events
        WHERE target_kind = 'backup_run'
            AND target_id = $1
        ORDER BY id
        "#,
    )
    .bind(run.backup_run_id.to_string())
    .fetch_all(&pool)
    .await?;
    assert_eq!(
        audit_rows
            .iter()
            .map(|row| (row.0.as_str(), row.1.as_str()))
            .collect::<Vec<_>>(),
        vec![
            ("backup.run", "success"),
            ("backup.restore_check", "success")
        ]
    );
    assert!(audit_rows.iter().all(|row| {
        !row.2.to_string().contains("RESTIC_PASSWORD")
            && !row.2.to_string().contains("local-restic-repo")
    }));

    Ok(())
}
