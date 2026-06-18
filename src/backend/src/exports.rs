//! Owner export manifests.
//!
//! Export manifests describe durable originals without streaming object bytes
//! through request handlers. Archive/download orchestration can build on this
//! stable manifest contract.

use serde::Serialize;
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;
use thiserror::Error;

use crate::storage::StorageKey;

/// Export manifest input.
#[derive(Debug)]
pub struct ExportManifestInput {
    /// Owner account ID.
    pub owner_id: i16,
}

/// Active-original export manifest.
#[derive(Debug, Serialize, PartialEq, Eq, Clone)]
pub struct ExportManifest {
    /// Manifest generation timestamp.
    pub generated_at: OffsetDateTime,
    /// Manifest schema marker.
    pub manifest_version: &'static str,
    /// Exported active asset rows.
    pub items: Vec<ExportManifestItem>,
}

/// One active asset original in an export manifest.
#[derive(Debug, Serialize, PartialEq, Eq, Clone)]
pub struct ExportManifestItem {
    /// Stable public asset ID.
    pub asset_id: Uuid,
    /// Original content digest.
    pub blake3_hash: String,
    /// Durable object-storage key.
    pub storage_key: String,
    /// Original media type.
    pub media_type: String,
    /// Original byte size.
    pub size_bytes: i64,
    /// First recorded source filename.
    pub original_filename: Option<String>,
    /// Asset creation timestamp.
    pub created_at: OffsetDateTime,
}

/// Original byte export input.
#[derive(Debug)]
pub struct ExportOriginalInput {
    /// Owner account ID.
    pub owner_id: i16,
    /// Public asset ID from the manifest.
    pub asset_public_id: Uuid,
}

/// Original byte export metadata.
#[derive(Debug, PartialEq, Eq)]
pub struct ExportOriginal {
    /// Durable object key.
    pub storage_key: StorageKey,
    /// Original media type.
    pub media_type: String,
    /// Original byte size.
    pub size_bytes: i64,
}

/// Export manifest failure.
#[derive(Debug, Error)]
pub enum ExportError {
    /// Asset was not found for owner or is not active.
    #[error("export original not found")]
    NotFound,
    /// Database contained an invalid storage key.
    #[error("export original storage key is invalid")]
    InvalidStorageKey,
    /// Database failed.
    #[error("export database error: {0}")]
    Database(#[from] sqlx::Error),
}

/// Builds an active-original manifest for one owner.
pub async fn original_manifest(
    pool: &PgPool,
    input: ExportManifestInput,
) -> Result<ExportManifest, ExportError> {
    let rows = sqlx::query_as::<
        _,
        (
            Uuid,
            String,
            String,
            String,
            i64,
            Option<String>,
            OffsetDateTime,
        ),
    >(
        r#"
        SELECT
            a.public_id,
            o.blake3_hash,
            o.storage_key,
            o.media_type,
            o.size_bytes,
            s.original_filename,
            a.created_at
        FROM assets a
        JOIN originals o ON o.id = a.original_id
        LEFT JOIN LATERAL (
            SELECT original_filename
            FROM asset_sources
            WHERE asset_id = a.id
            ORDER BY created_at ASC
            LIMIT 1
        ) s ON true
        WHERE a.owner_id = $1
          AND a.trashed_at IS NULL
        ORDER BY a.created_at ASC, a.public_id ASC
        "#,
    )
    .bind(input.owner_id)
    .fetch_all(pool)
    .await?;

    Ok(ExportManifest {
        generated_at: OffsetDateTime::now_utc(),
        manifest_version: "mirror-original-export-v1",
        items: rows
            .into_iter()
            .map(
                |(
                    asset_id,
                    blake3_hash,
                    storage_key,
                    media_type,
                    size_bytes,
                    original_filename,
                    created_at,
                )| ExportManifestItem {
                    asset_id,
                    blake3_hash,
                    storage_key,
                    media_type,
                    size_bytes,
                    original_filename,
                    created_at,
                },
            )
            .collect(),
    })
}

/// Loads active-original storage metadata for export.
pub async fn original_blob(
    pool: &PgPool,
    input: ExportOriginalInput,
) -> Result<ExportOriginal, ExportError> {
    let row = sqlx::query_as::<_, (String, String, i64)>(
        r#"
        SELECT o.storage_key, o.media_type, o.size_bytes
        FROM assets a
        JOIN originals o ON o.id = a.original_id
        WHERE a.owner_id = $1
          AND a.public_id = $2
          AND a.trashed_at IS NULL
        "#,
    )
    .bind(input.owner_id)
    .bind(input.asset_public_id)
    .fetch_optional(pool)
    .await?
    .ok_or(ExportError::NotFound)?;

    Ok(ExportOriginal {
        storage_key: StorageKey::new(row.0).map_err(|_| ExportError::InvalidStorageKey)?,
        media_type: row.1,
        size_bytes: row.2,
    })
}
