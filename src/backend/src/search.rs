//! Owner asset search.
//!
//! V1 starts with exact metadata search. Semantic search plugs into this API
//! surface once a real text embedding runtime is configured.

use serde::Serialize;
use sqlx::PgPool;
use thiserror::Error;
use uuid::Uuid;

use crate::assets::{AssetDerivativeView, AssetTimelineItem};

/// Default search page size.
pub const DEFAULT_LIMIT: i64 = 60;
const MAX_LIMIT: i64 = 200;
const MAX_QUERY_CHARS: usize = 128;

/// Asset search input.
#[derive(Debug)]
pub struct SearchAssetsInput {
    /// Owner account ID.
    pub owner_id: i16,
    /// Filename query.
    pub query: String,
    /// Requested result limit.
    pub limit: Option<i64>,
}

/// Asset search page.
#[derive(Debug, Serialize, PartialEq)]
pub struct AssetSearchPage {
    /// Matching active assets in newest-first order.
    pub items: Vec<AssetTimelineItem>,
}

/// Search failure.
#[derive(Debug, Error)]
pub enum SearchError {
    /// Query or limit violates local search bounds.
    #[error("invalid search input")]
    InvalidInput,
    /// Database failed.
    #[error("search database error: {0}")]
    Database(#[from] sqlx::Error),
}

/// Searches active owner assets by original filename.
pub async fn search_assets(
    pool: &PgPool,
    input: SearchAssetsInput,
) -> Result<AssetSearchPage, SearchError> {
    let query = search_query(&input.query)?;
    let limit = search_limit(input.limit)?;
    let rows = sqlx::query!(
        r#"
        SELECT
            a.public_id,
            a.created_at,
            a.favorite_at,
            o.blake3_hash,
            o.media_type,
            o.size_bytes,
            s.original_filename as "original_filename?",
            t.format AS "thumbnail_format?",
            t.width AS "thumbnail_width?",
            t.height AS "thumbnail_height?",
            p.format AS "preview_format?",
            p.width AS "preview_width?",
            p.height AS "preview_height?"
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
          AND lower(COALESCE(s.original_filename, '')) LIKE $2 ESCAPE '\'
        ORDER BY a.created_at DESC, a.public_id DESC
        LIMIT $3
        "#,
        input.owner_id,
        format!("%{}%", escape_like(&query)),
        limit
    )
    .fetch_all(pool)
    .await?;

    Ok(AssetSearchPage {
        items: rows
            .into_iter()
            .map(|row| AssetTimelineItem {
                asset_id: row.public_id,
                created_at: row.created_at,
                favorite_at: row.favorite_at,
                original_blake3: row.blake3_hash,
                media_type: row.media_type,
                size_bytes: row.size_bytes,
                original_filename: row.original_filename,
                thumbnail: derivative_view(
                    row.thumbnail_format,
                    row.thumbnail_width,
                    row.thumbnail_height,
                ),
                preview: derivative_view(row.preview_format, row.preview_width, row.preview_height),
            })
            .collect(),
    })
}

/// Loads active owner assets by public ID in caller-provided rank order.
pub async fn search_assets_by_public_ids(
    pool: &PgPool,
    owner_id: i16,
    asset_public_ids: &[Uuid],
) -> Result<AssetSearchPage, SearchError> {
    if asset_public_ids.is_empty() {
        return Ok(AssetSearchPage { items: Vec::new() });
    }

    let rows = sqlx::query!(
        r#"
        WITH requested(asset_public_id, ord) AS (
            SELECT * FROM unnest($2::uuid[]) WITH ORDINALITY
        )
        SELECT
            a.public_id,
            a.created_at,
            a.favorite_at,
            o.blake3_hash,
            o.media_type,
            o.size_bytes,
            s.original_filename as "original_filename?",
            t.format AS "thumbnail_format?",
            t.width AS "thumbnail_width?",
            t.height AS "thumbnail_height?",
            p.format AS "preview_format?",
            p.width AS "preview_width?",
            p.height AS "preview_height?"
        FROM requested r
        JOIN assets a
          ON a.public_id = r.asset_public_id
         AND a.owner_id = $1
         AND a.trashed_at IS NULL
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
        ORDER BY r.ord ASC
        "#,
        owner_id,
        asset_public_ids
    )
    .fetch_all(pool)
    .await?;

    Ok(AssetSearchPage {
        items: rows
            .into_iter()
            .map(|row| AssetTimelineItem {
                asset_id: row.public_id,
                created_at: row.created_at,
                favorite_at: row.favorite_at,
                original_blake3: row.blake3_hash,
                media_type: row.media_type,
                size_bytes: row.size_bytes,
                original_filename: row.original_filename,
                thumbnail: derivative_view(
                    row.thumbnail_format,
                    row.thumbnail_width,
                    row.thumbnail_height,
                ),
                preview: derivative_view(row.preview_format, row.preview_width, row.preview_height),
            })
            .collect(),
    })
}

fn derivative_view(
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

fn search_query(query: &str) -> Result<String, SearchError> {
    let query = query.trim().to_lowercase();
    if query.is_empty() || query.chars().count() > MAX_QUERY_CHARS {
        Err(SearchError::InvalidInput)
    } else {
        Ok(query)
    }
}

fn search_limit(limit: Option<i64>) -> Result<i64, SearchError> {
    let limit = limit.unwrap_or(DEFAULT_LIMIT);
    match limit {
        1..=MAX_LIMIT => Ok(limit),
        _ => Err(SearchError::InvalidInput),
    }
}

fn escape_like(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        if matches!(character, '%' | '_' | '\\') {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped
}
