//! Original-object integrity scanning and explicit orphan remediation.

use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

use sqlx::PgPool;

use crate::storage::{ObjectStorage, StorageError, StorageKey};

/// One database-backed original whose object was absent from storage.
#[derive(Debug, Clone, PartialEq, Eq)]
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
