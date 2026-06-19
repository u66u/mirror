//! Resumable upload routes.
//!
//! C001: completion promotes storage before committing DB asset rows. Recovery
//! tooling must detect orphan originals with `assets::detect_original_orphan`.

use actix_web::{HttpRequest, HttpResponse, delete, get, post, put, web};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    assets::{self, PromotedUpload},
    config::RateLimitQuota,
    face,
    http::{auth, error::ApiError},
    rate_limit::{self, QuotaInput},
    state::AppState,
    uploads::{self, CreateUploadInput, UPLOAD_PART_SIZE_BYTES, UploadSessionView},
};

const UPLOAD_CREATE_ACTION: &str = "upload_create";
const UPLOAD_PART_ACTION: &str = "upload_part";
const UPLOAD_COMPLETE_ACTION: &str = "upload_complete";

/// Create upload request body.
#[derive(Debug, Deserialize)]
pub struct CreateUploadRequest {
    /// Original client filename for owner display.
    pub original_filename: String,
    /// Expected total size in bytes.
    pub expected_size: i64,
    /// Expected BLAKE3 hex digest.
    pub expected_blake3: String,
    /// Declared supported media type.
    pub media_type: String,
    /// Stable client UUID making request retries idempotent.
    pub client_upload_key: Option<Uuid>,
}

/// Complete upload response.
#[derive(Debug, Serialize)]
pub struct CompleteUploadResponse {
    /// Verified upload session.
    pub upload: UploadSessionView,
    /// Durable asset/original identity created from the upload.
    pub promoted: PromotedUpload,
}

/// Creates a resumable upload session.
#[post("/uploads")]
pub async fn create_upload_route(
    state: web::Data<AppState>,
    req: HttpRequest,
    body: web::Json<CreateUploadRequest>,
) -> Result<HttpResponse, ApiError> {
    let (pool, _) = deps(&state)?;
    let current = auth::require_unsafe_owner(pool, &req).await?;
    reject_blocked_upload(
        &state,
        pool,
        current.owner_id(),
        UPLOAD_CREATE_ACTION,
        state.config.rate_limits.upload_create,
    )
    .await?;

    let upload = uploads::create_upload(
        pool,
        CreateUploadInput {
            owner_id: current.owner_id(),
            original_filename: body.original_filename.clone(),
            expected_size: body.expected_size,
            expected_blake3: body.expected_blake3.clone(),
            media_type: body.media_type.clone(),
            client_upload_key: body.client_upload_key,
        },
    )
    .await?;

    Ok(HttpResponse::Created().json(upload))
}

/// Returns upload state for resume.
#[get("/uploads/{upload_id}")]
pub async fn get_upload_route(
    state: web::Data<AppState>,
    req: HttpRequest,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, ApiError> {
    let (pool, _) = deps(&state)?;
    let current = auth::require_owner(pool, &req).await?;
    let upload = uploads::get_upload(pool, current.owner_id(), path.into_inner()).await?;

    Ok(HttpResponse::Ok().json(upload))
}

/// Writes one upload part.
#[put("/uploads/{upload_id}/parts/{part_index}")]
pub async fn put_part_route(
    state: web::Data<AppState>,
    req: HttpRequest,
    path: web::Path<(Uuid, i32)>,
    body: web::Payload,
) -> Result<HttpResponse, ApiError> {
    let (pool, storage) = deps(&state)?;
    let current = auth::require_unsafe_owner(pool, &req).await?;
    reject_blocked_upload(
        &state,
        pool,
        current.owner_id(),
        UPLOAD_PART_ACTION,
        state.config.rate_limits.upload_part,
    )
    .await?;
    let (upload_id, part_index) = path.into_inner();
    let body = body
        .to_bytes_limited(UPLOAD_PART_SIZE_BYTES)
        .await
        .map_err(|_| {
            ApiError::PayloadTooLarge(
                "upload_part_too_large",
                "upload part exceeds the 4 MiB limit",
            )
        })?
        .map_err(|_| {
            ApiError::BadRequest("invalid_upload_part", "invalid upload part request body")
        })?;

    uploads::put_part(
        pool,
        storage,
        current.owner_id(),
        upload_id,
        part_index,
        body.to_vec(),
    )
    .await?;

    Ok(HttpResponse::NoContent().finish())
}

/// Verifies staged upload bytes and promotes them to a durable asset.
#[post("/uploads/{upload_id}/complete")]
pub async fn complete_upload_route(
    state: web::Data<AppState>,
    req: HttpRequest,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, ApiError> {
    let (pool, storage) = deps(&state)?;
    let current = auth::require_unsafe_owner(pool, &req).await?;
    reject_blocked_upload(
        &state,
        pool,
        current.owner_id(),
        UPLOAD_COMPLETE_ACTION,
        state.config.rate_limits.upload_complete,
    )
    .await?;
    let upload_id = path.into_inner();
    let (upload, promoted) =
        assets::complete_and_promote_upload(pool, storage, current.owner_id(), upload_id).await?;
    if state.config.face_recognition_enabled {
        face::enqueue_face_index(pool, promoted.asset_internal_id)
            .await
            .map_err(|_| ApiError::Internal)?;
    }

    Ok(HttpResponse::Ok().json(CompleteUploadResponse { upload, promoted }))
}

/// Cancels an unfinished upload session.
#[delete("/uploads/{upload_id}")]
pub async fn cancel_upload_route(
    state: web::Data<AppState>,
    req: HttpRequest,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, ApiError> {
    let (pool, _) = deps(&state)?;
    let current = auth::require_unsafe_owner(pool, &req).await?;

    uploads::cancel_upload(pool, current.owner_id(), path.into_inner()).await?;

    Ok(HttpResponse::NoContent().finish())
}

fn deps(state: &AppState) -> Result<(&sqlx::PgPool, &crate::storage::ObjectStorage), ApiError> {
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
    Ok((pool, storage))
}

async fn reject_blocked_upload(
    state: &AppState,
    pool: &sqlx::PgPool,
    owner_id: i16,
    action: &'static str,
    quota: RateLimitQuota,
) -> Result<(), ApiError> {
    let key = owner_id.to_string();
    let now = time::OffsetDateTime::now_utc();
    if rate_limit::is_blocked(pool, &state.config.rate_limit_secret, action, &key, now)
        .await
        .map_err(|_| ApiError::Internal)?
    {
        return Err(ApiError::TooManyRequests(
            "rate_limited",
            "too many upload requests",
        ));
    }

    let blocked = rate_limit::record_quota_attempt(
        pool,
        &state.config.rate_limit_secret,
        QuotaInput {
            action,
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
            "too many upload requests",
        ))
    } else {
        Ok(())
    }
}
