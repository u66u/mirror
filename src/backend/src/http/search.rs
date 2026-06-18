//! Search routes.

use actix_web::{HttpRequest, HttpResponse, get, web};
use serde::Deserialize;

use crate::{
    http::{auth, error::ApiError},
    search::{self, SearchAssetsInput},
    state::AppState,
};

/// Search query parameters.
#[derive(Debug, Deserialize)]
pub struct SearchQuery {
    /// Filename query for v1 metadata search.
    pub q: String,
    /// Optional page size.
    pub limit: Option<i64>,
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
    let page = search::search_assets(
        pool,
        SearchAssetsInput {
            owner_id: current.owner_id(),
            query: query.q.clone(),
            limit: query.limit,
        },
    )
    .await?;

    Ok(HttpResponse::Ok().json(page))
}
