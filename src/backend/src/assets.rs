//! Asset/original promotion.
//!
//! C001: storage promotion happens before DB commit. `detect_original_orphan`
//! lets recovery tooling identify promoted objects that lack DB rows.

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use blake3::Hasher;
use serde::Serialize;
use sqlx::{PgPool, Postgres, Transaction};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::jobs::{JobKind, JobSpec, enqueue_in_tx};
use crate::public_derivatives;
use crate::storage::{ObjectStorage, StorageKey};

/// Result of promoting a verified upload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct PromotedUpload {
    /// Stable public asset ID used by first-party API routes.
    pub asset_id: Uuid,
}

/// Asset timeline listing input.
#[derive(Debug)]
pub struct ListAssetsInput {
    /// Owner account ID.
    pub owner_id: i16,
    /// Requested page size. Capped by the backend.
    pub limit: Option<i64>,
    /// Opaque cursor from a previous page.
    pub cursor: Option<String>,
}

/// Trash listing input.
#[derive(Debug)]
pub struct ListTrashedAssetsInput {
    /// Owner account ID.
    pub owner_id: i16,
    /// Requested page size. Capped by the backend.
    pub limit: Option<i64>,
    /// Opaque cursor from a previous page.
    pub cursor: Option<String>,
}

/// One asset in owner timeline order.
#[derive(Debug, Serialize, PartialEq)]
pub struct AssetTimelineItem {
    /// Stable asset ID for API clients.
    pub asset_id: Uuid,
    /// Asset creation time used for timeline ordering.
    pub created_at: OffsetDateTime,
    /// Favorite marker if set.
    pub favorite_at: Option<OffsetDateTime>,
    /// Original BLAKE3 content digest.
    pub original_blake3: String,
    /// Original media type.
    pub media_type: String,
    /// Original byte size.
    pub size_bytes: i64,
    /// First recorded source filename for display only.
    pub original_filename: Option<String>,
    /// Available thumbnail derivative, if generated.
    pub thumbnail: Option<AssetDerivativeView>,
    /// Available preview derivative, if generated.
    pub preview: Option<AssetDerivativeView>,
}

/// One asset in trash order.
#[derive(Debug, Serialize, PartialEq)]
pub struct TrashedAssetTimelineItem {
    /// Stable asset ID for API clients.
    pub asset_id: Uuid,
    /// Asset creation time.
    pub created_at: OffsetDateTime,
    /// When the asset entered trash.
    pub trashed_at: OffsetDateTime,
    /// Favorite marker if set.
    pub favorite_at: Option<OffsetDateTime>,
    /// Original BLAKE3 content digest.
    pub original_blake3: String,
    /// Original media type.
    pub media_type: String,
    /// Original byte size.
    pub size_bytes: i64,
    /// First recorded source filename for display only.
    pub original_filename: Option<String>,
    /// Available thumbnail derivative, if generated.
    pub thumbnail: Option<AssetDerivativeView>,
    /// Available preview derivative, if generated.
    pub preview: Option<AssetDerivativeView>,
}

/// Derivative metadata safe to expose to first-party clients.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AssetDerivativeView {
    /// Encoded format.
    pub format: String,
    /// Pixel width.
    pub width: i32,
    /// Pixel height.
    pub height: i32,
}

/// Cursor-paginated asset timeline page.
#[derive(Debug, Serialize, PartialEq)]
pub struct AssetTimelinePage {
    /// Page items in newest-first order.
    pub items: Vec<AssetTimelineItem>,
    /// Cursor for the next page, if more rows exist.
    pub next_cursor: Option<String>,
}

/// Cursor-paginated trash page.
#[derive(Debug, Serialize, PartialEq)]
pub struct TrashedAssetTimelinePage {
    /// Page items in newest-trashed-first order.
    pub items: Vec<TrashedAssetTimelineItem>,
    /// Cursor for the next page, if more rows exist.
    pub next_cursor: Option<String>,
}

/// Promotion failure.
#[derive(Debug)]
pub enum PromoteError {
    /// Upload is missing or not verified.
    UploadNotVerified,
    /// Staged bytes no longer match verified upload metadata.
    VerificationFailed,
    /// Storage backend failed.
    Storage(crate::storage::StorageError),
    /// Database failed.
    Database(sqlx::Error),
}

impl std::fmt::Display for PromoteError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::UploadNotVerified => "upload is not verified",
            Self::VerificationFailed => "upload verification failed",
            Self::Storage(_) => "asset storage error",
            Self::Database(_) => "asset database error",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for PromoteError {}

/// Asset listing failure.
#[derive(Debug)]
pub enum ListAssetsError {
    /// Page size or cursor was invalid.
    InvalidInput,
    /// Database failed.
    Database(sqlx::Error),
}

impl std::fmt::Display for ListAssetsError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::InvalidInput => "invalid asset list input",
            Self::Database(_) => "asset list database error",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for ListAssetsError {}

/// Asset read failure.
#[derive(Debug)]
pub enum AssetReadError {
    /// Asset or derivative was not found for owner.
    NotFound,
    /// Derivative kind is not supported.
    InvalidInput,
    /// Database failed.
    Database(sqlx::Error),
}

impl std::fmt::Display for AssetReadError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::NotFound => "asset not found",
            Self::InvalidInput => "invalid asset read input",
            Self::Database(_) => "asset read database error",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for AssetReadError {}

/// Asset mutation failure.
#[derive(Debug)]
pub enum AssetMutationError {
    /// Asset was not found for owner.
    NotFound,
    /// Asset must be moved to trash before this mutation.
    NotTrashed,
    /// Database failed.
    Database(sqlx::Error),
}

impl std::fmt::Display for AssetMutationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::NotFound => "asset not found",
            Self::NotTrashed => "asset is not trashed",
            Self::Database(_) => "asset mutation database error",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for AssetMutationError {}

/// Storage metadata for a derivative after owner authorization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetDerivativeBlob {
    /// Internal object storage key.
    pub storage_key: String,
    /// HTTP content type.
    pub content_type: &'static str,
}

/// Promotes a verified staged upload into immutable original storage and asset rows.
///
/// The final object write can outlive a failed DB transaction. Callers should
/// run orphan detection from C001 during integrity checks.
pub async fn promote_verified_upload(
    pool: &PgPool,
    storage: &ObjectStorage,
    owner_id: i16,
    upload_id: Uuid,
) -> Result<PromotedUpload, PromoteError> {
    if let Some(existing) = existing_asset_for_upload(pool, upload_id).await? {
        return Ok(existing);
    }

    let upload = load_verified_upload(pool, owner_id, upload_id).await?;
    let bytes = read_verified_staged_bytes(pool, storage, upload_id, &upload).await?;
    let final_key = StorageKey::original_blake3(&upload.expected_blake3)
        .map_err(|_| PromoteError::VerificationFailed)?;

    if !storage
        .exists(&final_key)
        .await
        .map_err(PromoteError::Storage)?
    {
        storage
            .write(&final_key, bytes)
            .await
            .map_err(PromoteError::Storage)?;
    }

    let mut tx = pool.begin().await.map_err(PromoteError::Database)?;
    let original_id = upsert_original(&mut tx, &upload, final_key.as_str()).await?;
    let asset_id = Uuid::now_v7();
    let asset_public_id = Uuid::now_v7();

    sqlx::query(
        r#"
        INSERT INTO assets (id, public_id, owner_id, original_id)
        VALUES ($1, $2, $3, $4)
        "#,
    )
    .bind(asset_id)
    .bind(asset_public_id)
    .bind(owner_id)
    .bind(original_id)
    .execute(&mut *tx)
    .await
    .map_err(PromoteError::Database)?;

    sqlx::query(
        r#"
        INSERT INTO asset_sources (id, asset_id, source_kind, upload_id, original_filename)
        VALUES ($1, $2, 'upload', $3, $4)
        "#,
    )
    .bind(Uuid::now_v7())
    .bind(asset_id)
    .bind(upload_id)
    .bind(&upload.original_filename)
    .execute(&mut *tx)
    .await
    .map_err(PromoteError::Database)?;

    enqueue_asset_jobs(&mut tx, asset_id).await?;
    tx.commit().await.map_err(PromoteError::Database)?;

    Ok(PromotedUpload {
        asset_id: asset_public_id,
    })
}

/// Detects a content-addressed original object that exists without DB metadata.
pub async fn detect_original_orphan(
    pool: &PgPool,
    storage: &ObjectStorage,
    blake3_hash: &str,
) -> Result<bool, PromoteError> {
    let final_key =
        StorageKey::original_blake3(blake3_hash).map_err(|_| PromoteError::VerificationFailed)?;
    let object_exists = storage
        .exists(&final_key)
        .await
        .map_err(PromoteError::Storage)?;
    let row_exists = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (SELECT 1 FROM originals WHERE blake3_hash = $1)",
    )
    .bind(blake3_hash)
    .fetch_one(pool)
    .await
    .map_err(PromoteError::Database)?;

    Ok(object_exists && !row_exists)
}

/// Lists owner assets in stable newest-first timeline order.
pub async fn list_assets(
    pool: &PgPool,
    input: ListAssetsInput,
) -> Result<AssetTimelinePage, ListAssetsError> {
    let limit = page_limit(input.limit)?;
    let rows = match input.cursor {
        Some(cursor) => {
            let (created_at, public_id) = decode_timeline_cursor(&cursor)?;
            list_assets_after(pool, input.owner_id, limit + 1, created_at, public_id).await?
        }
        None => list_assets_first_page(pool, input.owner_id, limit + 1).await?,
    };

    build_timeline_page(rows, limit)
}

/// Lists owner trash in stable newest-trashed-first order.
pub async fn list_trashed_assets(
    pool: &PgPool,
    input: ListTrashedAssetsInput,
) -> Result<TrashedAssetTimelinePage, ListAssetsError> {
    let limit = page_limit(input.limit)?;
    let rows = match input.cursor {
        Some(cursor) => {
            let (trashed_at, public_id) = decode_timeline_cursor(&cursor)?;
            list_trashed_assets_after(pool, input.owner_id, limit + 1, trashed_at, public_id)
                .await?
        }
        None => list_trashed_assets_first_page(pool, input.owner_id, limit + 1).await?,
    };

    build_trashed_timeline_page(rows, limit)
}

/// Loads derivative storage metadata by public asset ID after owner scoping.
pub async fn load_derivative_blob(
    pool: &PgPool,
    owner_id: i16,
    asset_public_id: Uuid,
    kind: &str,
) -> Result<AssetDerivativeBlob, AssetReadError> {
    if !public_derivatives::public_kind_allowed(kind) {
        return Err(AssetReadError::InvalidInput);
    }
    let row = sqlx::query_as::<_, (String, String)>(
        r#"
        SELECT d.storage_key, d.format
        FROM assets a
        JOIN derivatives d ON d.asset_id = a.id
        WHERE a.owner_id = $1
          AND a.public_id = $2
          AND a.trashed_at IS NULL
          AND d.kind = $3
        ORDER BY d.created_at DESC
        LIMIT 1
        "#,
    )
    .bind(owner_id)
    .bind(asset_public_id)
    .bind(kind)
    .fetch_optional(pool)
    .await
    .map_err(AssetReadError::Database)?
    .ok_or(AssetReadError::NotFound)?;

    Ok(AssetDerivativeBlob {
        storage_key: row.0,
        content_type: public_derivatives::public_format_content_type(&row.1)
            .ok_or(AssetReadError::InvalidInput)?,
    })
}

/// Moves an owner asset to trash without deleting original bytes.
pub async fn trash_asset(
    pool: &PgPool,
    owner_id: i16,
    asset_public_id: Uuid,
) -> Result<(), AssetMutationError> {
    let changed = sqlx::query(
        r#"
        UPDATE assets
        SET trashed_at = COALESCE(trashed_at, now())
        WHERE owner_id = $1
          AND public_id = $2
        "#,
    )
    .bind(owner_id)
    .bind(asset_public_id)
    .execute(pool)
    .await
    .map_err(AssetMutationError::Database)?
    .rows_affected();

    if changed > 0 {
        Ok(())
    } else {
        Err(AssetMutationError::NotFound)
    }
}

/// Restores an owner asset from trash.
pub async fn restore_asset(
    pool: &PgPool,
    owner_id: i16,
    asset_public_id: Uuid,
) -> Result<(), AssetMutationError> {
    let changed = sqlx::query(
        r#"
        UPDATE assets
        SET trashed_at = NULL
        WHERE owner_id = $1
          AND public_id = $2
          AND trashed_at IS NOT NULL
        "#,
    )
    .bind(owner_id)
    .bind(asset_public_id)
    .execute(pool)
    .await
    .map_err(AssetMutationError::Database)?
    .rows_affected();

    if changed > 0 {
        Ok(())
    } else {
        Err(AssetMutationError::NotFound)
    }
}

/// Marks an active owner asset as favorite.
pub async fn favorite_asset(
    pool: &PgPool,
    owner_id: i16,
    asset_public_id: Uuid,
) -> Result<(), AssetMutationError> {
    let changed = sqlx::query(
        r#"
        UPDATE assets
        SET favorite_at = COALESCE(favorite_at, now())
        WHERE owner_id = $1
          AND public_id = $2
          AND trashed_at IS NULL
        "#,
    )
    .bind(owner_id)
    .bind(asset_public_id)
    .execute(pool)
    .await
    .map_err(AssetMutationError::Database)?
    .rows_affected();

    if changed > 0 {
        Ok(())
    } else {
        Err(AssetMutationError::NotFound)
    }
}

/// Removes favorite marker from an active owner asset.
pub async fn unfavorite_asset(
    pool: &PgPool,
    owner_id: i16,
    asset_public_id: Uuid,
) -> Result<(), AssetMutationError> {
    let changed = sqlx::query(
        r#"
        UPDATE assets
        SET favorite_at = NULL
        WHERE owner_id = $1
          AND public_id = $2
          AND trashed_at IS NULL
        "#,
    )
    .bind(owner_id)
    .bind(asset_public_id)
    .execute(pool)
    .await
    .map_err(AssetMutationError::Database)?
    .rows_affected();

    if changed > 0 {
        Ok(())
    } else {
        Err(AssetMutationError::NotFound)
    }
}

/// Permanently removes a trashed owner asset from application state.
///
/// Original object deletion remains an explicit integrity-remediation step so
/// content-addressed objects are never deleted before the database commit that
/// proves they are unreferenced.
pub async fn purge_trashed_asset(
    pool: &PgPool,
    owner_id: i16,
    asset_public_id: Uuid,
) -> Result<(), AssetMutationError> {
    let mut tx = pool.begin().await.map_err(AssetMutationError::Database)?;
    let Some(asset) = sqlx::query_as::<_, (Uuid, Uuid, Option<OffsetDateTime>)>(
        r#"
        SELECT id, original_id, trashed_at
        FROM assets
        WHERE owner_id = $1
          AND public_id = $2
        FOR UPDATE
        "#,
    )
    .bind(owner_id)
    .bind(asset_public_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(AssetMutationError::Database)?
    else {
        return Err(AssetMutationError::NotFound);
    };
    if asset.2.is_none() {
        return Err(AssetMutationError::NotTrashed);
    }

    sqlx::query("DELETE FROM assets WHERE id = $1")
        .bind(asset.0)
        .execute(&mut *tx)
        .await
        .map_err(AssetMutationError::Database)?;

    let remaining_original_refs: i64 =
        sqlx::query_scalar("SELECT count(*) FROM assets WHERE original_id = $1")
            .bind(asset.1)
            .fetch_one(&mut *tx)
            .await
            .map_err(AssetMutationError::Database)?;
    let original_removed = if remaining_original_refs == 0 {
        sqlx::query("DELETE FROM originals WHERE id = $1")
            .bind(asset.1)
            .execute(&mut *tx)
            .await
            .map_err(AssetMutationError::Database)?
            .rows_affected()
            > 0
    } else {
        false
    };

    sqlx::query(
        r#"
        INSERT INTO audit_events (
            actor_kind,
            actor_owner_id,
            action,
            outcome,
            target_kind,
            target_id,
            metadata
        )
        VALUES (
            'owner',
            $1,
            'asset.purge',
            'success',
            'asset',
            $2,
            jsonb_build_object('original_removed', $3)
        )
        "#,
    )
    .bind(owner_id)
    .bind(asset_public_id.to_string())
    .bind(original_removed)
    .execute(&mut *tx)
    .await
    .map_err(AssetMutationError::Database)?;

    tx.commit().await.map_err(AssetMutationError::Database)
}

#[derive(Debug)]
struct VerifiedUpload {
    original_filename: String,
    expected_size: i64,
    expected_blake3: String,
    media_type: String,
}

#[derive(Debug)]
struct UploadPart {
    size_bytes: i64,
    storage_key: String,
    blake3_hash: String,
}

async fn existing_asset_for_upload(
    pool: &PgPool,
    upload_id: Uuid,
) -> Result<Option<PromotedUpload>, PromoteError> {
    sqlx::query_scalar::<_, Uuid>(
        r#"
        SELECT a.public_id
        FROM asset_sources s
        JOIN assets a ON a.id = s.asset_id
        WHERE s.upload_id = $1
        "#,
    )
    .bind(upload_id)
    .fetch_optional(pool)
    .await
    .map(|row| row.map(|asset_id| PromotedUpload { asset_id }))
    .map_err(PromoteError::Database)
}

async fn load_verified_upload(
    pool: &PgPool,
    owner_id: i16,
    upload_id: Uuid,
) -> Result<VerifiedUpload, PromoteError> {
    let row = sqlx::query_as::<_, (String, i64, String, String)>(
        r#"
        SELECT original_filename, expected_size, expected_blake3, media_type
        FROM upload_sessions
        WHERE id = $1
          AND owner_id = $2
          AND status = 'verified'
        "#,
    )
    .bind(upload_id)
    .bind(owner_id)
    .fetch_optional(pool)
    .await
    .map_err(PromoteError::Database)?
    .ok_or(PromoteError::UploadNotVerified)?;

    Ok(VerifiedUpload {
        original_filename: row.0,
        expected_size: row.1,
        expected_blake3: row.2,
        media_type: row.3,
    })
}

async fn read_verified_staged_bytes(
    pool: &PgPool,
    storage: &ObjectStorage,
    upload_id: Uuid,
    upload: &VerifiedUpload,
) -> Result<Vec<u8>, PromoteError> {
    let parts = sqlx::query_as::<_, (i64, String, String)>(
        r#"
        SELECT size_bytes, storage_key, blake3_hash
        FROM upload_parts
        WHERE upload_id = $1
        ORDER BY part_index
        "#,
    )
    .bind(upload_id)
    .fetch_all(pool)
    .await
    .map_err(PromoteError::Database)?
    .into_iter()
    .map(|(size_bytes, storage_key, blake3_hash)| UploadPart {
        size_bytes,
        storage_key,
        blake3_hash,
    })
    .collect::<Vec<_>>();

    let mut hasher = Hasher::new();
    let mut total_size = 0_i64;
    let mut all_bytes = Vec::new();

    for part in parts {
        let key =
            StorageKey::new(part.storage_key).map_err(|_| PromoteError::VerificationFailed)?;
        let bytes = storage.read(&key).await.map_err(PromoteError::Storage)?;
        let bytes_len = i64::try_from(bytes.len()).map_err(|_| PromoteError::VerificationFailed)?;
        if bytes_len != part.size_bytes
            || blake3::hash(&bytes).to_hex().as_str() != part.blake3_hash
        {
            return Err(PromoteError::VerificationFailed);
        }
        total_size += bytes_len;
        hasher.update(&bytes);
        all_bytes.extend(bytes);
    }

    if total_size != upload.expected_size
        || hasher.finalize().to_hex().as_str() != upload.expected_blake3
    {
        return Err(PromoteError::VerificationFailed);
    }

    Ok(all_bytes)
}

async fn upsert_original(
    tx: &mut Transaction<'_, Postgres>,
    upload: &VerifiedUpload,
    storage_key: &str,
) -> Result<Uuid, PromoteError> {
    sqlx::query_scalar::<_, Uuid>(
        r#"
        INSERT INTO originals (id, blake3_hash, storage_key, size_bytes, media_type)
        VALUES ($1, $2, $3, $4, $5)
        ON CONFLICT (blake3_hash)
        DO UPDATE SET blake3_hash = EXCLUDED.blake3_hash
        RETURNING id
        "#,
    )
    .bind(Uuid::now_v7())
    .bind(&upload.expected_blake3)
    .bind(storage_key)
    .bind(upload.expected_size)
    .bind(&upload.media_type)
    .fetch_one(&mut **tx)
    .await
    .map_err(PromoteError::Database)
}

async fn enqueue_asset_jobs(
    tx: &mut Transaction<'_, Postgres>,
    asset_id: Uuid,
) -> Result<(), PromoteError> {
    for kind in [JobKind::ExtractMetadata, JobKind::GenerateDerivatives] {
        let payload = serde_json::json!({ "asset_id": asset_id });
        enqueue_in_tx(
            tx,
            JobSpec::immediate(kind, payload, format!("{}:{asset_id}", kind.as_str())),
        )
        .await
        .map_err(|error| match error {
            crate::jobs::JobError::Database(error) => PromoteError::Database(error),
            crate::jobs::JobError::InvalidKind(_) => PromoteError::VerificationFailed,
        })?;
    }

    Ok(())
}

type AssetTimelineRow = (
    Uuid,
    OffsetDateTime,
    Option<OffsetDateTime>,
    String,
    String,
    i64,
    Option<String>,
    Option<String>,
    Option<i32>,
    Option<i32>,
    Option<String>,
    Option<i32>,
    Option<i32>,
);

type TrashedAssetTimelineRow = (
    Uuid,
    OffsetDateTime,
    OffsetDateTime,
    Option<OffsetDateTime>,
    String,
    String,
    i64,
    Option<String>,
    Option<String>,
    Option<i32>,
    Option<i32>,
    Option<String>,
    Option<i32>,
    Option<i32>,
);

async fn list_assets_first_page(
    pool: &PgPool,
    owner_id: i16,
    limit: i64,
) -> Result<Vec<AssetTimelineRow>, ListAssetsError> {
    sqlx::query_as::<_, AssetTimelineRow>(
        r#"
        SELECT
            a.public_id,
            a.created_at,
            a.favorite_at,
            o.blake3_hash,
            o.media_type,
            o.size_bytes,
            s.original_filename,
            t.format,
            t.width,
            t.height,
            p.format,
            p.width,
            p.height
        FROM assets a
        JOIN originals o ON o.id = a.original_id
        LEFT JOIN LATERAL (
            SELECT original_filename
            FROM asset_sources
            WHERE asset_id = a.id
            ORDER BY created_at ASC
            LIMIT 1
        ) s ON true
        LEFT JOIN LATERAL (
            SELECT format, width, height
            FROM derivatives
            WHERE asset_id = a.id
              AND kind = 'thumbnail'
            ORDER BY created_at DESC
            LIMIT 1
        ) t ON true
        LEFT JOIN LATERAL (
            SELECT format, width, height
            FROM derivatives
            WHERE asset_id = a.id
              AND kind = 'preview'
            ORDER BY created_at DESC
            LIMIT 1
        ) p ON true
        WHERE a.owner_id = $1
          AND a.trashed_at IS NULL
        ORDER BY a.created_at DESC, a.public_id DESC
        LIMIT $2
        "#,
    )
    .bind(owner_id)
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(ListAssetsError::Database)
}

async fn list_trashed_assets_first_page(
    pool: &PgPool,
    owner_id: i16,
    limit: i64,
) -> Result<Vec<TrashedAssetTimelineRow>, ListAssetsError> {
    sqlx::query_as::<_, TrashedAssetTimelineRow>(
        r#"
        SELECT
            a.public_id,
            a.created_at,
            a.trashed_at,
            a.favorite_at,
            o.blake3_hash,
            o.media_type,
            o.size_bytes,
            s.original_filename,
            t.format,
            t.width,
            t.height,
            p.format,
            p.width,
            p.height
        FROM assets a
        JOIN originals o ON o.id = a.original_id
        LEFT JOIN LATERAL (
            SELECT original_filename
            FROM asset_sources
            WHERE asset_id = a.id
            ORDER BY created_at ASC
            LIMIT 1
        ) s ON true
        LEFT JOIN LATERAL (
            SELECT format, width, height
            FROM derivatives
            WHERE asset_id = a.id
              AND kind = 'thumbnail'
            ORDER BY created_at DESC
            LIMIT 1
        ) t ON true
        LEFT JOIN LATERAL (
            SELECT format, width, height
            FROM derivatives
            WHERE asset_id = a.id
              AND kind = 'preview'
            ORDER BY created_at DESC
            LIMIT 1
        ) p ON true
        WHERE a.owner_id = $1
          AND a.trashed_at IS NOT NULL
        ORDER BY a.trashed_at DESC, a.public_id DESC
        LIMIT $2
        "#,
    )
    .bind(owner_id)
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(ListAssetsError::Database)
}

async fn list_assets_after(
    pool: &PgPool,
    owner_id: i16,
    limit: i64,
    cursor_created_at: OffsetDateTime,
    cursor_public_id: Uuid,
) -> Result<Vec<AssetTimelineRow>, ListAssetsError> {
    sqlx::query_as::<_, AssetTimelineRow>(
        r#"
        SELECT
            a.public_id,
            a.created_at,
            a.favorite_at,
            o.blake3_hash,
            o.media_type,
            o.size_bytes,
            s.original_filename,
            t.format,
            t.width,
            t.height,
            p.format,
            p.width,
            p.height
        FROM assets a
        JOIN originals o ON o.id = a.original_id
        LEFT JOIN LATERAL (
            SELECT original_filename
            FROM asset_sources
            WHERE asset_id = a.id
            ORDER BY created_at ASC
            LIMIT 1
        ) s ON true
        LEFT JOIN LATERAL (
            SELECT format, width, height
            FROM derivatives
            WHERE asset_id = a.id
              AND kind = 'thumbnail'
            ORDER BY created_at DESC
            LIMIT 1
        ) t ON true
        LEFT JOIN LATERAL (
            SELECT format, width, height
            FROM derivatives
            WHERE asset_id = a.id
              AND kind = 'preview'
            ORDER BY created_at DESC
            LIMIT 1
        ) p ON true
        WHERE a.owner_id = $1
          AND a.trashed_at IS NULL
          AND (a.created_at, a.public_id) < ($3, $4)
        ORDER BY a.created_at DESC, a.public_id DESC
        LIMIT $2
        "#,
    )
    .bind(owner_id)
    .bind(limit)
    .bind(cursor_created_at)
    .bind(cursor_public_id)
    .fetch_all(pool)
    .await
    .map_err(ListAssetsError::Database)
}

async fn list_trashed_assets_after(
    pool: &PgPool,
    owner_id: i16,
    limit: i64,
    cursor_trashed_at: OffsetDateTime,
    cursor_public_id: Uuid,
) -> Result<Vec<TrashedAssetTimelineRow>, ListAssetsError> {
    sqlx::query_as::<_, TrashedAssetTimelineRow>(
        r#"
        SELECT
            a.public_id,
            a.created_at,
            a.trashed_at,
            a.favorite_at,
            o.blake3_hash,
            o.media_type,
            o.size_bytes,
            s.original_filename,
            t.format,
            t.width,
            t.height,
            p.format,
            p.width,
            p.height
        FROM assets a
        JOIN originals o ON o.id = a.original_id
        LEFT JOIN LATERAL (
            SELECT original_filename
            FROM asset_sources
            WHERE asset_id = a.id
            ORDER BY created_at ASC
            LIMIT 1
        ) s ON true
        LEFT JOIN LATERAL (
            SELECT format, width, height
            FROM derivatives
            WHERE asset_id = a.id
              AND kind = 'thumbnail'
            ORDER BY created_at DESC
            LIMIT 1
        ) t ON true
        LEFT JOIN LATERAL (
            SELECT format, width, height
            FROM derivatives
            WHERE asset_id = a.id
              AND kind = 'preview'
            ORDER BY created_at DESC
            LIMIT 1
        ) p ON true
        WHERE a.owner_id = $1
          AND a.trashed_at IS NOT NULL
          AND (a.trashed_at, a.public_id) < ($3, $4)
        ORDER BY a.trashed_at DESC, a.public_id DESC
        LIMIT $2
        "#,
    )
    .bind(owner_id)
    .bind(limit)
    .bind(cursor_trashed_at)
    .bind(cursor_public_id)
    .fetch_all(pool)
    .await
    .map_err(ListAssetsError::Database)
}

fn build_timeline_page(
    rows: Vec<AssetTimelineRow>,
    limit: i64,
) -> Result<AssetTimelinePage, ListAssetsError> {
    let limit = usize::try_from(limit).map_err(|_| ListAssetsError::InvalidInput)?;
    let has_more = rows.len() > limit;
    let items = rows
        .into_iter()
        .take(limit)
        .map(
            |(
                asset_id,
                created_at,
                favorite_at,
                original_blake3,
                media_type,
                size_bytes,
                original_filename,
                thumbnail_format,
                thumbnail_width,
                thumbnail_height,
                preview_format,
                preview_width,
                preview_height,
            )| AssetTimelineItem {
                asset_id,
                created_at,
                favorite_at,
                original_blake3,
                media_type,
                size_bytes,
                original_filename,
                thumbnail: asset_derivative_view(
                    thumbnail_format,
                    thumbnail_width,
                    thumbnail_height,
                ),
                preview: asset_derivative_view(preview_format, preview_width, preview_height),
            },
        )
        .collect::<Vec<_>>();
    let next_cursor = if has_more {
        items
            .last()
            .map(|item| encode_timeline_cursor(item.created_at, item.asset_id))
    } else {
        None
    };

    Ok(AssetTimelinePage { items, next_cursor })
}

fn build_trashed_timeline_page(
    rows: Vec<TrashedAssetTimelineRow>,
    limit: i64,
) -> Result<TrashedAssetTimelinePage, ListAssetsError> {
    let limit = usize::try_from(limit).map_err(|_| ListAssetsError::InvalidInput)?;
    let has_more = rows.len() > limit;
    let items = rows
        .into_iter()
        .take(limit)
        .map(
            |(
                asset_id,
                created_at,
                trashed_at,
                favorite_at,
                original_blake3,
                media_type,
                size_bytes,
                original_filename,
                thumbnail_format,
                thumbnail_width,
                thumbnail_height,
                preview_format,
                preview_width,
                preview_height,
            )| TrashedAssetTimelineItem {
                asset_id,
                created_at,
                trashed_at,
                favorite_at,
                original_blake3,
                media_type,
                size_bytes,
                original_filename,
                thumbnail: asset_derivative_view(
                    thumbnail_format,
                    thumbnail_width,
                    thumbnail_height,
                ),
                preview: asset_derivative_view(preview_format, preview_width, preview_height),
            },
        )
        .collect::<Vec<_>>();
    let next_cursor = if has_more {
        items
            .last()
            .map(|item| encode_timeline_cursor(item.trashed_at, item.asset_id))
    } else {
        None
    };

    Ok(TrashedAssetTimelinePage { items, next_cursor })
}

fn page_limit(limit: Option<i64>) -> Result<i64, ListAssetsError> {
    const DEFAULT_LIMIT: i64 = 60;
    const MAX_LIMIT: i64 = 200;

    match limit {
        Some(value) if (1..=MAX_LIMIT).contains(&value) => Ok(value),
        Some(_) => Err(ListAssetsError::InvalidInput),
        None => Ok(DEFAULT_LIMIT),
    }
}

fn asset_derivative_view(
    format: Option<String>,
    width: Option<i32>,
    height: Option<i32>,
) -> Option<AssetDerivativeView> {
    Some(AssetDerivativeView {
        format: format?,
        width: width?,
        height: height?,
    })
}

fn encode_timeline_cursor(created_at: OffsetDateTime, public_id: Uuid) -> String {
    URL_SAFE_NO_PAD.encode(format!("{}:{public_id}", created_at.unix_timestamp_nanos()))
}

fn decode_timeline_cursor(cursor: &str) -> Result<(OffsetDateTime, Uuid), ListAssetsError> {
    let bytes = URL_SAFE_NO_PAD
        .decode(cursor)
        .map_err(|_| ListAssetsError::InvalidInput)?;
    let decoded = std::str::from_utf8(&bytes).map_err(|_| ListAssetsError::InvalidInput)?;
    let (timestamp, public_id) = decoded
        .split_once(':')
        .ok_or(ListAssetsError::InvalidInput)?;
    let timestamp = timestamp
        .parse::<i128>()
        .map_err(|_| ListAssetsError::InvalidInput)?;
    let created_at = OffsetDateTime::from_unix_timestamp_nanos(timestamp)
        .map_err(|_| ListAssetsError::InvalidInput)?;
    let public_id = Uuid::parse_str(public_id).map_err(|_| ListAssetsError::InvalidInput)?;

    Ok((created_at, public_id))
}
