//! People album and face-review routes.

use actix_web::{HttpRequest, HttpResponse, get, patch, post, web};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    http::{auth, error::ApiError},
    people::{self, PeopleReviewError},
    state::AppState,
};

/// Rename/trust request.
#[derive(Debug, Deserialize)]
pub struct RenamePersonRequest {
    /// Owner-visible person name.
    pub display_name: String,
}

/// Merge request.
#[derive(Debug, Deserialize)]
pub struct MergePeopleRequest {
    /// Source cluster merged into the path target.
    pub source_person_id: Uuid,
}

/// Selected face IDs request.
#[derive(Debug, Deserialize)]
pub struct FaceSelectionRequest {
    /// Face occurrence IDs.
    pub face_ids: Vec<Uuid>,
}

/// Split response.
#[derive(Debug, Serialize)]
pub struct SplitFacesResponse {
    /// New unreviewed person cluster ID.
    pub person_id: Uuid,
}

/// Lists visible owner-local people clusters.
#[get("/people")]
pub async fn list_people_route(
    state: web::Data<AppState>,
    req: HttpRequest,
) -> Result<HttpResponse, ApiError> {
    let Some(pool) = state.db.as_ref() else {
        return Err(ApiError::ServiceUnavailable(
            "database_unavailable",
            "database is unavailable",
        ));
    };
    let owner = auth::require_owner(pool, &req).await?;
    let people = people::list_people(pool, owner.owner_id())
        .await
        .map_err(map_people_error)?;
    Ok(HttpResponse::Ok().json(people))
}

/// Renames and trusts one people cluster.
#[patch("/people/{person_id}")]
pub async fn rename_person_route(
    state: web::Data<AppState>,
    req: HttpRequest,
    path: web::Path<Uuid>,
    body: web::Json<RenamePersonRequest>,
) -> Result<HttpResponse, ApiError> {
    let Some(pool) = state.db.as_ref() else {
        return Err(ApiError::ServiceUnavailable(
            "database_unavailable",
            "database is unavailable",
        ));
    };
    let owner = auth::require_unsafe_owner(pool, &req).await?;
    people::rename_person(
        pool,
        owner.owner_id(),
        path.into_inner(),
        &body.display_name,
    )
    .await
    .map_err(map_people_error)?;
    Ok(HttpResponse::NoContent().finish())
}

/// Hides one people cluster.
#[post("/people/{person_id}/hide")]
pub async fn hide_person_route(
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
    people::hide_person(pool, owner.owner_id(), path.into_inner())
        .await
        .map_err(map_people_error)?;
    Ok(HttpResponse::NoContent().finish())
}

/// Merges source cluster into target cluster.
#[post("/people/{person_id}/merge")]
pub async fn merge_people_route(
    state: web::Data<AppState>,
    req: HttpRequest,
    path: web::Path<Uuid>,
    body: web::Json<MergePeopleRequest>,
) -> Result<HttpResponse, ApiError> {
    let Some(pool) = state.db.as_ref() else {
        return Err(ApiError::ServiceUnavailable(
            "database_unavailable",
            "database is unavailable",
        ));
    };
    let owner = auth::require_unsafe_owner(pool, &req).await?;
    people::merge_people(
        pool,
        owner.owner_id(),
        path.into_inner(),
        body.source_person_id,
    )
    .await
    .map_err(map_people_error)?;
    Ok(HttpResponse::NoContent().finish())
}

/// Splits selected faces into a new unreviewed cluster.
#[post("/people/{person_id}/split")]
pub async fn split_faces_route(
    state: web::Data<AppState>,
    req: HttpRequest,
    path: web::Path<Uuid>,
    body: web::Json<FaceSelectionRequest>,
) -> Result<HttpResponse, ApiError> {
    let Some(pool) = state.db.as_ref() else {
        return Err(ApiError::ServiceUnavailable(
            "database_unavailable",
            "database is unavailable",
        ));
    };
    let owner = auth::require_unsafe_owner(pool, &req).await?;
    let person_id = people::split_faces(pool, owner.owner_id(), path.into_inner(), &body.face_ids)
        .await
        .map_err(map_people_error)?;
    Ok(HttpResponse::Created().json(SplitFacesResponse { person_id }))
}

/// Returns selected faces to the unassigned review pool.
#[post("/people/faces/unassign")]
pub async fn unassign_faces_route(
    state: web::Data<AppState>,
    req: HttpRequest,
    body: web::Json<FaceSelectionRequest>,
) -> Result<HttpResponse, ApiError> {
    let Some(pool) = state.db.as_ref() else {
        return Err(ApiError::ServiceUnavailable(
            "database_unavailable",
            "database is unavailable",
        ));
    };
    let owner = auth::require_unsafe_owner(pool, &req).await?;
    people::unassign_faces(pool, owner.owner_id(), &body.face_ids)
        .await
        .map_err(map_people_error)?;
    Ok(HttpResponse::NoContent().finish())
}

fn map_people_error(error: PeopleReviewError) -> ApiError {
    match error {
        PeopleReviewError::PersonNotFound => {
            ApiError::NotFound("person_not_found", "person not found")
        }
        PeopleReviewError::FaceNotFound => ApiError::NotFound("face_not_found", "face not found"),
        PeopleReviewError::OwnerMismatch => {
            ApiError::NotFound("person_not_found", "person not found")
        }
        PeopleReviewError::InvalidDisplayName => {
            ApiError::BadRequest("invalid_person_name", "invalid person name")
        }
        PeopleReviewError::Database(error) => {
            tracing::error!(
                action = "people_http_error",
                ?error,
                "people database error"
            );
            ApiError::Internal
        }
    }
}
