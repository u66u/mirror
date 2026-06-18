use std::{env, process::Command};

use mirror_backend::{
    backups::{CreateBackupRunInput, backup_manifest_summary, create_backup_run},
    storage::StorageKey,
};
use tempfile::TempDir;
use uuid::Uuid;

mod support;
use support::{TestResult, fresh_owner_pool, write_executable_script};

#[test]
fn maintenance_apply_requires_explicit_orphan_selection() -> TestResult {
    let output = Command::new(env!("CARGO_BIN_EXE_maintenance"))
        .arg("--apply")
        .output()?;

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("--apply requires at least one --delete-orphan KEY")
    );
    Ok(())
}

#[test]
fn maintenance_rejects_noncanonical_original_key_before_database_access() -> TestResult {
    let output = Command::new(env!("CARGO_BIN_EXE_maintenance"))
        .args([
            "--delete-orphan",
            "originals/blake3/arbitrary/object",
            "--apply",
        ])
        .output()?;

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("not a content-addressed original key")
    );
    Ok(())
}

#[test]
fn maintenance_backup_plan_prints_restic_inputs_without_database() -> TestResult {
    let output = Command::new(env!("CARGO_BIN_EXE_maintenance"))
        .env("MIRROR_STORAGE_ROOT", "/vault/storage")
        .arg("--backup-plan")
        .arg("/vault/backup/postgres.dump")
        .output()?;

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("dump_program\tpg_dump"));
    assert!(stdout.contains("--format=custom"));
    assert!(stdout.contains("PGDATABASE"));
    assert!(stdout.contains("backup_program\trestic"));
    assert!(stdout.contains("/vault/storage/originals"));
    assert!(stdout.contains("/vault/storage/derivatives"));
    assert!(stdout.contains("/vault/backup/postgres.dump"));
    assert!(stdout.contains("RESTIC_REPOSITORY"));
    assert!(stdout.contains("RESTIC_PASSWORD_FILE"));
    assert!(!stdout.contains("MIRROR_DATABASE_URL"));

    Ok(())
}

#[test]
fn maintenance_restore_plan_prints_restore_inputs_without_database() -> TestResult {
    let output = Command::new(env!("CARGO_BIN_EXE_maintenance"))
        .arg("--restore-plan")
        .arg("snapshot-123")
        .arg("/restore/target")
        .arg("/restore/target/vault/backup/postgres.dump")
        .output()?;

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("restore_program\trestic"));
    assert!(stdout.contains("restore_args\trestore\tsnapshot-123"));
    assert!(stdout.contains("/restore/target"));
    assert!(stdout.contains("RESTIC_REPOSITORY"));
    assert!(stdout.contains("RESTIC_PASSWORD_FILE"));
    assert!(stdout.contains("db_restore_program\tpg_restore"));
    assert!(stdout.contains("--clean"));
    assert!(stdout.contains("--if-exists"));
    assert!(stdout.contains("--no-owner"));
    assert!(stdout.contains("PGDATABASE"));
    assert!(!stdout.contains("MIRROR_DATABASE_URL"));

    Ok(())
}

#[cfg(unix)]
#[tokio::test]
#[ignore = "requires MIRROR_TEST_DATABASE_URL pointing at a dedicated test database"]
async fn maintenance_run_backup_records_restic_snapshot_without_secrets() -> TestResult {
    let pool = fresh_owner_pool().await?;
    let test_database_url = env::var("MIRROR_TEST_DATABASE_URL").map_err(std::io::Error::other)?;
    let temp_dir = TempDir::new()?;
    let bin_dir = temp_dir.path().join("bin");
    std::fs::create_dir(&bin_dir)?;
    write_executable_script(
        &bin_dir.join("restic"),
        "#!/bin/sh\nprintf '%s\n' 'snapshot snapshot-test-id saved'\n",
    )?;
    write_executable_script(
        &bin_dir.join("pg_dump"),
        r#"#!/bin/sh
set -eu
test -n "${PGDATABASE:-}"
out=""
while [ "$#" -gt 0 ]; do
  case "$1" in
    --file)
      shift
      out="$1"
      ;;
  esac
  shift
done
test -n "$out"
printf '%s\n' 'dump' > "$out"
"#,
    )?;
    let password_file = temp_dir.path().join("restic-password");
    std::fs::write(&password_file, "test-password")?;
    let dump_path = temp_dir.path().join("postgres.dump");
    let path = format!(
        "{}:{}",
        bin_dir.display(),
        env::var("PATH").unwrap_or_default()
    );

    let output = Command::new(env!("CARGO_BIN_EXE_maintenance"))
        .env("MIRROR_DATABASE_URL", &test_database_url)
        .env("MIRROR_STORAGE_ROOT", temp_dir.path().join("storage"))
        .env("RESTIC_REPOSITORY", "local-test-repo")
        .env("RESTIC_PASSWORD_FILE", &password_file)
        .env("PATH", path)
        .args(["--run-backup", dump_path.to_str().unwrap_or_default()])
        .args(["--repository-hint", "local-test-repo"])
        .output()?;

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("backup_run\t"));
    assert!(stdout.contains("succeeded"));
    assert!(stdout.contains("snapshot-test-id"));
    assert!(!stdout.contains("test-password"));
    assert!(!stdout.contains(&test_database_url));
    assert_eq!(std::fs::read_to_string(&dump_path)?, "dump\n");

    let row: (String, Option<String>, serde_json::Value) = sqlx::query_as(r#"
        SELECT status, snapshot_id, manifest
        FROM backup_runs
        ORDER BY created_at DESC
        LIMIT 1
        "#)
    .fetch_one(&pool)
    .await?;
    assert_eq!(row.0, "succeeded");
    assert_eq!(row.1.as_deref(), Some("snapshot-test-id"));
    assert_eq!(row.2["durable_object_count"], 0);
    assert!(!row.2.to_string().contains("RESTIC_PASSWORD"));

    Ok(())
}

#[tokio::test]
#[ignore = "requires MIRROR_TEST_DATABASE_URL pointing at a dedicated test database"]
async fn maintenance_restore_check_marks_failed_when_original_object_missing() -> TestResult {
    let pool = fresh_owner_pool().await?;
    let test_database_url = env::var("MIRROR_TEST_DATABASE_URL").map_err(std::io::Error::other)?;
    let temp_dir = TempDir::new()?;
    let bytes = b"missing-after-restore";
    let hash = blake3::hash(bytes).to_hex().to_string();
    let key = StorageKey::original_blake3(&hash)?;
    sqlx::query!(
        r#"
        INSERT INTO originals (id, blake3_hash, storage_key, size_bytes, media_type)
        VALUES ($1, $2, $3, $4, 'image/jpeg')
        "#,
        Uuid::now_v7(),
        hash,
        key.as_str(),
        i64::try_from(bytes.len())?
    )
    .execute(&pool)
    .await?;
    let run = create_backup_run(
        &pool,
        CreateBackupRunInput {
            repository_hint: Some("restore-check-test".to_owned()),
            manifest: backup_manifest_summary(1),
        },
    )
    .await?;

    let output = Command::new(env!("CARGO_BIN_EXE_maintenance"))
        .env("MIRROR_DATABASE_URL", test_database_url)
        .env("MIRROR_STORAGE_ROOT", temp_dir.path().join("storage"))
        .args(["--restore-check", &run.backup_run_id.to_string()])
        .output()?;

    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("restore_check\t"));
    assert!(stdout.contains("restore_check_failed"));
    let row: (String, Option<String>) =
        sqlx::query_as("SELECT status, error_message FROM backup_runs WHERE id = $1")
            .bind(run.backup_run_id)
            .fetch_one(&pool)
            .await?;
    assert_eq!(row.0, "restore_check_failed");
    assert!(
        row.1
            .as_deref()
            .is_some_and(|message| message.contains("missing=1"))
    );

    Ok(())
}
