//! Owner asset search.
//!
//! V1 starts with exact metadata search. Semantic search plugs into this API
//! surface once a real text embedding runtime is configured.

use serde::Serialize;
use sqlx::PgPool;
use time::OffsetDateTime;
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
#[derive(Debug)]
pub enum SearchError {
    /// Query or limit violates local search bounds.
    InvalidInput,
    /// Database failed.
    Database(sqlx::Error),
}

impl std::fmt::Display for SearchError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidInput => formatter.write_str("invalid search input"),
            Self::Database(_) => formatter.write_str("search database error"),
        }
    }
}

impl std::error::Error for SearchError {}

/// Searches active owner assets by original filename.
pub async fn search_assets(
    pool: &PgPool,
    input: SearchAssetsInput,
) -> Result<AssetSearchPage, SearchError> {
    let query = search_query(&input.query)?;
    let limit = search_limit(input.limit)?;
    let rows = sqlx::query_as::<_, SearchAssetRow>(
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
          AND lower(COALESCE(s.original_filename, '')) LIKE $2 ESCAPE '\'
        ORDER BY a.created_at DESC, a.public_id DESC
        LIMIT $3
        "#,
    )
    .bind(input.owner_id)
    .bind(format!("%{}%", escape_like(&query)))
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(SearchError::Database)?;

    Ok(AssetSearchPage {
        items: rows.into_iter().map(AssetTimelineItem::from).collect(),
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

    let rows = sqlx::query_as::<_, SearchAssetRow>(
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
            s.original_filename,
            t.format,
            t.width,
            t.height,
            p.format,
            p.width,
            p.height
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
    )
    .bind(owner_id)
    .bind(asset_public_ids)
    .fetch_all(pool)
    .await
    .map_err(SearchError::Database)?;

    Ok(AssetSearchPage {
        items: rows.into_iter().map(AssetTimelineItem::from).collect(),
    })
}

type SearchAssetRow = (
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

impl From<SearchAssetRow> for AssetTimelineItem {
    fn from(row: SearchAssetRow) -> Self {
        let (
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
        ) = row;

        Self {
            asset_id,
            created_at,
            favorite_at,
            original_blake3,
            media_type,
            size_bytes,
            original_filename,
            thumbnail: derivative_view(thumbnail_format, thumbnail_width, thumbnail_height),
            preview: derivative_view(preview_format, preview_width, preview_height),
        }
    }
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
