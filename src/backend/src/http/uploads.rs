//! Resumable upload routes.
//!
//! C001: completion promotes storage before committing DB asset rows. Recovery
//! tooling must detect orphan originals with `assets::detect_original_orphan`.

use actix_web::{HttpRequest, HttpResponse, delete, get, post, put, web};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    assets::{self, PromotedUpload},
    http::{auth, error::ApiError},
    state::AppState,
    uploads::{self, CreateUploadInput, UploadSessionView},
};

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
    let current = auth::require_current_session(pool, &req).await?;
    auth::require_csrf(pool, &req, current.session_id).await?;

    let upload = uploads::create_upload(
        pool,
        CreateUploadInput {
            owner_id: current.owner_id,
            original_filename: body.original_filename.clone(),
            expected_size: body.expected_size,
            expected_blake3: body.expected_blake3.clone(),
            media_type: body.media_type.clone(),
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
    let current = auth::require_current_session(pool, &req).await?;
    let upload = uploads::get_upload(pool, current.owner_id, path.into_inner()).await?;

    Ok(HttpResponse::Ok().json(upload))
}

/// Writes one upload part.
#[put("/uploads/{upload_id}/parts/{part_index}")]
pub async fn put_part_route(
    state: web::Data<AppState>,
    req: HttpRequest,
    path: web::Path<(Uuid, i32)>,
    body: web::Bytes,
) -> Result<HttpResponse, ApiError> {
    let (pool, storage) = deps(&state)?;
    let current = auth::require_current_session(pool, &req).await?;
    auth::require_csrf(pool, &req, current.session_id).await?;
    let (upload_id, part_index) = path.into_inner();

    uploads::put_part(
        pool,
        storage,
        current.owner_id,
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
    let current = auth::require_current_session(pool, &req).await?;
    auth::require_csrf(pool, &req, current.session_id).await?;
    let upload_id = path.into_inner();
    let upload = uploads::complete_upload(pool, storage, current.owner_id, upload_id).await?;
    let promoted =
        assets::promote_verified_upload(pool, storage, current.owner_id, upload_id).await?;

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
    let current = auth::require_current_session(pool, &req).await?;
    auth::require_csrf(pool, &req, current.session_id).await?;

    uploads::cancel_upload(pool, current.owner_id, path.into_inner()).await?;

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
