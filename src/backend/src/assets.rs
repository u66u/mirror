//! Asset/original promotion.
//!
//! C001: storage promotion happens before DB commit. `detect_original_orphan`
//! lets recovery tooling identify promoted objects that lack DB rows.

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use blake3::Hasher;
use bytes::Bytes;
use futures_util::{TryStreamExt, stream};
use serde::Serialize;
use sqlx::{PgPool, Postgres, Transaction};
use thiserror::Error;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::jobs::{JobKind, JobSpec, enqueue_in_tx};
use crate::public_derivatives;
use crate::storage::{ObjectStorage, StorageKey};
use crate::uploads::{self, UploadSessionView, UploadStatus};

/// Result of promoting a verified upload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct PromotedUpload {
    /// Stable public asset ID used by first-party API routes.
    pub asset_id: Uuid,
    /// Internal asset row ID used by backend jobs.
    #[serde(skip)]
    pub asset_internal_id: Uuid,
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
#[derive(Debug, Error)]
pub enum PromoteError {
    /// Upload is missing or not verified.
    #[error("upload is not verified")]
    UploadNotVerified,
    /// Staged bytes no longer match verified upload metadata.
    #[error("upload verification failed")]
    VerificationFailed,
    /// Storage backend failed.
    #[error("asset storage error: {0}")]
    Storage(#[from] crate::storage::StorageError),
    /// Database failed.
    #[error("asset database error: {0}")]
    Database(#[from] sqlx::Error),
}

/// Asset listing failure.
#[derive(Debug, Error)]
pub enum ListAssetsError {
    /// Page size or cursor was invalid.
    #[error("invalid asset list input")]
    InvalidInput,
    /// Database failed.
    #[error("asset list database error: {0}")]
    Database(#[from] sqlx::Error),
}

/// Asset read failure.
#[derive(Debug, Error)]
pub enum AssetReadError {
    /// Asset or derivative was not found for owner.
    #[error("asset not found")]
    NotFound,
    /// Derivative kind is not supported.
    #[error("invalid asset read input")]
    InvalidInput,
    /// Database failed.
    #[error("asset read database error: {0}")]
    Database(#[from] sqlx::Error),
}

/// Asset mutation failure.
#[derive(Debug, Error)]
pub enum AssetMutationError {
    /// Asset was not found for owner.
    #[error("asset not found")]
    NotFound,
    /// Asset must be moved to trash before this mutation.
    #[error("asset is not trashed")]
    NotTrashed,
    /// Database failed.
    #[error("asset mutation database error: {0}")]
    Database(#[from] sqlx::Error),
}

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
    let mut tx = pool.begin().await.map_err(PromoteError::Database)?;
    let upload = load_verified_upload_for_update(&mut tx, owner_id, upload_id).await?;
    if let Some(existing) = existing_asset_for_upload(&mut tx, upload_id).await? {
        tx.commit().await.map_err(PromoteError::Database)?;
        return Ok(existing);
    }

    let final_key = StorageKey::original_blake3(&upload.expected_blake3)
        .map_err(|_| PromoteError::VerificationFailed)?;

    if storage
        .exists(&final_key)
        .await
        .map_err(PromoteError::Storage)?
    {
        verify_staged_upload_parts(&mut tx, storage, upload_id, &upload).await?;
    } else {
        write_staged_upload_to_original(&mut tx, storage, upload_id, &upload, &final_key).await?;
    }

    let promoted =
        insert_asset_for_upload(&mut tx, owner_id, upload_id, &upload, final_key.as_str()).await?;
    tx.commit().await.map_err(PromoteError::Database)?;

    Ok(promoted)
}

/// Verifies an open upload and promotes it without reading staged parts twice.
pub async fn complete_and_promote_upload(
    pool: &PgPool,
    storage: &ObjectStorage,
    owner_id: i16,
    upload_id: Uuid,
) -> Result<(UploadSessionView, PromotedUpload), PromoteError> {
    let mut tx = pool.begin().await.map_err(PromoteError::Database)?;
    let promotable = load_promotable_upload_for_update(&mut tx, owner_id, upload_id).await?;

    if let Some(existing) = existing_asset_for_upload(&mut tx, upload_id).await? {
        if promotable.status == UploadPromotionStatus::Open {
            mark_upload_verified(&mut tx, owner_id, upload_id).await?;
        }
        let parts = upload_parts(&mut tx, upload_id).await?;
        let upload = upload_view(upload_id, &promotable.upload, &parts);
        tx.commit().await.map_err(PromoteError::Database)?;
        return Ok((upload, existing));
    }

    let parts = upload_parts(&mut tx, upload_id).await?;
    validate_upload_parts(&parts, &promotable.upload)?;

    let final_key = StorageKey::original_blake3(&promotable.upload.expected_blake3)
        .map_err(|_| PromoteError::VerificationFailed)?;

    if storage
        .exists(&final_key)
        .await
        .map_err(PromoteError::Storage)?
    {
        verify_staged_upload_part_rows(storage, parts.clone(), &promotable.upload).await?;
    } else {
        write_staged_upload_parts_to_original(
            storage,
            upload_id,
            &promotable.upload,
            &final_key,
            parts.clone(),
        )
        .await?;
    }

    if promotable.status == UploadPromotionStatus::Open {
        mark_upload_verified(&mut tx, owner_id, upload_id).await?;
    }
    let promoted = insert_asset_for_upload(
        &mut tx,
        owner_id,
        upload_id,
        &promotable.upload,
        final_key.as_str(),
    )
    .await?;
    let upload = upload_view(upload_id, &promotable.upload, &parts);
    tx.commit().await.map_err(PromoteError::Database)?;

    Ok((upload, promoted))
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
    let row_exists = sqlx::query_scalar!(
        "SELECT EXISTS (SELECT 1 FROM originals WHERE blake3_hash = $1) AS \"exists!\"",
        blake3_hash
    )
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
    let row = sqlx::query!(
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
        owner_id,
        asset_public_id,
        kind
    )
    .fetch_optional(pool)
    .await
    .map_err(AssetReadError::Database)?
    .ok_or(AssetReadError::NotFound)?;

    Ok(AssetDerivativeBlob {
        storage_key: row.storage_key,
        content_type: public_derivatives::public_format_content_type(&row.format)
            .ok_or(AssetReadError::InvalidInput)?,
    })
}

/// Moves an owner asset to trash without deleting original bytes.
pub async fn trash_asset(
    pool: &PgPool,
    owner_id: i16,
    asset_public_id: Uuid,
) -> Result<(), AssetMutationError> {
    let changed = sqlx::query!(
        r#"
        UPDATE assets
        SET trashed_at = COALESCE(trashed_at, now())
        WHERE owner_id = $1
          AND public_id = $2
        "#,
        owner_id,
        asset_public_id
    )
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
    let changed = sqlx::query!(
        r#"
        UPDATE assets
        SET trashed_at = NULL
        WHERE owner_id = $1
          AND public_id = $2
          AND trashed_at IS NOT NULL
        "#,
        owner_id,
        asset_public_id
    )
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
    let changed = sqlx::query!(
        r#"
        UPDATE assets
        SET favorite_at = COALESCE(favorite_at, now())
        WHERE owner_id = $1
          AND public_id = $2
          AND trashed_at IS NULL
        "#,
        owner_id,
        asset_public_id
    )
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
    let changed = sqlx::query!(
        r#"
        UPDATE assets
        SET favorite_at = NULL
        WHERE owner_id = $1
          AND public_id = $2
          AND trashed_at IS NULL
        "#,
        owner_id,
        asset_public_id
    )
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
    let Some(asset) = sqlx::query!(
        r#"
        SELECT id, original_id, trashed_at
        FROM assets
        WHERE owner_id = $1
          AND public_id = $2
        FOR UPDATE
        "#,
        owner_id,
        asset_public_id
    )
    .fetch_optional(&mut *tx)
    .await
    .map_err(AssetMutationError::Database)?
    else {
        return Err(AssetMutationError::NotFound);
    };
    if asset.trashed_at.is_none() {
        return Err(AssetMutationError::NotTrashed);
    }

    sqlx::query!("DELETE FROM assets WHERE id = $1", asset.id)
        .execute(&mut *tx)
        .await
        .map_err(AssetMutationError::Database)?;

    let remaining_original_refs = sqlx::query_scalar!(
        r#"SELECT count(*) as "count!" FROM assets WHERE original_id = $1"#,
        asset.original_id
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(AssetMutationError::Database)?;
    let original_removed = if remaining_original_refs == 0 {
        sqlx::query!("DELETE FROM originals WHERE id = $1", asset.original_id)
            .execute(&mut *tx)
            .await
            .map_err(AssetMutationError::Database)?
            .rows_affected()
            > 0
    } else {
        false
    };

    sqlx::query!(
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
            jsonb_build_object('original_removed', $3::boolean)
        )
        "#,
        owner_id,
        asset_public_id.to_string(),
        original_removed
    )
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

#[derive(Debug, Clone)]
struct UploadPart {
    part_index: i32,
    size_bytes: i64,
    storage_key: String,
    blake3_hash: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UploadPromotionStatus {
    Open,
    Verified,
}

#[derive(Debug)]
struct PromotableUpload {
    status: UploadPromotionStatus,
    upload: VerifiedUpload,
}

async fn existing_asset_for_upload(
    tx: &mut Transaction<'_, Postgres>,
    upload_id: Uuid,
) -> Result<Option<PromotedUpload>, PromoteError> {
    sqlx::query!(
        r#"
        SELECT a.id, a.public_id
        FROM asset_sources s
        JOIN assets a ON a.id = s.asset_id
        WHERE s.upload_id = $1
        "#,
        upload_id,
    )
    .fetch_optional(&mut **tx)
    .await
    .map(|row| {
        row.map(|row| PromotedUpload {
            asset_id: row.public_id,
            asset_internal_id: row.id,
        })
    })
    .map_err(PromoteError::Database)
}

async fn insert_asset_for_upload(
    tx: &mut Transaction<'_, Postgres>,
    owner_id: i16,
    upload_id: Uuid,
    upload: &VerifiedUpload,
    final_key: &str,
) -> Result<PromotedUpload, PromoteError> {
    let original_id = upsert_original(tx, upload, final_key).await?;
    let asset_id = Uuid::now_v7();
    let asset_public_id = Uuid::now_v7();

    sqlx::query!(
        r#"
        INSERT INTO assets (id, public_id, owner_id, original_id)
        VALUES ($1, $2, $3, $4)
        "#,
        asset_id,
        asset_public_id,
        owner_id,
        original_id
    )
    .execute(&mut **tx)
    .await
    .map_err(PromoteError::Database)?;

    sqlx::query!(
        r#"
        INSERT INTO asset_sources (id, asset_id, source_kind, upload_id, original_filename)
        VALUES ($1, $2, 'upload', $3, $4)
        "#,
        Uuid::now_v7(),
        asset_id,
        upload_id,
        &upload.original_filename
    )
    .execute(&mut **tx)
    .await
    .map_err(PromoteError::Database)?;

    enqueue_asset_jobs(tx, asset_id).await?;

    Ok(PromotedUpload {
        asset_id: asset_public_id,
        asset_internal_id: asset_id,
    })
}

async fn load_promotable_upload_for_update(
    tx: &mut Transaction<'_, Postgres>,
    owner_id: i16,
    upload_id: Uuid,
) -> Result<PromotableUpload, PromoteError> {
    let row = sqlx::query!(
        r#"
        SELECT status, original_filename, expected_size, expected_blake3, media_type
        FROM upload_sessions
        WHERE id = $1
          AND owner_id = $2
        FOR UPDATE
        "#,
        upload_id,
        owner_id
    )
    .fetch_optional(&mut **tx)
    .await
    .map_err(PromoteError::Database)?
    .ok_or(PromoteError::UploadNotVerified)?;

    let status = match row.status.as_str() {
        "open" => UploadPromotionStatus::Open,
        "verified" => UploadPromotionStatus::Verified,
        _ => return Err(PromoteError::UploadNotVerified),
    };

    Ok(PromotableUpload {
        status,
        upload: VerifiedUpload {
            original_filename: row.original_filename,
            expected_size: row.expected_size,
            expected_blake3: row.expected_blake3,
            media_type: row.media_type,
        },
    })
}

async fn load_verified_upload_for_update(
    tx: &mut Transaction<'_, Postgres>,
    owner_id: i16,
    upload_id: Uuid,
) -> Result<VerifiedUpload, PromoteError> {
    let row = sqlx::query!(
        r#"
        SELECT status, original_filename, expected_size, expected_blake3, media_type
        FROM upload_sessions
        WHERE id = $1
          AND owner_id = $2
        FOR UPDATE
        "#,
        upload_id,
        owner_id
    )
    .fetch_optional(&mut **tx)
    .await
    .map_err(PromoteError::Database)?
    .ok_or(PromoteError::UploadNotVerified)?;

    if row.status != "verified" {
        return Err(PromoteError::UploadNotVerified);
    }

    Ok(VerifiedUpload {
        original_filename: row.original_filename,
        expected_size: row.expected_size,
        expected_blake3: row.expected_blake3,
        media_type: row.media_type,
    })
}

async fn upload_parts(
    tx: &mut Transaction<'_, Postgres>,
    upload_id: Uuid,
) -> Result<Vec<UploadPart>, PromoteError> {
    let parts = sqlx::query!(
        r#"
        SELECT part_index, size_bytes, storage_key, blake3_hash
        FROM upload_parts
        WHERE upload_id = $1
        ORDER BY part_index
        "#,
        upload_id,
    )
    .fetch_all(&mut **tx)
    .await
    .map_err(PromoteError::Database)?
    .into_iter()
    .map(|row| UploadPart {
        part_index: row.part_index,
        size_bytes: row.size_bytes,
        storage_key: row.storage_key,
        blake3_hash: row.blake3_hash,
    })
    .collect::<Vec<_>>();

    Ok(parts)
}

fn validate_upload_parts(
    parts: &[UploadPart],
    upload: &VerifiedUpload,
) -> Result<(), PromoteError> {
    let expected_part_count = uploads::expected_part_count(upload.expected_size)
        .ok_or(PromoteError::VerificationFailed)?;
    if i64::try_from(parts.len()).map_err(|_| PromoteError::VerificationFailed)?
        != expected_part_count
    {
        return Err(PromoteError::VerificationFailed);
    }
    for (expected_part_index, part) in parts.iter().enumerate() {
        let expected_part_index =
            i32::try_from(expected_part_index).map_err(|_| PromoteError::VerificationFailed)?;
        let expected_size = uploads::expected_part_size(upload.expected_size, expected_part_index)
            .ok_or(PromoteError::VerificationFailed)?;
        if part.part_index != expected_part_index || part.size_bytes != expected_size {
            return Err(PromoteError::VerificationFailed);
        }
    }
    Ok(())
}

async fn mark_upload_verified(
    tx: &mut Transaction<'_, Postgres>,
    owner_id: i16,
    upload_id: Uuid,
) -> Result<(), PromoteError> {
    let updated = sqlx::query!(
        r#"
        UPDATE upload_sessions
        SET status = 'verified',
            completed_at = COALESCE(completed_at, now()),
            updated_at = now()
        WHERE id = $1
          AND owner_id = $2
          AND status = 'open'
        "#,
        upload_id,
        owner_id
    )
    .execute(&mut **tx)
    .await
    .map_err(PromoteError::Database)?
    .rows_affected();
    if updated == 1 {
        Ok(())
    } else {
        Err(PromoteError::UploadNotVerified)
    }
}

fn upload_view(
    upload_id: Uuid,
    upload: &VerifiedUpload,
    parts: &[UploadPart],
) -> UploadSessionView {
    UploadSessionView {
        upload_id,
        status: UploadStatus::Verified,
        expected_size: upload.expected_size,
        expected_blake3: upload.expected_blake3.clone(),
        media_type: upload.media_type.clone(),
        committed_parts: parts.iter().map(|part| part.part_index).collect(),
    }
}

async fn verify_staged_upload_parts(
    tx: &mut Transaction<'_, Postgres>,
    storage: &ObjectStorage,
    upload_id: Uuid,
    upload: &VerifiedUpload,
) -> Result<(), PromoteError> {
    let parts = upload_parts(tx, upload_id).await?;
    validate_upload_parts(&parts, upload)?;
    verify_staged_upload_part_rows(storage, parts, upload).await
}

async fn verify_staged_upload_part_rows(
    storage: &ObjectStorage,
    parts: Vec<UploadPart>,
    upload: &VerifiedUpload,
) -> Result<(), PromoteError> {
    let stream = verified_part_stream(storage.clone(), parts, upload);
    futures_util::pin_mut!(stream);
    while stream.try_next().await?.is_some() {}
    Ok(())
}

async fn write_staged_upload_to_original(
    tx: &mut Transaction<'_, Postgres>,
    storage: &ObjectStorage,
    upload_id: Uuid,
    upload: &VerifiedUpload,
    final_key: &StorageKey,
) -> Result<(), PromoteError> {
    let parts = upload_parts(tx, upload_id).await?;
    validate_upload_parts(&parts, upload)?;
    write_staged_upload_parts_to_original(storage, upload_id, upload, final_key, parts).await
}

async fn write_staged_upload_parts_to_original(
    storage: &ObjectStorage,
    upload_id: Uuid,
    upload: &VerifiedUpload,
    final_key: &StorageKey,
    parts: Vec<UploadPart>,
) -> Result<(), PromoteError> {
    let temp_key = StorageKey::staging_upload(upload_id, "promoted-original")
        .map_err(|_| PromoteError::VerificationFailed)?;
    let stream = verified_part_stream(storage.clone(), parts, upload);
    let write_result = storage.write_stream(&temp_key, stream).await;
    if let Err(error) = write_result {
        let _ = storage.delete(&temp_key).await;
        return Err(error);
    }

    if let Err(error) = storage.promote(&temp_key, final_key).await {
        let _ = storage.delete(&temp_key).await;
        return Err(PromoteError::Storage(error));
    }

    Ok(())
}

fn verified_part_stream(
    storage: ObjectStorage,
    parts: Vec<UploadPart>,
    upload: &VerifiedUpload,
) -> impl futures_util::Stream<Item = Result<Bytes, PromoteError>> + 'static {
    stream::try_unfold(
        VerifiedPartStreamState {
            storage,
            parts,
            next_index: 0,
            total_size: 0,
            hasher: Hasher::new(),
            expected_size: upload.expected_size,
            expected_blake3: upload.expected_blake3.clone(),
            media_type: upload.media_type.clone(),
            first_bytes: Vec::new(),
        },
        |mut state| async move {
            if state.next_index == state.parts.len() {
                if state.total_size != state.expected_size
                    || state.hasher.finalize().to_hex().as_str() != state.expected_blake3
                    || !uploads::media_signature_matches(&state.media_type, &state.first_bytes)
                {
                    return Err(PromoteError::VerificationFailed);
                }
                return Ok(None);
            }

            let part = &state.parts[state.next_index];
            let key = StorageKey::new(part.storage_key.clone())
                .map_err(|_| PromoteError::VerificationFailed)?;
            let bytes = state
                .storage
                .read(&key)
                .await
                .map_err(PromoteError::Storage)?;
            let bytes_len =
                i64::try_from(bytes.len()).map_err(|_| PromoteError::VerificationFailed)?;
            if bytes_len != part.size_bytes
                || blake3::hash(&bytes).to_hex().as_str() != part.blake3_hash
            {
                return Err(PromoteError::VerificationFailed);
            }
            state.total_size += bytes_len;
            state.hasher.update(&bytes);
            if state.first_bytes.len() < 12 {
                let remaining = 12 - state.first_bytes.len();
                state
                    .first_bytes
                    .extend(bytes.iter().copied().take(remaining));
            }
            state.next_index += 1;

            Ok(Some((Bytes::from(bytes), state)))
        },
    )
}

struct VerifiedPartStreamState {
    storage: ObjectStorage,
    parts: Vec<UploadPart>,
    next_index: usize,
    total_size: i64,
    hasher: Hasher,
    expected_size: i64,
    expected_blake3: String,
    media_type: String,
    first_bytes: Vec<u8>,
}

async fn upsert_original(
    tx: &mut Transaction<'_, Postgres>,
    upload: &VerifiedUpload,
    storage_key: &str,
) -> Result<Uuid, PromoteError> {
    sqlx::query_scalar!(
        r#"
        INSERT INTO originals (id, blake3_hash, storage_key, size_bytes, media_type)
        VALUES ($1, $2, $3, $4, $5)
        ON CONFLICT (blake3_hash)
        DO UPDATE SET blake3_hash = EXCLUDED.blake3_hash
        RETURNING id
        "#,
        Uuid::now_v7(),
        upload.expected_blake3,
        storage_key,
        upload.expected_size,
        upload.media_type
    )
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

struct AssetTimelineRow {
    public_id: Uuid,
    created_at: OffsetDateTime,
    favorite_at: Option<OffsetDateTime>,
    blake3_hash: String,
    media_type: String,
    size_bytes: i64,
    original_filename: Option<String>,
    thumbnail_format: Option<String>,
    thumbnail_width: Option<i32>,
    thumbnail_height: Option<i32>,
    preview_format: Option<String>,
    preview_width: Option<i32>,
    preview_height: Option<i32>,
}

struct TrashedAssetTimelineRow {
    public_id: Uuid,
    created_at: OffsetDateTime,
    trashed_at: OffsetDateTime,
    favorite_at: Option<OffsetDateTime>,
    blake3_hash: String,
    media_type: String,
    size_bytes: i64,
    original_filename: Option<String>,
    thumbnail_format: Option<String>,
    thumbnail_width: Option<i32>,
    thumbnail_height: Option<i32>,
    preview_format: Option<String>,
    preview_width: Option<i32>,
    preview_height: Option<i32>,
}

async fn list_assets_first_page(
    pool: &PgPool,
    owner_id: i16,
    limit: i64,
) -> Result<Vec<AssetTimelineRow>, ListAssetsError> {
    sqlx::query_as!(
        AssetTimelineRow,
        r#"
        SELECT
            asset_public_id as "public_id!",
            created_at as "created_at!",
            favorite_at,
            blake3_hash as "blake3_hash!",
            media_type as "media_type!",
            size_bytes as "size_bytes!",
            original_filename,
            thumbnail_format as "thumbnail_format?",
            thumbnail_width as "thumbnail_width?",
            thumbnail_height as "thumbnail_height?",
            preview_format as "preview_format?",
            preview_width as "preview_width?",
            preview_height as "preview_height?"
        FROM asset_display_view
        WHERE asset_owner_id = $1
          AND trashed_at IS NULL
        ORDER BY created_at DESC, asset_public_id DESC
        LIMIT $2
        "#,
        owner_id,
        limit
    )
    .fetch_all(pool)
    .await
    .map_err(ListAssetsError::Database)
}

async fn list_trashed_assets_first_page(
    pool: &PgPool,
    owner_id: i16,
    limit: i64,
) -> Result<Vec<TrashedAssetTimelineRow>, ListAssetsError> {
    sqlx::query_as!(
        TrashedAssetTimelineRow,
        r#"
        SELECT
            asset_public_id as "public_id!",
            created_at as "created_at!",
            trashed_at as "trashed_at!",
            favorite_at,
            blake3_hash as "blake3_hash!",
            media_type as "media_type!",
            size_bytes as "size_bytes!",
            original_filename,
            thumbnail_format as "thumbnail_format?",
            thumbnail_width as "thumbnail_width?",
            thumbnail_height as "thumbnail_height?",
            preview_format as "preview_format?",
            preview_width as "preview_width?",
            preview_height as "preview_height?"
        FROM asset_display_view
        WHERE asset_owner_id = $1
          AND trashed_at IS NOT NULL
        ORDER BY trashed_at DESC, asset_public_id DESC
        LIMIT $2
        "#,
        owner_id,
        limit
    )
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
    sqlx::query_as!(
        AssetTimelineRow,
        r#"
        SELECT
            asset_public_id as "public_id!",
            created_at as "created_at!",
            favorite_at,
            blake3_hash as "blake3_hash!",
            media_type as "media_type!",
            size_bytes as "size_bytes!",
            original_filename,
            thumbnail_format as "thumbnail_format?",
            thumbnail_width as "thumbnail_width?",
            thumbnail_height as "thumbnail_height?",
            preview_format as "preview_format?",
            preview_width as "preview_width?",
            preview_height as "preview_height?"
        FROM asset_display_view
        WHERE asset_owner_id = $1
          AND trashed_at IS NULL
          AND (created_at, asset_public_id) < ($3, $4)
        ORDER BY created_at DESC, asset_public_id DESC
        LIMIT $2
        "#,
        owner_id,
        limit,
        cursor_created_at,
        cursor_public_id
    )
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
    sqlx::query_as!(
        TrashedAssetTimelineRow,
        r#"
        SELECT
            asset_public_id as "public_id!",
            created_at as "created_at!",
            trashed_at as "trashed_at!",
            favorite_at,
            blake3_hash as "blake3_hash!",
            media_type as "media_type!",
            size_bytes as "size_bytes!",
            original_filename,
            thumbnail_format as "thumbnail_format?",
            thumbnail_width as "thumbnail_width?",
            thumbnail_height as "thumbnail_height?",
            preview_format as "preview_format?",
            preview_width as "preview_width?",
            preview_height as "preview_height?"
        FROM asset_display_view
        WHERE asset_owner_id = $1
          AND trashed_at IS NOT NULL
          AND (trashed_at, asset_public_id) < ($3, $4)
        ORDER BY trashed_at DESC, asset_public_id DESC
        LIMIT $2
        "#,
        owner_id,
        limit,
        cursor_trashed_at,
        cursor_public_id
    )
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
        .map(|row| AssetTimelineItem {
            asset_id: row.public_id,
            created_at: row.created_at,
            favorite_at: row.favorite_at,
            original_blake3: row.blake3_hash,
            media_type: row.media_type,
            size_bytes: row.size_bytes,
            original_filename: row.original_filename,
            thumbnail: asset_derivative_view(
                row.thumbnail_format,
                row.thumbnail_width,
                row.thumbnail_height,
            ),
            preview: asset_derivative_view(
                row.preview_format,
                row.preview_width,
                row.preview_height,
            ),
        })
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
        .map(|row| TrashedAssetTimelineItem {
            asset_id: row.public_id,
            created_at: row.created_at,
            trashed_at: row.trashed_at,
            favorite_at: row.favorite_at,
            original_blake3: row.blake3_hash,
            media_type: row.media_type,
            size_bytes: row.size_bytes,
            original_filename: row.original_filename,
            thumbnail: asset_derivative_view(
                row.thumbnail_format,
                row.thumbnail_width,
                row.thumbnail_height,
            ),
            preview: asset_derivative_view(
                row.preview_format,
                row.preview_width,
                row.preview_height,
            ),
        })
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

pub fn asset_derivative_view(
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
