//! Search routes.

use actix_web::{HttpRequest, HttpResponse, get, web};
use serde::Deserialize;

use crate::{
    http::{auth, error::ApiError},
    ml::{self, SharedImageTextRuntime},
    rate_limit::{self, QuotaInput},
    search::{self, SearchAssetsInput},
    semantic_index::SemanticSearchOptions,
    state::AppState,
};

const SEMANTIC_SEARCH_ACTION: &str = "semantic_search";

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
            reject_blocked_semantic_search(&state, pool, &req, current).await?;
            let Some(runtime) = req.app_data::<web::Data<SharedImageTextRuntime>>() else {
                return Err(ApiError::ServiceUnavailable(
                    "semantic_search_unavailable",
                    "semantic search is unavailable",
                ));
            };
            let search_options = SemanticSearchOptions {
                use_ann: state.config.semantic_search.ann_enabled,
                ann_ef_search: state.config.semantic_search.ann_ef_search,
            };
            let hits = ml::semantic_text_search_with_options(
                pool,
                runtime.get_ref(),
                current.owner_id(),
                &query.q,
                query.limit.unwrap_or(search::DEFAULT_LIMIT),
                search_options,
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

async fn reject_blocked_semantic_search(
    state: &AppState,
    pool: &sqlx::PgPool,
    req: &HttpRequest,
    credential: auth::OwnerCredential,
) -> Result<(), ApiError> {
    let key = credential.rate_limit_key(req, &state.config.trusted_proxies);
    let now = time::OffsetDateTime::now_utc();
    if rate_limit::is_blocked(
        pool,
        &state.config.rate_limit_secret,
        SEMANTIC_SEARCH_ACTION,
        &key,
        now,
    )
    .await
    .map_err(|_| ApiError::Internal)?
    {
        return Err(ApiError::TooManyRequests(
            "rate_limited",
            "too many semantic search requests",
        ));
    }

    let quota = state.config.rate_limits.semantic_search;
    let blocked = rate_limit::record_quota_attempt(
        pool,
        &state.config.rate_limit_secret,
        QuotaInput {
            action: SEMANTIC_SEARCH_ACTION,
            key: &key,
            now,
            max_attempts: quota.max_per_window.saturating_add(1),
            window: quota.window,
            block_for: quota.block_for,
        },
    )
    .await
    .map_err(|_| ApiError::Internal)?;
    if blocked {
        Err(ApiError::TooManyRequests(
            "rate_limited",
            "too many semantic search requests",
        ))
    } else {
        Ok(())
    }
}
