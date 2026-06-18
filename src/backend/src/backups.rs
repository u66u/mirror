//! Durable backup selection.
//!
//! Backup code must opt in to durable namespaces instead of recursively backing
//! up the whole storage root. Staging, temp, logs, and scratch data are not
//! durable vault state.

use std::{
    path::{Path, PathBuf},
    process::Command,
};
use thiserror::Error;

use serde::Serialize;
use serde_json::{Value, json};
use sqlx::{PgPool, types::Json};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::paths::clean_path;
use crate::storage::{ObjectStorage, StorageError, StorageKey};

/// Durable storage object manifest for backup tooling.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct DurableStorageBackupManifest {
    /// Manifest generation timestamp.
    pub generated_at: OffsetDateTime,
    /// Manifest schema marker.
    pub manifest_version: &'static str,
    /// Durable object keys to include in object-storage backups.
    pub objects: Vec<DurableStorageObject>,
}

/// One object selected for durable backup.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct DurableStorageObject {
    /// Object storage key.
    pub storage_key: String,
}

/// Restic backup plan with secret values kept outside argv.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResticBackupPlan {
    /// Program name.
    pub program: &'static str,
    /// Command arguments.
    pub args: Vec<String>,
    /// Environment variable names required at execution time.
    pub required_env: Vec<&'static str>,
}

/// PostgreSQL dump plan with the database URL kept outside argv.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostgresDumpPlan {
    /// Program name.
    pub program: &'static str,
    /// Command arguments.
    pub args: Vec<String>,
    /// Environment variable names required at execution time.
    pub required_env: Vec<&'static str>,
}

/// Restic restore plan with repository/password kept outside argv.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResticRestorePlan {
    /// Program name.
    pub program: &'static str,
    /// Command arguments.
    pub args: Vec<String>,
    /// Environment variable names required at execution time.
    pub required_env: Vec<&'static str>,
}

/// PostgreSQL restore plan with the database URL kept outside argv.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostgresRestorePlan {
    /// Program name.
    pub program: &'static str,
    /// Command arguments.
    pub args: Vec<String>,
    /// Environment variable names required at execution time.
    pub required_env: Vec<&'static str>,
}

/// Persisted backup status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackupRunStatus {
    /// Plan exists but command has not started.
    Planned,
    /// Command is running.
    Running,
    /// Backup command succeeded.
    Succeeded,
    /// Backup command failed.
    Failed,
    /// Restore check succeeded.
    RestoreCheckSucceeded,
    /// Restore check failed.
    RestoreCheckFailed,
}

impl BackupRunStatus {
    fn try_from_str(s: &str) -> Option<Self> {
        match s {
            "planned" => Some(Self::Planned),
            "running" => Some(Self::Running),
            "succeeded" => Some(Self::Succeeded),
            "failed" => Some(Self::Failed),
            "restore_check_succeeded" => Some(Self::RestoreCheckSucceeded),
            "restore_check_failed" => Some(Self::RestoreCheckFailed),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Planned => "planned",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::RestoreCheckSucceeded => "restore_check_succeeded",
            Self::RestoreCheckFailed => "restore_check_failed",
        }
    }
}

/// New backup run input.
#[derive(Debug)]
pub struct CreateBackupRunInput {
    /// Redacted repository locator or label.
    pub repository_hint: Option<String>,
    /// Manifest summary. Must not contain secrets.
    pub manifest: Value,
}

/// Backup run row.
#[derive(Debug, PartialEq, Eq)]
pub struct BackupRun {
    /// Backup run ID.
    pub backup_run_id: Uuid,
    /// Current status.
    pub status: String,
    /// Restic snapshot ID, if known.
    pub snapshot_id: Option<String>,
}

/// Successful restic invocation output selected for persistence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResticBackupOutput {
    /// Snapshot ID parsed from restic output, or a fallback marker.
    pub snapshot_id: String,
}

/// Backup manifest failure.
#[derive(Debug, Error)]
pub enum BackupError {
    /// Storage operation failed.
    #[error("backup storage error")]
    Storage(#[from] StorageError),
    /// Storage listed a key that violates key invariants.
    #[error("backup storage key is invalid")]
    InvalidStorageKey(String),
    /// Backup path is empty or unsafe.
    #[error("backup path is invalid")]
    InvalidPath,
    /// Restic snapshot ID is empty or unsafe.
    #[error("backup snapshot ID is invalid")]
    InvalidSnapshotId,
    /// Database failed.
    #[error("backup database error")]
    Database(#[from] sqlx::Error),
    /// Restic process could not start.
    #[error("backup command error")]
    Command(std::io::Error),
    /// pg_dump returned a non-zero status.
    #[error("postgres dump failed")]
    PgDumpFailed,
    /// Restic returned a non-zero status.
    #[error("backup command failed")]
    ResticFailed,
}

/// Records a planned restic backup.
pub async fn create_backup_run(
    pool: &PgPool,
    input: CreateBackupRunInput,
) -> Result<BackupRun, BackupError> {
    let id = Uuid::now_v7();
    let row = sqlx::query!(
        r#"
        INSERT INTO backup_runs (
            id,
            kind,
            status,
            repository_hint,
            manifest
        )
        VALUES ($1, 'restic', 'planned', $2, $3)
        RETURNING id, status, snapshot_id
        "#,
        id,
        input.repository_hint,
        sqlx::types::Json(&input.manifest) as _
    )
    .fetch_one(pool)
    .await?;

    let status_str = BackupRunStatus::try_from_str(row.status.as_str())
        .ok_or_else(|| BackupError::InvalidStorageKey("status".into()))?;

    Ok(BackupRun {
        backup_run_id: row.id,
        status: status_str.as_str().to_string(),
        snapshot_id: row.snapshot_id,
    })
}

/// Marks a restic backup run as succeeded.
pub async fn mark_backup_succeeded(
    pool: &PgPool,
    backup_run_id: Uuid,
    snapshot_id: &str,
) -> Result<BackupRun, BackupError> {
    update_backup_run(
        pool,
        backup_run_id,
        BackupRunStatus::Succeeded,
        Some(snapshot_id),
        None,
    )
    .await
}

/// Marks a restic backup run as failed.
pub async fn mark_backup_failed(
    pool: &PgPool,
    backup_run_id: Uuid,
    error_message: &str,
) -> Result<BackupRun, BackupError> {
    update_backup_run(
        pool,
        backup_run_id,
        BackupRunStatus::Failed,
        None,
        Some(error_message),
    )
    .await
}

/// Marks restore-check status for a backup run.
pub async fn mark_restore_check(
    pool: &PgPool,
    backup_run_id: Uuid,
    succeeded: bool,
    error_message: Option<&str>,
) -> Result<BackupRun, BackupError> {
    let status = if succeeded {
        BackupRunStatus::RestoreCheckSucceeded
    } else {
        BackupRunStatus::RestoreCheckFailed
    };
    update_backup_run(pool, backup_run_id, status, None, error_message).await
}

/// Builds a restic backup command plan for durable vault state.
///
/// The plan backs up explicit durable paths only. Passwords and repository
/// credentials are required through environment variables, never argv.
pub fn restic_backup_plan(
    storage_root: &Path,
    postgres_dump_path: &Path,
) -> Result<ResticBackupPlan, BackupError> {
    let storage_root = clean_backup_path(storage_root)?;
    let postgres_dump_path = clean_backup_path(postgres_dump_path)?;
    Ok(ResticBackupPlan {
        program: "restic",
        args: vec![
            "backup".to_owned(),
            storage_root.join("originals").display().to_string(),
            storage_root.join("derivatives").display().to_string(),
            postgres_dump_path.display().to_string(),
            "--exclude".to_owned(),
            storage_root.join("staging").display().to_string(),
            "--exclude".to_owned(),
            storage_root.join("tmp").display().to_string(),
            "--exclude".to_owned(),
            storage_root.join("logs").display().to_string(),
            "--exclude".to_owned(),
            storage_root.join("scratch").display().to_string(),
        ],
        required_env: vec!["RESTIC_REPOSITORY", "RESTIC_PASSWORD_FILE"],
    })
}

/// Builds a restic restore command plan for an existing snapshot.
pub fn restic_restore_plan(
    snapshot_id: &str,
    restore_target: &Path,
) -> Result<ResticRestorePlan, BackupError> {
    let snapshot_id = clean_snapshot_id(snapshot_id)?;
    let restore_target = clean_backup_path(restore_target)?;
    Ok(ResticRestorePlan {
        program: "restic",
        args: vec![
            "restore".to_owned(),
            snapshot_id.to_owned(),
            "--target".to_owned(),
            restore_target.display().to_string(),
        ],
        required_env: vec!["RESTIC_REPOSITORY", "RESTIC_PASSWORD_FILE"],
    })
}

/// Builds a `pg_restore` command plan for a custom-format dump.
///
/// The destination database URL is supplied through `PGDATABASE` at execution
/// time so it is not exposed through process argv.
pub fn postgres_restore_plan(dump_path: &Path) -> Result<PostgresRestorePlan, BackupError> {
    let dump_path = clean_backup_path(dump_path)?;
    Ok(PostgresRestorePlan {
        program: "pg_restore",
        args: vec![
            "--clean".to_owned(),
            "--if-exists".to_owned(),
            "--no-owner".to_owned(),
            dump_path.display().to_string(),
        ],
        required_env: vec!["PGDATABASE"],
    })
}

/// Builds a `pg_dump -Fc` command plan.
///
/// The database URL is supplied through `PGDATABASE` at execution time so it is
/// not exposed through process argv.
pub fn postgres_dump_plan(dump_path: &Path) -> Result<PostgresDumpPlan, BackupError> {
    let dump_path = clean_backup_path(dump_path)?;
    Ok(PostgresDumpPlan {
        program: "pg_dump",
        args: vec![
            "--format=custom".to_owned(),
            "--file".to_owned(),
            dump_path.display().to_string(),
        ],
        required_env: vec!["PGDATABASE"],
    })
}

/// Runs `pg_dump` with the supplied plan.
pub fn run_postgres_dump_plan(
    plan: &PostgresDumpPlan,
    database_url: &str,
) -> Result<(), BackupError> {
    let output = Command::new(plan.program)
        .args(&plan.args)
        .env("PGDATABASE", database_url)
        .output()
        .map_err(BackupError::Command)?;
    if output.status.success() {
        Ok(())
    } else {
        Err(BackupError::PgDumpFailed)
    }
}

/// Runs restic with the supplied plan and returns the snapshot ID.
///
/// The process inherits environment so operators can provide
/// `RESTIC_REPOSITORY` and `RESTIC_PASSWORD_FILE` through the service manager.
/// Secret values are never copied into argv or persisted output.
pub fn run_restic_backup_plan(plan: &ResticBackupPlan) -> Result<ResticBackupOutput, BackupError> {
    let output = Command::new(plan.program)
        .args(&plan.args)
        .output()
        .map_err(BackupError::Command)?;
    if !output.status.success() {
        return Err(BackupError::ResticFailed);
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(ResticBackupOutput {
        snapshot_id: parse_restic_snapshot_id(&stdout)
            .unwrap_or("unknown")
            .to_owned(),
    })
}

/// Builds the durable object-storage selection for backup.
pub async fn durable_storage_manifest(
    storage: &ObjectStorage,
) -> Result<DurableStorageBackupManifest, BackupError> {
    let mut keys = Vec::new();
    keys.extend(list_valid(storage, &StorageKey::originals_blake3_prefix()).await?);
    keys.extend(list_valid(storage, &StorageKey::derivatives_prefix()).await?);
    keys.extend(list_valid(storage, &StorageKey::model_packs_prefix()).await?);
    keys.sort();
    keys.dedup();

    Ok(DurableStorageBackupManifest {
        generated_at: OffsetDateTime::now_utc(),
        manifest_version: "mirror-durable-storage-backup-v1",
        objects: keys
            .into_iter()
            .map(|storage_key| DurableStorageObject { storage_key })
            .collect(),
    })
}

fn parse_restic_snapshot_id(stdout: &str) -> Option<&str> {
    stdout.lines().rev().find_map(|line| {
        let mut words = line.split_whitespace();
        while let Some(word) = words.next() {
            if word == "snapshot" {
                return words.next();
            }
        }
        None
    })
}

async fn update_backup_run(
    pool: &PgPool,
    backup_run_id: Uuid,
    status: BackupRunStatus,
    snapshot_id: Option<&str>,
    error_message: Option<&str>,
) -> Result<BackupRun, BackupError> {
    let mut tx = pool.begin().await?;
    let row = sqlx::query!(
        r#"
        UPDATE backup_runs
        SET
            status = $2,
            snapshot_id = COALESCE($3, snapshot_id),
            error_message = $4,
            completed_at = CASE
                WHEN $2 IN ('succeeded', 'failed', 'restore_check_succeeded', 'restore_check_failed')
                THEN now()
                ELSE completed_at
            END,
            updated_at = now()
        WHERE id = $1
        RETURNING id, status, snapshot_id
        "#,
        backup_run_id,
        status.as_str(),
        snapshot_id,
        error_message
    )
    .fetch_one(&mut *tx)
    .await?;

    let action = backup_audit_action(status);
    let outcome = backup_audit_outcome(status);
    let target_id = backup_run_id.to_string();
    let meta = Json(json!({
        "status": status.as_str(),
        "snapshot_recorded": snapshot_id.is_some(),
        "error_recorded": error_message.is_some(),
    }));

    sqlx::query!(
        r#"
        INSERT INTO audit_events (
            actor_kind,
            action,
            outcome,
            target_kind,
            target_id,
            metadata
        )
        VALUES (
            'system',
            $1,
            $2,
            'backup_run',
            $3,
            $4
        )
        "#,
        action,
        outcome,
        target_id,
        meta as _
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    Ok(BackupRun {
        backup_run_id: row.id,
        status: row.status,
        snapshot_id: row.snapshot_id,
    })
}

fn backup_audit_action(status: BackupRunStatus) -> &'static str {
    match status {
        BackupRunStatus::Planned | BackupRunStatus::Running => "backup.run",
        BackupRunStatus::Succeeded | BackupRunStatus::Failed => "backup.run",
        BackupRunStatus::RestoreCheckSucceeded | BackupRunStatus::RestoreCheckFailed => {
            "backup.restore_check"
        }
    }
}

fn backup_audit_outcome(status: BackupRunStatus) -> &'static str {
    match status {
        BackupRunStatus::Failed | BackupRunStatus::RestoreCheckFailed => "failure",
        BackupRunStatus::Planned
        | BackupRunStatus::Running
        | BackupRunStatus::Succeeded
        | BackupRunStatus::RestoreCheckSucceeded => "success",
    }
}

/// Small manifest summary used by the backup runner.
#[must_use]
pub fn backup_manifest_summary(object_count: usize) -> Value {
    json!({
        "manifest_version": "mirror-backup-run-v1",
        "durable_object_count": object_count,
    })
}

fn clean_backup_path(path: &Path) -> Result<PathBuf, BackupError> {
    clean_path(path).map_err(|_| BackupError::InvalidPath)
}

fn clean_snapshot_id(snapshot_id: &str) -> Result<&str, BackupError> {
    let valid = !snapshot_id.is_empty()
        && snapshot_id.len() <= 200
        && snapshot_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'));
    if valid {
        Ok(snapshot_id)
    } else {
        Err(BackupError::InvalidSnapshotId)
    }
}

async fn list_valid(
    storage: &ObjectStorage,
    prefix: &StorageKey,
) -> Result<Vec<String>, BackupError> {
    storage
        .list_recursive(prefix)
        .await?
        .into_iter()
        .map(|key| {
            StorageKey::new(key.clone())
                .map_err(|_| BackupError::InvalidStorageKey(key.clone()))?;
            Ok(key)
        })
        .collect()
}
