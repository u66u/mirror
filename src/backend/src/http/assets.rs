//! Asset timeline routes.
//!
//! Handlers only adapt HTTP/auth into `assets` inputs. Cursor semantics live in
//! the feature module so web and Android share one backend contract.

use actix_web::{HttpRequest, HttpResponse, delete, get, post, web};
use futures_util::TryStreamExt;
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    assets::{self, ListAssetsInput},
    http::{auth, error::ApiError},
    state::AppState,
};

/// Asset list query parameters.
#[derive(Debug, Deserialize)]
pub struct ListAssetsQuery {
    /// Optional page size.
    pub limit: Option<i64>,
    /// Opaque cursor returned by the previous page.
    pub cursor: Option<String>,
}

/// Lists owner assets in newest-first timeline order.
#[get("/assets")]
pub async fn list_assets_route(
    state: web::Data<AppState>,
    req: HttpRequest,
    query: web::Query<ListAssetsQuery>,
) -> Result<HttpResponse, ApiError> {
    let Some(pool) = state.db.as_ref() else {
        return Err(ApiError::ServiceUnavailable(
            "database_unavailable",
            "database is unavailable",
        ));
    };
    let current = auth::require_owner(pool, &req).await?;
    let page = assets::list_assets(
        pool,
        ListAssetsInput {
            owner_id: current.owner_id(),
            limit: query.limit,
            cursor: query.cursor.clone(),
        },
    )
    .await?;

    Ok(HttpResponse::Ok().json(page))
}

/// Lists owner trash in newest-trashed-first order.
#[get("/trash/assets")]
pub async fn list_trashed_assets_route(
    state: web::Data<AppState>,
    req: HttpRequest,
    query: web::Query<ListAssetsQuery>,
) -> Result<HttpResponse, ApiError> {
    let Some(pool) = state.db.as_ref() else {
        return Err(ApiError::ServiceUnavailable(
            "database_unavailable",
            "database is unavailable",
        ));
    };
    let current = auth::require_owner(pool, &req).await?;
    let page = assets::list_trashed_assets(
        pool,
        assets::ListTrashedAssetsInput {
            owner_id: current.owner_id(),
            limit: query.limit,
            cursor: query.cursor.clone(),
        },
    )
    .await?;

    Ok(HttpResponse::Ok().json(page))
}

/// Returns derivative bytes for an owner asset.
#[get("/assets/{asset_id}/derivatives/{kind}")]
pub async fn get_derivative(
    state: web::Data<AppState>,
    req: HttpRequest,
    path: web::Path<(Uuid, String)>,
) -> Result<HttpResponse, ApiError> {
    let Some(pool) = state.db.as_ref() else {
        return Err(ApiError::ServiceUnavailable(
            "database_unavailable",
            "database is unavailable",
        ));
    };
    let Some(storage) = state.storage.as_ref() else {
        return Err(ApiError::ServiceUnavailable(
            "storage_unavailable",
            "storage is unavailable",
        ));
    };
    let current = auth::require_owner(pool, &req).await?;
    let (asset_id, kind) = path.into_inner();
    let derivative =
        assets::load_derivative_blob(pool, current.owner_id(), asset_id, &kind).await?;
    let key =
        crate::storage::StorageKey::new(derivative.storage_key).map_err(|_| ApiError::Internal)?;
    let bytes = storage.read(&key).await.map_err(|_| ApiError::Internal)?;

    // Derivatives are private to the owner but safe to keep client-side for a
    // day, which is what lets apps show already-seen thumbnails while offline.
    Ok(HttpResponse::Ok()
        .insert_header(("cache-control", "private, max-age=86400"))
        .content_type(derivative.content_type)
        .body(bytes))
}

/// Moves an owner asset to trash.
#[delete("/assets/{asset_id}")]
pub async fn trash_asset_route(
    state: web::Data<AppState>,
    req: HttpRequest,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, ApiError> {
    let Some(pool) = state.db.as_ref() else {
        return Err(ApiError::ServiceUnavailable(
            "database_unavailable",
            "database is unavailable",
        ));
    };
    let current = auth::require_unsafe_owner(pool, &req).await?;
    assets::trash_asset(pool, current.owner_id(), path.into_inner()).await?;
    Ok(HttpResponse::NoContent().finish())
}

/// Restores an owner asset from trash.
#[post("/assets/{asset_id}/restore")]
pub async fn restore_asset_route(
    state: web::Data<AppState>,
    req: HttpRequest,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, ApiError> {
    let Some(pool) = state.db.as_ref() else {
        return Err(ApiError::ServiceUnavailable(
            "database_unavailable",
            "database is unavailable",
        ));
    };
    let current = auth::require_unsafe_owner(pool, &req).await?;
    assets::restore_asset(pool, current.owner_id(), path.into_inner()).await?;
    Ok(HttpResponse::NoContent().finish())
}

/// Marks an owner asset as favorite.
#[post("/assets/{asset_id}/favorite")]
pub async fn favorite_asset_route(
    state: web::Data<AppState>,
    req: HttpRequest,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, ApiError> {
    let Some(pool) = state.db.as_ref() else {
        return Err(ApiError::ServiceUnavailable(
            "database_unavailable",
            "database is unavailable",
        ));
    };
    let current = auth::require_unsafe_owner(pool, &req).await?;
    assets::favorite_asset(pool, current.owner_id(), path.into_inner()).await?;
    Ok(HttpResponse::NoContent().finish())
}

/// Removes an owner asset favorite marker.
#[delete("/assets/{asset_id}/favorite")]
pub async fn unfavorite_asset_route(
    state: web::Data<AppState>,
    req: HttpRequest,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, ApiError> {
    let Some(pool) = state.db.as_ref() else {
        return Err(ApiError::ServiceUnavailable(
            "database_unavailable",
            "database is unavailable",
        ));
    };
    let current = auth::require_unsafe_owner(pool, &req).await?;
    assets::unfavorite_asset(pool, current.owner_id(), path.into_inner()).await?;
    Ok(HttpResponse::NoContent().finish())
}

/// Permanently purges a trashed owner asset.
#[delete("/assets/{asset_id}/purge")]
pub async fn purge_asset_route(
    state: web::Data<AppState>,
    req: HttpRequest,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, ApiError> {
    let Some(pool) = state.db.as_ref() else {
        return Err(ApiError::ServiceUnavailable(
            "database_unavailable",
            "database is unavailable",
        ));
    };
    let current = auth::require_unsafe_owner(pool, &req).await?;
    assets::purge_trashed_asset(pool, current.owner_id(), path.into_inner()).await?;
    Ok(HttpResponse::NoContent().finish())
}

/// A validated single byte range, `start..=end` inclusive.
#[derive(Debug, PartialEq, Eq)]
enum RangeRequest {
    /// No usable `Range` header: serve the whole object.
    Full,
    /// One satisfiable range.
    Partial { start: u64, end: u64 },
    /// A single range that lies outside the object.
    Unsatisfiable,
}

/// Parses a `Range: bytes=...` header against an object of `size` bytes.
///
/// Only single ranges are honored; multi-range and malformed headers fall back
/// to a full response, which RFC 9110 permits.
fn parse_range(header: Option<&str>, size: u64) -> RangeRequest {
    let Some(spec) = header.and_then(|value| value.trim().strip_prefix("bytes=")) else {
        return RangeRequest::Full;
    };
    if spec.contains(',') {
        return RangeRequest::Full;
    }
    let Some((first, last)) = spec.split_once('-') else {
        return RangeRequest::Full;
    };
    let (first, last) = (first.trim(), last.trim());
    if size == 0 {
        return RangeRequest::Unsatisfiable;
    }
    let last_byte = size - 1;
    match (first.parse::<u64>(), last.parse::<u64>()) {
        (Ok(start), Ok(end)) if start <= end => {
            if start > last_byte {
                RangeRequest::Unsatisfiable
            } else {
                RangeRequest::Partial {
                    start,
                    end: end.min(last_byte),
                }
            }
        }
        (Ok(start), Err(_)) if last.is_empty() => {
            if start > last_byte {
                RangeRequest::Unsatisfiable
            } else {
                RangeRequest::Partial {
                    start,
                    end: last_byte,
                }
            }
        }
        (Err(_), Ok(suffix)) if first.is_empty() && suffix > 0 => RangeRequest::Partial {
            start: size.saturating_sub(suffix),
            end: last_byte,
        },
        _ => RangeRequest::Full,
    }
}

/// Streams an owner original inline, honoring a single HTTP byte range.
///
/// Players need seekable access to large videos, which the export download
/// route (a full-body attachment with its own quota) does not provide.
#[get("/assets/{asset_id}/original")]
pub async fn get_original(
    state: web::Data<AppState>,
    req: HttpRequest,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, ApiError> {
    let Some(pool) = state.db.as_ref() else {
        return Err(ApiError::ServiceUnavailable(
            "database_unavailable",
            "database is unavailable",
        ));
    };
    let Some(storage) = state.storage.as_ref() else {
        return Err(ApiError::ServiceUnavailable(
            "storage_unavailable",
            "storage is unavailable",
        ));
    };
    let current = auth::require_owner(pool, &req).await?;
    let original = crate::exports::original_blob(
        pool,
        crate::exports::ExportOriginalInput {
            owner_id: current.owner_id(),
            asset_public_id: path.into_inner(),
        },
    )
    .await
    .map_err(|_| ApiError::NotFound("asset_not_found", "asset not found"))?;
    let size = u64::try_from(original.size_bytes).map_err(|_| ApiError::Internal)?;
    let header = req
        .headers()
        .get("range")
        .and_then(|value| value.to_str().ok());

    match parse_range(header, size) {
        RangeRequest::Unsatisfiable => Ok(HttpResponse::RangeNotSatisfiable()
            .insert_header(("content-range", format!("bytes */{size}")))
            .finish()),
        RangeRequest::Partial { start, end } => {
            let stream = storage
                .read_range_stream(&original.storage_key, start, end + 1)
                .await
                .map_err(|_| ApiError::Internal)?;
            Ok(HttpResponse::PartialContent()
                .insert_header(("accept-ranges", "bytes"))
                .insert_header(("content-range", format!("bytes {start}-{end}/{size}")))
                .insert_header(("content-length", (end - start + 1).to_string()))
                .insert_header(("cache-control", "private, no-store"))
                .content_type(original.media_type)
                .streaming(stream.map_err(std::io::Error::other)))
        }
        RangeRequest::Full => {
            let stream = storage
                .read_stream(&original.storage_key)
                .await
                .map_err(|_| ApiError::Internal)?;
            Ok(HttpResponse::Ok()
                .insert_header(("accept-ranges", "bytes"))
                .insert_header(("content-length", size.to_string()))
                .insert_header(("cache-control", "private, no-store"))
                .content_type(original.media_type)
                .streaming(stream.map_err(std::io::Error::other)))
        }
    }
}

#[cfg(test)]
mod range_tests {
    use super::{RangeRequest, parse_range};

    #[test]
    fn parses_single_ranges_and_clamps_the_end() {
        assert_eq!(
            parse_range(Some("bytes=0-99"), 1000),
            RangeRequest::Partial { start: 0, end: 99 }
        );
        assert_eq!(
            parse_range(Some("bytes=900-5000"), 1000),
            RangeRequest::Partial {
                start: 900,
                end: 999
            }
        );
        assert_eq!(
            parse_range(Some("bytes=500-"), 1000),
            RangeRequest::Partial {
                start: 500,
                end: 999
            }
        );
        assert_eq!(
            parse_range(Some("bytes=-100"), 1000),
            RangeRequest::Partial {
                start: 900,
                end: 999
            }
        );
        assert_eq!(
            parse_range(Some("bytes=-5000"), 1000),
            RangeRequest::Partial { start: 0, end: 999 }
        );
    }

    #[test]
    fn rejects_out_of_bounds_and_ignores_unusable_headers() {
        assert_eq!(
            parse_range(Some("bytes=1000-1100"), 1000),
            RangeRequest::Unsatisfiable
        );
        assert_eq!(
            parse_range(Some("bytes=0-1"), 0),
            RangeRequest::Unsatisfiable
        );
        assert_eq!(parse_range(None, 1000), RangeRequest::Full);
        assert_eq!(parse_range(Some("items=0-1"), 1000), RangeRequest::Full);
        assert_eq!(parse_range(Some("bytes=0-1,5-6"), 1000), RangeRequest::Full);
        assert_eq!(parse_range(Some("bytes=9-3"), 1000), RangeRequest::Full);
        assert_eq!(parse_range(Some("bytes=abc"), 1000), RangeRequest::Full);
    }
}
