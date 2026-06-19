use std::{env, process::Command};

use mirror_backend::{
    backups::{CreateBackupRunInput, backup_manifest_summary, create_backup_run},
    models::{
        ImagePreprocessConfig, ModelPackFileManifest, ModelPackManifest, ModelPackSelfTestManifest,
        OnnxModelPackConfig,
    },
    storage::StorageKey,
};
use sha2::{Digest, Sha256};
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
    assert!(stdout.contains("/vault/storage/model-packs"));
    assert!(stdout.contains("/vault/backup/postgres.dump"));
    assert!(stdout.contains("RESTIC_REPOSITORY"));
    assert!(stdout.contains("RESTIC_PASSWORD_FILE"));
    assert!(!stdout.contains("MIRROR_DATABASE_URL"));

    Ok(())
}

#[test]
fn maintenance_retention_plan_prints_prune_policy_without_database() -> TestResult {
    let output = Command::new(env!("CARGO_BIN_EXE_maintenance"))
        .arg("--retention-plan")
        .output()?;

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("retention_program\trestic"));
    assert!(stdout.contains("retention_args\tforget"));
    assert!(stdout.contains("--prune"));
    assert!(stdout.contains("--keep-daily"));
    assert!(stdout.contains("--keep-weekly"));
    assert!(stdout.contains("--keep-monthly"));
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

#[test]
fn maintenance_prints_generated_model_pack_schema_without_database() -> TestResult {
    let output = Command::new(env!("CARGO_BIN_EXE_maintenance"))
        .arg("--model-pack-schema")
        .output()?;

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\"title\": \"ModelPackManifest\""));
    assert!(stdout.contains("\"image_preprocess\""));
    assert!(stdout.contains("\"self_tests\""));
    assert!(!stdout.contains("MIRROR_DATABASE_URL"));

    Ok(())
}

#[test]
fn maintenance_validates_local_model_pack_without_database() -> TestResult {
    let source_dir = write_cli_model_pack()?;
    let output = Command::new(env!("CARGO_BIN_EXE_maintenance"))
        .arg("--validate-model-pack")
        .arg(source_dir.path())
        .output()?;

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("model_pack\tvalid"));
    assert!(stdout.contains("kind\tsemantic_image_text"));
    assert!(stdout.contains("runtime\tonnx"));
    assert!(stdout.contains("files\t4"));
    assert!(stdout.contains("bytes\t21"));
    assert!(!stdout.contains("MIRROR_DATABASE_URL"));

    Ok(())
}

#[test]
fn maintenance_reports_operator_error_for_invalid_model_pack() -> TestResult {
    let source_dir = write_cli_model_pack()?;
    std::fs::write(
        source_dir.path().join("models/image_encoder.onnx"),
        b"changed",
    )?;
    let output = Command::new(env!("CARGO_BIN_EXE_maintenance"))
        .arg("--validate-model-pack")
        .arg(source_dir.path())
        .output()?;

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("size or SHA-256 checksum mismatch"));

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
test -n "${PGHOST:-}"
test -n "${PGPORT:-}"
test -n "${PGUSER:-}"
test -n "${PGDATABASE:-}"
case "$PGDATABASE" in
  *://*) exit 3 ;;
esac
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

    let row: (String, Option<String>, serde_json::Value) = sqlx::query_as(
        r#"
        SELECT status, snapshot_id, manifest
        FROM backup_runs
        ORDER BY created_at DESC
        LIMIT 1
        "#,
    )
    .fetch_one(&pool)
    .await?;
    assert_eq!(row.0, "succeeded");
    assert_eq!(row.1.as_deref(), Some("snapshot-test-id"));
    assert_eq!(row.2["durable_object_count"], 0);
    assert!(!row.2.to_string().contains("RESTIC_PASSWORD"));

    Ok(())
}

fn write_cli_model_pack() -> TestResult<TempDir> {
    let source_dir = TempDir::new()?;
    std::fs::create_dir_all(source_dir.path().join("models"))?;
    std::fs::write(
        source_dir.path().join("models/image_encoder.onnx"),
        b"image",
    )?;
    std::fs::write(source_dir.path().join("models/text_encoder.onnx"), b"text")?;
    std::fs::create_dir_all(source_dir.path().join("tokenizer"))?;
    std::fs::write(
        source_dir.path().join("tokenizer/tokenizer.json"),
        b"tokenizer",
    )?;
    std::fs::create_dir_all(source_dir.path().join("self-tests"))?;
    std::fs::write(source_dir.path().join("self-tests/cat.jpg"), b"cat")?;

    let manifest = ModelPackManifest {
        kind: "semantic_image_text".to_owned(),
        runtime: "onnx".to_owned(),
        model_key: "siglip2-base-patch16-224".to_owned(),
        model_revision: "2026-06-18.cli".to_owned(),
        license: "Apache-2.0".to_owned(),
        embedding_dimension: 768,
        distance_metric: "cosine".to_owned(),
        onnx: OnnxModelPackConfig {
            image_model_path: "models/image_encoder.onnx".to_owned(),
            text_model_path: "models/text_encoder.onnx".to_owned(),
            tokenizer_path: "tokenizer/tokenizer.json".to_owned(),
            image_input_name: "pixel_values".to_owned(),
            image_output_name: "image_embeds".to_owned(),
            text_input_ids_name: "input_ids".to_owned(),
            text_attention_mask_name: "attention_mask".to_owned(),
            text_output_name: "text_embeds".to_owned(),
        },
        image_preprocess: ImagePreprocessConfig {
            width: 224,
            height: 224,
            color_order: "rgb".to_owned(),
            tensor_layout: "nchw".to_owned(),
            mean: [0.5, 0.5, 0.5],
            std: [0.5, 0.5, 0.5],
        },
        face_detection: None,
        face_embedding: None,
        files: vec![
            cli_file_manifest("models/image_encoder.onnx", b"image")?,
            cli_file_manifest("models/text_encoder.onnx", b"text")?,
            cli_file_manifest("tokenizer/tokenizer.json", b"tokenizer")?,
            cli_file_manifest("self-tests/cat.jpg", b"cat")?,
        ],
        self_tests: vec![ModelPackSelfTestManifest {
            name: "text_image_fixture_similarity".to_owned(),
            input_path: "self-tests/cat.jpg".to_owned(),
            expected_output_sha256: "d".repeat(64),
        }],
    };
    let body = serde_json::to_vec_pretty(&manifest).map_err(std::io::Error::other)?;
    std::fs::write(source_dir.path().join("manifest.json"), body)?;
    Ok(source_dir)
}

fn cli_file_manifest(path: &str, bytes: &[u8]) -> TestResult<ModelPackFileManifest> {
    let digest = Sha256::digest(bytes);
    Ok(ModelPackFileManifest {
        path: path.to_owned(),
        sha256: format!("{digest:x}"),
        size_bytes: i64::try_from(bytes.len())?,
    })
}

#[cfg(unix)]
#[test]
fn maintenance_run_retention_executes_forget_prune_without_database() -> TestResult {
    let temp_dir = TempDir::new()?;
    let bin_dir = temp_dir.path().join("bin");
    std::fs::create_dir(&bin_dir)?;
    let marker = temp_dir.path().join("retention-ran");
    write_executable_script(
        &bin_dir.join("restic"),
        &format!(
            r#"#!/bin/sh
set -eu
test "$1" = forget
case " $* " in
  *" --prune "*) ;;
  *) exit 2 ;;
esac
case " $* " in
  *" --keep-daily "*) ;;
  *) exit 2 ;;
esac
test -n "${{RESTIC_REPOSITORY:-}}"
test -n "${{RESTIC_PASSWORD_FILE:-}}"
printf '%s\n' "$*" > "{}"
"#,
            marker.display()
        ),
    )?;
    let password_file = temp_dir.path().join("restic-password");
    std::fs::write(&password_file, "test-password")?;
    let path = format!(
        "{}:{}",
        bin_dir.display(),
        env::var("PATH").unwrap_or_default()
    );

    let output = Command::new(env!("CARGO_BIN_EXE_maintenance"))
        .env("RESTIC_REPOSITORY", "local-test-repo")
        .env("RESTIC_PASSWORD_FILE", &password_file)
        .env("PATH", path)
        .arg("--run-retention")
        .output()?;

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("retention_run\tsucceeded"));
    assert!(!stdout.contains("test-password"));
    let args = std::fs::read_to_string(marker)?;
    assert!(args.contains("--prune"));
    assert!(args.contains("--keep-daily"));

    Ok(())
}

#[cfg(unix)]
#[tokio::test]
#[ignore = "requires MIRROR_TEST_DATABASE_URL pointing at a dedicated test database"]
async fn maintenance_run_restore_executes_restic_then_pg_restore_without_secret_argv() -> TestResult
{
    let _pool = fresh_owner_pool().await?;
    let test_database_url = env::var("MIRROR_TEST_DATABASE_URL").map_err(std::io::Error::other)?;
    let temp_dir = TempDir::new()?;
    let bin_dir = temp_dir.path().join("bin");
    std::fs::create_dir(&bin_dir)?;
    let restore_marker = temp_dir.path().join("restic-ran");
    let pg_marker = temp_dir.path().join("pg-restore-ran");
    write_executable_script(
        &bin_dir.join("restic"),
        &format!(
            r#"#!/bin/sh
set -eu
test "$1" = restore
test "$2" = snapshot-restore-test
test "$3" = --target
test -n "${{RESTIC_REPOSITORY:-}}"
test -n "${{RESTIC_PASSWORD_FILE:-}}"
printf '%s\n' "$4" > "{}"
"#,
            restore_marker.display()
        ),
    )?;
    write_executable_script(
        &bin_dir.join("pg_restore"),
        &format!(
            r#"#!/bin/sh
set -eu
test -n "${{PGHOST:-}}"
test -n "${{PGPORT:-}}"
test -n "${{PGUSER:-}}"
test -n "${{PGDATABASE:-}}"
case "${{PGDATABASE}}" in
  *://*) exit 3 ;;
esac
case " $* " in
  *" --clean "*) ;;
  *) exit 2 ;;
esac
case " $* " in
  *" --if-exists "*) ;;
  *) exit 2 ;;
esac
case " $* " in
  *" --no-owner "*) ;;
  *) exit 2 ;;
esac
printf 'database=%s\nhost=%s\nport=%s\nuser=%s\n' "$PGDATABASE" "$PGHOST" "$PGPORT" "$PGUSER" > "{}"
"#,
            pg_marker.display()
        ),
    )?;
    let password_file = temp_dir.path().join("restic-password");
    std::fs::write(&password_file, "test-password")?;
    let restore_target = temp_dir.path().join("restore-target");
    let dump_path = temp_dir
        .path()
        .join("restore-target/vault/backup/postgres.dump");
    std::fs::create_dir_all(dump_path.parent().unwrap_or(temp_dir.path()))?;
    std::fs::write(&dump_path, "dump")?;
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
        .args([
            "--run-restore",
            "snapshot-restore-test",
            restore_target.to_str().unwrap_or_default(),
            dump_path.to_str().unwrap_or_default(),
        ])
        .output()?;

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("restore_run\tsnapshot-restore-test"));
    assert!(!stdout.contains("test-password"));
    assert!(!stdout.contains(&test_database_url));
    assert_eq!(
        std::fs::read_to_string(&restore_marker)?,
        format!("{}\n", restore_target.display())
    );
    let pg_env = std::fs::read_to_string(&pg_marker)?;
    assert!(pg_env.contains("database="));
    assert!(pg_env.contains("host="));
    assert!(pg_env.contains("port="));
    assert!(pg_env.contains("user="));
    assert!(!pg_env.contains("://"));

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
    let row = sqlx::query!(
        "SELECT status, error_message FROM backup_runs WHERE id = $1",
        run.backup_run_id
    )
    .fetch_one(&pool)
    .await?;
    assert_eq!(row.status, "restore_check_failed");
    assert!(
        row.error_message
            .as_deref()
            .is_some_and(|message| message.contains("missing=1"))
    );

    Ok(())
}
