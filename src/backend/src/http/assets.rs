//! Asset timeline routes.
//!
//! Handlers only adapt HTTP/auth into `assets` inputs. Cursor semantics live in
//! the feature module so web and Android share one backend contract.

use actix_web::{HttpRequest, HttpResponse, delete, get, post, web};
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

    Ok(HttpResponse::Ok()
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
