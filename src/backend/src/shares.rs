//! Private asset shares.
//!
//! Raw share tokens are returned once and persisted only as token hashes.
//! Public share reads expose asset/derivative metadata, not owner EXIF/GPS.

use sqlx::PgPool;
use thiserror::Error;
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use crate::{
    auth::{OpaqueToken, TokenHash},
    public_derivatives,
    storage::StorageKey,
};

const DEFAULT_SHARE_TTL: Duration = Duration::days(7);
const MAX_SHARE_TTL: Duration = Duration::days(30);

/// Share creation input.
#[derive(Debug)]
pub struct CreateShareInput {
    /// Owner account ID.
    pub owner_id: i16,
    /// Public asset ID from first-party API.
    pub asset_public_id: Uuid,
    /// Optional caller-provided TTL in seconds.
    pub expires_in_seconds: Option<i64>,
    /// Whether this share may download originals. V1 routes do not expose
    /// original bytes yet, but policy is stored up front.
    pub allow_original_download: bool,
}

/// Created share output. Raw token is returned once.
#[derive(Debug)]
pub struct CreatedShare {
    /// Public share ID for owner revocation.
    pub share_id: Uuid,
    /// Raw share token for URL construction.
    pub token: OpaqueToken,
    /// Share expiration.
    pub expires_at: OffsetDateTime,
    /// Original-download policy.
    pub allow_original_download: bool,
}

/// Public share page data.
#[derive(Debug, PartialEq, Eq)]
pub struct ShareView {
    /// Public share ID.
    pub share_id: Uuid,
    /// Public asset ID.
    pub asset_id: Uuid,
    /// Original media type.
    pub media_type: String,
    /// Optional thumbnail derivative.
    pub thumbnail: Option<ShareDerivativeView>,
    /// Optional preview derivative.
    pub preview: Option<ShareDerivativeView>,
    /// Original-download policy.
    pub allow_original_download: bool,
    /// Share expiration.
    pub expires_at: OffsetDateTime,
}

/// Public derivative metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShareDerivativeView {
    /// Encoded format.
    pub format: String,
    /// Pixel width.
    pub width: i32,
    /// Pixel height.
    pub height: i32,
}

/// Public derivative bytes behind a valid share token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShareDerivativeBlob {
    /// Internal object storage key.
    pub storage_key: StorageKey,
    /// HTTP content type.
    pub content_type: &'static str,
}

/// Share operation failure.
#[derive(Debug, Error)]
pub enum ShareError {
    /// Input violates share policy.
    #[error("invalid share input")]
    InvalidInput,
    /// Asset/share does not exist or is not active.
    #[error("share not found")]
    NotFound,
    /// Token generation failed.
    #[error("share token generation failed")]
    TokenGeneration,
    /// Database failed.
    #[error("share database error: {0}")]
    Database(#[from] sqlx::Error),
}

/// Creates one active private share for an owner asset.
pub async fn create_share(
    pool: &PgPool,
    input: CreateShareInput,
) -> Result<CreatedShare, ShareError> {
    let ttl = share_ttl(input.expires_in_seconds)?;
    let expires_at = OffsetDateTime::now_utc() + ttl;
    let asset_id = internal_asset_id(pool, input.owner_id, input.asset_public_id).await?;
    let token = OpaqueToken::generate().map_err(|_| ShareError::TokenGeneration)?;
    let token_hash = token.hash();
    let share_id = Uuid::now_v7();
    let share_public_id = Uuid::now_v7();

    let token_bytes = token_hash.as_bytes().as_slice();
    sqlx::query!(
        r#"
        INSERT INTO asset_shares (
            id,
            public_id,
            owner_id,
            asset_id,
            token_hash,
            allow_original_download,
            expires_at
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        "#,
        share_id,
        share_public_id,
        input.owner_id,
        asset_id,
        token_bytes,
        input.allow_original_download,
        expires_at
    )
    .execute(pool)
    .await
    .map_err(ShareError::Database)?;

    Ok(CreatedShare {
        share_id: share_public_id,
        token,
        expires_at,
        allow_original_download: input.allow_original_download,
    })
}

/// Revokes an owner share by public share ID.
pub async fn revoke_share(
    pool: &PgPool,
    owner_id: i16,
    share_public_id: Uuid,
) -> Result<bool, ShareError> {
    let changed = sqlx::query!(
        r#"
        UPDATE asset_shares
        SET revoked_at = COALESCE(revoked_at, now())
        WHERE owner_id = $1
          AND public_id = $2
          AND revoked_at IS NULL
        "#,
        owner_id,
        share_public_id
    )
    .execute(pool)
    .await
    .map_err(ShareError::Database)?
    .rows_affected();

    Ok(changed > 0)
}

/// Loads public share metadata by raw token.
pub async fn load_share(pool: &PgPool, raw_token: &str) -> Result<ShareView, ShareError> {
    let row = active_share_row(pool, raw_token).await?;
    Ok(ShareView {
        share_id: row.share_public_id,
        asset_id: row.asset_public_id,
        media_type: row.media_type,
        thumbnail: share_derivative_view(
            row.thumbnail_format,
            row.thumbnail_width,
            row.thumbnail_height,
        ),
        preview: share_derivative_view(row.preview_format, row.preview_width, row.preview_height),
        allow_original_download: row.allow_original_download,
        expires_at: row.expires_at,
    })
}

/// Loads derivative storage metadata by raw share token.
pub async fn load_share_derivative_blob(
    pool: &PgPool,
    raw_token: &str,
    kind: &str,
) -> Result<ShareDerivativeBlob, ShareError> {
    if !public_derivatives::public_kind_allowed(kind) {
        return Err(ShareError::InvalidInput);
    }
    let token_hash = TokenHash::from_raw(raw_token);
    let token_bytes = token_hash.as_bytes().as_slice();
    let row = sqlx::query!(
        r#"
        SELECT d.storage_key, d.format
        FROM asset_shares sh
        JOIN derivatives d ON d.asset_id = sh.asset_id
        JOIN assets a ON a.id = sh.asset_id
        WHERE sh.token_hash = $1
          AND sh.revoked_at IS NULL
          AND sh.expires_at > now()
          AND a.trashed_at IS NULL
          AND d.kind = $2
        ORDER BY d.created_at DESC
        LIMIT 1
        "#,
        token_bytes,
        kind
    )
    .fetch_optional(pool)
    .await
    .map_err(ShareError::Database)?
    .ok_or(ShareError::NotFound)?;

    Ok(ShareDerivativeBlob {
        storage_key: StorageKey::new(row.storage_key).map_err(|_| ShareError::NotFound)?,
        content_type: public_derivatives::public_format_content_type(&row.format)
            .ok_or(ShareError::InvalidInput)?,
    })
}

fn share_ttl(expires_in_seconds: Option<i64>) -> Result<Duration, ShareError> {
    let ttl = expires_in_seconds
        .map(Duration::seconds)
        .unwrap_or(DEFAULT_SHARE_TTL);
    if ttl.is_zero() || ttl.is_negative() || ttl > MAX_SHARE_TTL {
        Err(ShareError::InvalidInput)
    } else {
        Ok(ttl)
    }
}

async fn internal_asset_id(
    pool: &PgPool,
    owner_id: i16,
    asset_public_id: Uuid,
) -> Result<Uuid, ShareError> {
    sqlx::query_scalar!(
        r#"
        SELECT id
        FROM assets
        WHERE owner_id = $1
          AND public_id = $2
          AND trashed_at IS NULL
        "#,
        owner_id,
        asset_public_id
    )
    .fetch_optional(pool)
    .await
    .map_err(ShareError::Database)?
    .ok_or(ShareError::NotFound)
}

struct ShareRow {
    share_public_id: Uuid,
    asset_public_id: Uuid,
    media_type: String,
    thumbnail_format: Option<String>,
    thumbnail_width: Option<i32>,
    thumbnail_height: Option<i32>,
    preview_format: Option<String>,
    preview_width: Option<i32>,
    preview_height: Option<i32>,
    allow_original_download: bool,
    expires_at: OffsetDateTime,
}

async fn active_share_row(pool: &PgPool, raw_token: &str) -> Result<ShareRow, ShareError> {
    let token_hash = TokenHash::from_raw(raw_token);
    let token_bytes = token_hash.as_bytes().as_slice();
    sqlx::query!(
        r#"
        SELECT
            sh.public_id,
            a.public_id as asset_public_id,
            o.media_type,
            t.format as thumbnail_format,
            t.width as thumbnail_width,
            t.height as thumbnail_height,
            p.format as preview_format,
            p.width as preview_width,
            p.height as preview_height,
            sh.allow_original_download,
            sh.expires_at
        FROM asset_shares sh
        JOIN assets a ON a.id = sh.asset_id
        JOIN originals o ON o.id = a.original_id
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
        WHERE sh.token_hash = $1
          AND sh.revoked_at IS NULL
          AND sh.expires_at > now()
          AND a.trashed_at IS NULL
        "#,
        token_bytes
    )
    .fetch_optional(pool)
    .await
    .map_err(ShareError::Database)?
    .map(|row| ShareRow {
        share_public_id: row.public_id,
        asset_public_id: row.asset_public_id,
        media_type: row.media_type,
        thumbnail_format: Some(row.thumbnail_format),
        thumbnail_width: Some(row.thumbnail_width),
        thumbnail_height: Some(row.thumbnail_height),
        preview_format: Some(row.preview_format),
        preview_width: Some(row.preview_width),
        preview_height: Some(row.preview_height),
        allow_original_download: row.allow_original_download,
        expires_at: row.expires_at,
    })
    .ok_or(ShareError::NotFound)
}

fn share_derivative_view(
    format: Option<String>,
    width: Option<i32>,
    height: Option<i32>,
) -> Option<ShareDerivativeView> {
    Some(ShareDerivativeView {
        format: format?,
        width: width?,
        height: height?,
    })
}
