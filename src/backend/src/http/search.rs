//! Search routes.

use actix_web::{HttpRequest, HttpResponse, get, web};
use serde::Deserialize;

use crate::{
    http::{auth, error::ApiError},
    ml::{self, SharedImageTextRuntime},
    search::{self, SearchAssetsInput},
    state::AppState,
};

/// Search query parameters.
#[derive(Debug, Deserialize)]
pub struct SearchQuery {
    /// Search query.
    pub q: String,
    /// Optional page size.
    pub limit: Option<i64>,
    /// Search mode: `filename` or `semantic`.
    pub mode: Option<String>,
}

/// Searches owner assets.
#[get("/search")]
pub async fn search_route(
    state: web::Data<AppState>,
    req: HttpRequest,
    query: web::Query<SearchQuery>,
) -> Result<HttpResponse, ApiError> {
    let Some(pool) = state.db.as_ref() else {
        return Err(ApiError::ServiceUnavailable(
            "database_unavailable",
            "database is unavailable",
        ));
    };
    let current = auth::require_owner(pool, &req).await?;
    let page = match query.mode.as_deref().unwrap_or("filename") {
        "filename" => {
            search::search_assets(
                pool,
                SearchAssetsInput {
                    owner_id: current.owner_id(),
                    query: query.q.clone(),
                    limit: query.limit,
                },
            )
            .await?
        }
        "semantic" => {
            let Some(runtime) = req.app_data::<web::Data<SharedImageTextRuntime>>() else {
                return Err(ApiError::ServiceUnavailable(
                    "semantic_search_unavailable",
                    "semantic search is unavailable",
                ));
            };
            let hits = ml::semantic_text_search(
                pool,
                runtime.get_ref(),
                current.owner_id(),
                &query.q,
                query.limit.unwrap_or(search::DEFAULT_LIMIT),
            )
            .await?;
            let asset_ids = hits
                .iter()
                .map(|hit| hit.asset_public_id)
                .collect::<Vec<_>>();
            search::search_assets_by_public_ids(pool, current.owner_id(), &asset_ids).await?
        }
        _ => {
            return Err(ApiError::BadRequest(
                "invalid_search_mode",
                "invalid search mode",
            ));
        }
    };

    Ok(HttpResponse::Ok().json(page))
}
