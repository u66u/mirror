//! Durable backup selection.
//!
//! Backup code must opt in to durable namespaces instead of recursively backing
//! up the whole storage root. Staging, temp, logs, and scratch data are not
//! durable vault state.

use std::{
    path::{Path, PathBuf},
    process::Command,
};

use serde::Serialize;
use serde_json::{Value, json};
use sqlx::{PgPool, types::Json};
use time::OffsetDateTime;
use uuid::Uuid;

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
#[derive(Debug)]
pub enum BackupError {
    /// Storage operation failed.
    Storage(StorageError),
    /// Storage listed a key that violates key invariants.
    InvalidStorageKey(String),
    /// Backup path is empty or unsafe.
    InvalidPath,
    /// Database failed.
    Database(sqlx::Error),
    /// Restic process could not start.
    Command(std::io::Error),
    /// Restic returned a non-zero status.
    ResticFailed,
}

impl std::fmt::Display for BackupError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::Storage(_) => "backup storage error",
            Self::InvalidStorageKey(_) => "backup storage key is invalid",
            Self::InvalidPath => "backup path is invalid",
            Self::Database(_) => "backup database error",
            Self::Command(_) => "backup command error",
            Self::ResticFailed => "backup command failed",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for BackupError {}

impl From<StorageError> for BackupError {
    fn from(error: StorageError) -> Self {
        Self::Storage(error)
    }
}

impl From<sqlx::Error> for BackupError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

/// Records a planned restic backup.
pub async fn create_backup_run(
    pool: &PgPool,
    input: CreateBackupRunInput,
) -> Result<BackupRun, BackupError> {
    let id = Uuid::now_v7();
    let row = sqlx::query_as::<_, (Uuid, String, Option<String>)>(
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
    )
    .bind(id)
    .bind(input.repository_hint)
    .bind(Json(input.manifest))
    .fetch_one(pool)
    .await?;

    Ok(BackupRun {
        backup_run_id: row.0,
        status: row.1,
        snapshot_id: row.2,
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
    let storage_root = clean_path(storage_root)?;
    let postgres_dump_path = clean_path(postgres_dump_path)?;
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
    let row = sqlx::query_as::<_, (Uuid, String, Option<String>)>(
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
    )
    .bind(backup_run_id)
    .bind(status.as_str())
    .bind(snapshot_id)
    .bind(error_message)
    .fetch_one(pool)
    .await?;

    Ok(BackupRun {
        backup_run_id: row.0,
        status: row.1,
        snapshot_id: row.2,
    })
}

/// Small manifest summary used by the backup runner.
#[must_use]
pub fn backup_manifest_summary(object_count: usize) -> Value {
    json!({
        "manifest_version": "mirror-backup-run-v1",
        "durable_object_count": object_count,
    })
}

fn clean_path(path: &Path) -> Result<PathBuf, BackupError> {
    if path.as_os_str().is_empty() {
        return Err(BackupError::InvalidPath);
    }
    if path.components().any(|component| {
        matches!(
            component,
            std::path::Component::ParentDir | std::path::Component::CurDir
        )
    }) {
        return Err(BackupError::InvalidPath);
    }
    Ok(path.to_path_buf())
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
