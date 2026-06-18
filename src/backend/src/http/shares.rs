//! Private share routes.
//!
//! Public share responses add privacy headers and expose only share-safe
//! metadata/derivative bytes.

use actix_web::{HttpRequest, HttpResponse, delete, get, post, web};
use serde::{Deserialize, Serialize};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use crate::{
    http::{auth, error::ApiError},
    rate_limit::{self, QuotaInput},
    shares::{self, CreateShareInput},
    state::AppState,
};

const SHARE_CREATE_ACTION: &str = "share_create";
const SHARE_CREATE_MAX_CREATED_PER_HOUR: i32 = 20;
const SHARE_CREATE_WINDOW: Duration = Duration::hours(1);
const SHARE_CREATE_BLOCK: Duration = Duration::hours(1);

/// Owner share creation body.
#[derive(Debug, Deserialize)]
pub struct CreateShareRequest {
    /// Optional share TTL in seconds.
    pub expires_in_seconds: Option<i64>,
    /// Whether original download is allowed by policy.
    pub allow_original_download: Option<bool>,
}

/// Owner share creation response.
#[derive(Debug, Serialize)]
pub struct CreateShareResponse {
    /// Public share ID for revocation.
    pub share_id: Uuid,
    /// Raw share token returned once.
    pub token: String,
    /// Share expiration.
    pub expires_at: OffsetDateTime,
    /// Original-download policy.
    pub allow_original_download: bool,
}

/// Public share page response.
#[derive(Debug, Serialize)]
pub struct ShareResponse {
    /// Public share ID.
    pub share_id: Uuid,
    /// Public asset ID.
    pub asset_id: Uuid,
    /// Original media type.
    pub media_type: String,
    /// Available thumbnail.
    pub thumbnail: Option<ShareDerivativeResponse>,
    /// Available preview.
    pub preview: Option<ShareDerivativeResponse>,
    /// Original-download policy.
    pub allow_original_download: bool,
    /// Share expiration.
    pub expires_at: OffsetDateTime,
}

/// Public derivative metadata.
#[derive(Debug, Serialize)]
pub struct ShareDerivativeResponse {
    /// Encoded format.
    pub format: String,
    /// Pixel width.
    pub width: i32,
    /// Pixel height.
    pub height: i32,
}

/// Creates a private share for one owner asset.
#[post("/assets/{asset_id}/shares")]
pub async fn create_share_route(
    state: web::Data<AppState>,
    req: HttpRequest,
    path: web::Path<Uuid>,
    body: web::Json<CreateShareRequest>,
) -> Result<HttpResponse, ApiError> {
    let Some(pool) = state.db.as_ref() else {
        return Err(ApiError::ServiceUnavailable(
            "database_unavailable",
            "database is unavailable",
        ));
    };
    let owner = auth::require_unsafe_owner(pool, &req).await?;
    reject_blocked_share_create(&state, pool, owner.owner_id()).await?;
    let created = shares::create_share(
        pool,
        CreateShareInput {
            owner_id: owner.owner_id(),
            asset_public_id: path.into_inner(),
            expires_in_seconds: body.expires_in_seconds,
            allow_original_download: body.allow_original_download.unwrap_or(false),
        },
    )
    .await?;

    Ok(HttpResponse::Created().json(CreateShareResponse {
        share_id: created.share_id,
        token: created.token.expose().to_owned(),
        expires_at: created.expires_at,
        allow_original_download: created.allow_original_download,
    }))
}

/// Revokes one owner share.
#[delete("/shares/{share_id}")]
pub async fn revoke_share_route(
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
    let owner = auth::require_unsafe_owner(pool, &req).await?;
    if shares::revoke_share(pool, owner.owner_id(), path.into_inner()).await? {
        Ok(HttpResponse::NoContent().finish())
    } else {
        Err(ApiError::NotFound("share_not_found", "share not found"))
    }
}

/// Loads public share metadata.
#[get("/shares/{token}")]
pub async fn get_share_route(
    state: web::Data<AppState>,
    path: web::Path<String>,
) -> Result<HttpResponse, ApiError> {
    let Some(pool) = state.db.as_ref() else {
        return Err(ApiError::ServiceUnavailable(
            "database_unavailable",
            "database is unavailable",
        ));
    };
    let share = shares::load_share(pool, &path.into_inner()).await?;
    Ok(privacy_response().json(ShareResponse {
        share_id: share.share_id,
        asset_id: share.asset_id,
        media_type: share.media_type,
        thumbnail: share.thumbnail.map(|d| derivative_response(d)),
        preview: share.preview.map(|d| derivative_response(d)),
        allow_original_download: share.allow_original_download,
        expires_at: share.expires_at,
    }))
}

/// Loads public share derivative bytes.
#[get("/shares/{token}/derivatives/{kind}")]
pub async fn get_share_derivative_route(
    state: web::Data<AppState>,
    path: web::Path<(String, String)>,
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
    let (token, kind) = path.into_inner();
    let derivative = shares::load_share_derivative_blob(pool, &token, &kind).await?;
    let bytes = storage
        .read(&derivative.storage_key)
        .await
        .map_err(|_| ApiError::Internal)?;

    Ok(privacy_response()
        .content_type(derivative.content_type)
        .body(bytes))
}

async fn reject_blocked_share_create(
    state: &AppState,
    pool: &sqlx::PgPool,
    owner_id: i16,
) -> Result<(), ApiError> {
    let key = owner_id.to_string();
    let now = OffsetDateTime::now_utc();
    if rate_limit::is_blocked(
        pool,
        &state.config.rate_limit_secret,
        SHARE_CREATE_ACTION,
        &key,
        now,
    )
    .await
    .map_err(|_| ApiError::Internal)?
    {
        return Err(ApiError::TooManyRequests(
            "rate_limited",
            "too many share creation attempts",
        ));
    }

    let blocked = rate_limit::record_quota_attempt(
        pool,
        &state.config.rate_limit_secret,
        QuotaInput {
            action: SHARE_CREATE_ACTION,
            key: &key,
            now,
            max_attempts: SHARE_CREATE_MAX_CREATED_PER_HOUR + 1,
            window: SHARE_CREATE_WINDOW,
            block_for: SHARE_CREATE_BLOCK,
        },
    )
    .await
    .map_err(|_| ApiError::Internal)?;
    if blocked {
        Err(ApiError::TooManyRequests(
            "rate_limited",
            "too many share creation attempts",
        ))
    } else {
        Ok(())
    }
}

fn derivative_response(value: crate::assets::AssetDerivativeView) -> ShareDerivativeResponse {
    ShareDerivativeResponse {
        format: value.format,
        width: value.width,
        height: value.height,
    }
}

fn privacy_response() -> actix_web::HttpResponseBuilder {
    let mut response = HttpResponse::Ok();
    response
        .insert_header(("cache-control", "private, no-store"))
        .insert_header(("referrer-policy", "no-referrer"))
        .insert_header(("x-robots-tag", "noindex"))
        .insert_header(("x-content-type-options", "nosniff"));
    response
}
