//! Original-object integrity scanning and explicit orphan remediation.

use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

use futures_util::TryStreamExt;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{PgPool, types::Json};
use uuid::Uuid;

use crate::{
    jobs::{self, JobKind, JobSpec, LeasedJob},
    storage::{ObjectStorage, StorageError, StorageKey},
};

/// One database-backed original whose object was absent from storage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MissingOriginalObject {
    /// Original content digest recorded by Postgres.
    pub blake3_hash: String,
    /// Object key recorded by Postgres.
    pub storage_key: String,
}

/// Read-only comparison of Postgres originals and stored original objects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OriginalStorageIntegrityReport {
    /// Stored objects that have no `originals.storage_key` reference.
    pub orphan_objects: Vec<StorageKey>,
    /// Postgres originals whose recorded object key was not listed.
    pub missing_objects: Vec<MissingOriginalObject>,
}

/// Persisted integrity scan run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IntegrityScanRun {
    /// Scan run ID.
    pub integrity_scan_run_id: Uuid,
    /// Current run status.
    pub status: String,
}

/// One missing derivative object recorded by an integrity scan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MissingDerivativeObject {
    /// Derivative row ID.
    pub derivative_id: Uuid,
    /// Object key recorded by Postgres.
    pub storage_key: String,
}

/// One corrupt object recorded by an integrity scan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CorruptStorageObject {
    /// Database row ID for the object.
    pub row_id: Uuid,
    /// Object key recorded by Postgres.
    pub storage_key: String,
    /// Expected BLAKE3 digest, when one is stored.
    pub expected_blake3: Option<String>,
    /// Actual BLAKE3 digest read from object storage.
    pub actual_blake3: Option<String>,
    /// Expected byte length.
    pub expected_size_bytes: i64,
    /// Actual byte length read from object storage.
    pub actual_size_bytes: Option<i64>,
    /// Stable diagnostic reason.
    pub reason: String,
}

/// Full durable-object integrity report persisted to Postgres.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectIntegrityReport {
    /// Original rows whose object is absent.
    pub missing_originals: Vec<MissingOriginalObject>,
    /// Original rows whose stored bytes do not match row checksum/size.
    pub corrupt_originals: Vec<CorruptStorageObject>,
    /// Derivative rows whose object is absent.
    pub missing_derivatives: Vec<MissingDerivativeObject>,
    /// Derivative rows whose stored bytes do not match row checksum/size.
    pub corrupt_derivatives: Vec<CorruptStorageObject>,
}

/// Outcome of explicitly requested orphan deletion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrphanRemediationReport {
    /// Selected objects deleted after a fresh database ownership check.
    pub deleted_objects: Vec<StorageKey>,
    /// Selected objects retained because a database reference now exists.
    pub retained_db_backed_objects: Vec<StorageKey>,
    /// Selected objects that were already absent after the database check.
    pub already_missing_objects: Vec<StorageKey>,
}

/// Integrity scan or remediation failure.
#[derive(Debug, Error)]
pub enum IntegrityError {
    /// Postgres query failed.
    #[error("integrity database error: {0}")]
    Database(#[from] sqlx::Error),
    /// Object storage operation failed.
    #[error("integrity storage error: {0}")]
    Storage(#[from] StorageError),
    /// A storage listing returned an unsafe object key.
    #[error("storage listed an invalid object key")]
    InvalidListedObjectKey(String),
    /// Remediation was asked to delete outside the originals/BLAKE3 namespace.
    #[error("invalid original object remediation key")]
    InvalidRemediationKey(String),
    /// Job kind belongs to another worker path.
    #[error("unsupported integrity job kind")]
    UnsupportedJobKind,
    /// Job payload is missing required fields.
    #[error("invalid integrity job payload")]
    InvalidJobPayload,
    /// Job queue operation failed.
    #[error("integrity job queue error: {0}")]
    Job(#[from] jobs::JobError),
    /// JSON persistence failed.
    #[error("integrity json error: {0}")]
    Json(#[from] serde_json::Error),
}

/// Compares all database originals with recursively listed BLAKE3 original objects.
///
/// This function never deletes or modifies objects.
pub async fn scan_original_storage(
    pool: &PgPool,
    storage: &ObjectStorage,
) -> Result<OriginalStorageIntegrityReport, IntegrityError> {
    let database_originals =
        sqlx::query!("SELECT blake3_hash, storage_key FROM originals ORDER BY storage_key")
            .fetch_all(pool)
            .await?;
    let database_keys = database_originals
        .iter()
        .map(|row| row.storage_key.as_str())
        .collect::<BTreeSet<_>>();

    let listed_keys = storage
        .list_recursive(&StorageKey::originals_blake3_prefix())
        .await?
        .into_iter()
        .collect::<BTreeSet<_>>();

    let orphan_objects = listed_keys
        .iter()
        .filter(|storage_key| !database_keys.contains(storage_key.as_str()))
        .map(|storage_key| {
            StorageKey::new(storage_key.clone())
                .map_err(|_| IntegrityError::InvalidListedObjectKey(storage_key.clone()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let missing_objects = database_originals
        .into_iter()
        .filter(|row| !listed_keys.contains(&row.storage_key))
        .map(|row| MissingOriginalObject {
            blake3_hash: row.blake3_hash,
            storage_key: row.storage_key,
        })
        .collect();

    Ok(OriginalStorageIntegrityReport {
        orphan_objects,
        missing_objects,
    })
}

/// Enqueues a background object-integrity scan.
pub async fn enqueue_integrity_scan(pool: &PgPool) -> Result<IntegrityScanRun, IntegrityError> {
    let run_id = Uuid::now_v7();
    let mut tx = pool.begin().await?;
    let row = sqlx::query!(
        r#"
        INSERT INTO integrity_scan_runs (id, status)
        VALUES ($1, 'queued')
        RETURNING id, status
        "#,
        run_id
    )
    .fetch_one(&mut *tx)
    .await?;
    jobs::enqueue_in_tx(
        &mut tx,
        JobSpec::immediate(
            JobKind::IntegrityScan,
            json!({ "integrity_scan_run_id": run_id }),
            format!("integrity-scan:{run_id}"),
        ),
    )
    .await?;
    tx.commit().await?;
    Ok(IntegrityScanRun {
        integrity_scan_run_id: row.id,
        status: row.status,
    })
}

/// Runs one leased integrity-scan job.
pub async fn run_integrity_job(
    pool: &PgPool,
    storage: &ObjectStorage,
    job: &LeasedJob,
) -> Result<(), IntegrityError> {
    if job.kind != JobKind::IntegrityScan {
        return Err(IntegrityError::UnsupportedJobKind);
    }
    let run_id = integrity_scan_run_id(&job.payload)?;
    mark_scan_running(pool, run_id).await?;
    match scan_durable_objects(pool, storage).await {
        Ok(report) => {
            mark_scan_succeeded(pool, run_id, &report).await?;
            mark_corrupt_original_assets(pool, &report).await?;
            Ok(())
        }
        Err(error) => {
            let _ = mark_scan_failed(pool, run_id, &error.to_string()).await;
            Err(error)
        }
    }
}

/// Verifies original and derivative objects without mutating storage.
pub async fn scan_durable_objects(
    pool: &PgPool,
    storage: &ObjectStorage,
) -> Result<ObjectIntegrityReport, IntegrityError> {
    let original_rows = sqlx::query!(
        r#"
        SELECT id, blake3_hash, storage_key, size_bytes
        FROM originals
        ORDER BY storage_key
        "#
    )
    .fetch_all(pool)
    .await?;
    let derivative_rows = sqlx::query!(
        r#"
        SELECT id, storage_key, size_bytes, blake3_hash
        FROM derivatives
        ORDER BY storage_key
        "#
    )
    .fetch_all(pool)
    .await?;

    let mut missing_originals = Vec::new();
    let mut corrupt_originals = Vec::new();
    for row in original_rows {
        let key = StorageKey::new(row.storage_key.clone())
            .map_err(|_| IntegrityError::InvalidListedObjectKey(row.storage_key.clone()))?;
        if !storage.exists(&key).await? {
            missing_originals.push(MissingOriginalObject {
                blake3_hash: row.blake3_hash,
                storage_key: row.storage_key,
            });
            continue;
        }
        let measured = measure_object(storage, &key).await?;
        if measured.blake3_hash != row.blake3_hash || measured.size_bytes != row.size_bytes {
            corrupt_originals.push(CorruptStorageObject {
                row_id: row.id,
                storage_key: row.storage_key,
                expected_blake3: Some(row.blake3_hash),
                actual_blake3: Some(measured.blake3_hash),
                expected_size_bytes: row.size_bytes,
                actual_size_bytes: Some(measured.size_bytes),
                reason: "checksum_or_size_mismatch".to_owned(),
            });
        }
    }

    let mut missing_derivatives = Vec::new();
    let mut corrupt_derivatives = Vec::new();
    for row in derivative_rows {
        let key = StorageKey::new(row.storage_key.clone())
            .map_err(|_| IntegrityError::InvalidListedObjectKey(row.storage_key.clone()))?;
        if !storage.exists(&key).await? {
            missing_derivatives.push(MissingDerivativeObject {
                derivative_id: row.id,
                storage_key: row.storage_key,
            });
            continue;
        }
        let Some(expected_hash) = row.blake3_hash else {
            corrupt_derivatives.push(CorruptStorageObject {
                row_id: row.id,
                storage_key: row.storage_key,
                expected_blake3: None,
                actual_blake3: None,
                expected_size_bytes: row.size_bytes,
                actual_size_bytes: None,
                reason: "missing_stored_checksum".to_owned(),
            });
            continue;
        };
        let measured = measure_object(storage, &key).await?;
        if measured.blake3_hash != expected_hash || measured.size_bytes != row.size_bytes {
            corrupt_derivatives.push(CorruptStorageObject {
                row_id: row.id,
                storage_key: row.storage_key,
                expected_blake3: Some(expected_hash),
                actual_blake3: Some(measured.blake3_hash),
                expected_size_bytes: row.size_bytes,
                actual_size_bytes: Some(measured.size_bytes),
                reason: "checksum_or_size_mismatch".to_owned(),
            });
        }
    }

    Ok(ObjectIntegrityReport {
        missing_originals,
        corrupt_originals,
        missing_derivatives,
        corrupt_derivatives,
    })
}

/// Deletes only caller-selected original objects that remain unreferenced.
///
/// Every selected key is checked against `originals.storage_key` immediately
/// before its storage operation. Keys that gained a database reference after a
/// scan are retained.
pub async fn remediate_original_orphans(
    pool: &PgPool,
    storage: &ObjectStorage,
    selected_keys: &[StorageKey],
) -> Result<OrphanRemediationReport, IntegrityError> {
    let mut unique_keys = BTreeMap::new();
    for key in selected_keys {
        if !key.is_original_blake3_object() {
            return Err(IntegrityError::InvalidRemediationKey(
                key.as_str().to_owned(),
            ));
        }
        unique_keys.insert(key.as_str().to_owned(), key.clone());
    }

    let mut deleted_objects = Vec::new();
    let mut retained_db_backed_objects = Vec::new();
    let mut already_missing_objects = Vec::new();

    for key in unique_keys.into_values() {
        let is_db_backed = sqlx::query_scalar!(
            "SELECT EXISTS (SELECT 1 FROM originals WHERE storage_key = $1)",
            key.as_str()
        )
        .fetch_one(pool)
        .await?;
        if is_db_backed.unwrap_or(false) {
            retained_db_backed_objects.push(key);
            continue;
        }

        if !storage.exists(&key).await? {
            already_missing_objects.push(key);
            continue;
        }

        storage.delete(&key).await?;
        deleted_objects.push(key);
    }

    Ok(OrphanRemediationReport {
        deleted_objects,
        retained_db_backed_objects,
        already_missing_objects,
    })
}

struct MeasuredObject {
    blake3_hash: String,
    size_bytes: i64,
}

async fn measure_object(
    storage: &ObjectStorage,
    key: &StorageKey,
) -> Result<MeasuredObject, IntegrityError> {
    let stream = storage.read_stream(key).await?;
    futures_util::pin_mut!(stream);
    let mut hasher = blake3::Hasher::new();
    let mut size_bytes = 0_i64;
    while let Some(chunk) = stream.try_next().await? {
        let chunk_len = i64::try_from(chunk.len()).map_err(|_| StorageError::ObjectTooLarge)?;
        size_bytes = size_bytes
            .checked_add(chunk_len)
            .ok_or(StorageError::ObjectTooLarge)?;
        hasher.update(&chunk);
    }
    Ok(MeasuredObject {
        blake3_hash: hasher.finalize().to_hex().to_string(),
        size_bytes,
    })
}

async fn mark_scan_running(pool: &PgPool, run_id: Uuid) -> Result<(), IntegrityError> {
    sqlx::query!(
        r#"
        UPDATE integrity_scan_runs
        SET status = 'running', started_at = COALESCE(started_at, now()), updated_at = now()
        WHERE id = $1
        "#,
        run_id
    )
    .execute(pool)
    .await?;
    Ok(())
}

async fn mark_scan_succeeded(
    pool: &PgPool,
    run_id: Uuid,
    report: &ObjectIntegrityReport,
) -> Result<(), IntegrityError> {
    sqlx::query!(
        r#"
        UPDATE integrity_scan_runs
        SET
            status = 'succeeded',
            missing_originals = $2,
            corrupt_originals = $3,
            missing_derivatives = $4,
            corrupt_derivatives = $5,
            error_message = NULL,
            completed_at = now(),
            updated_at = now()
        WHERE id = $1
        "#,
        run_id,
        Json(&report.missing_originals) as _,
        Json(&report.corrupt_originals) as _,
        Json(&report.missing_derivatives) as _,
        Json(&report.corrupt_derivatives) as _
    )
    .execute(pool)
    .await?;
    Ok(())
}

async fn mark_scan_failed(
    pool: &PgPool,
    run_id: Uuid,
    error_message: &str,
) -> Result<(), IntegrityError> {
    let mut message = error_message.chars().take(1_000).collect::<String>();
    if message.is_empty() {
        message.push_str("integrity scan failed");
    }
    sqlx::query!(
        r#"
        UPDATE integrity_scan_runs
        SET status = 'failed', error_message = $2, completed_at = now(), updated_at = now()
        WHERE id = $1
        "#,
        run_id,
        message
    )
    .execute(pool)
    .await?;
    Ok(())
}

async fn mark_corrupt_original_assets(
    pool: &PgPool,
    report: &ObjectIntegrityReport,
) -> Result<(), IntegrityError> {
    let missing_storage_keys = report
        .missing_originals
        .iter()
        .map(|original| original.storage_key.clone())
        .collect::<Vec<_>>();
    let corrupt_original_ids = report
        .corrupt_originals
        .iter()
        .map(|original| original.row_id)
        .collect::<Vec<_>>();
    if missing_storage_keys.is_empty() && corrupt_original_ids.is_empty() {
        return Ok(());
    }

    sqlx::query!(
        r#"
        UPDATE assets
        SET status = 'corrupt'
        WHERE original_id IN (
            SELECT id
            FROM originals
            WHERE storage_key = ANY($1)
               OR id = ANY($2)
        )
        "#,
        &missing_storage_keys,
        &corrupt_original_ids
    )
    .execute(pool)
    .await?;
    Ok(())
}

fn integrity_scan_run_id(value: &serde_json::Value) -> Result<Uuid, IntegrityError> {
    let raw = value
        .get("integrity_scan_run_id")
        .and_then(serde_json::Value::as_str)
        .ok_or(IntegrityError::InvalidJobPayload)?;
    Uuid::parse_str(raw).map_err(|_| IntegrityError::InvalidJobPayload)
}
