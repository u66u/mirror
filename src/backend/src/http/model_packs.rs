//! Model-pack admin routes.

use actix_web::{HttpRequest, HttpResponse, get, post, web};
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    http::{auth, error::ApiError},
    ml::{self, SharedImageTextRuntime},
    models::{self, ModelPackManifest},
    state::AppState,
};

/// Records a runtime self-test result for a model pack.
#[derive(Debug, Deserialize)]
pub struct RecordSelfTestRequest {
    /// Whether the runtime self-test passed.
    pub passed: bool,
    /// Operator/runtime error message for failed self-tests.
    pub error_message: Option<String>,
}

/// Lists model packs.
#[get("/model-packs")]
pub async fn list_model_packs_route(
    state: web::Data<AppState>,
    req: HttpRequest,
) -> Result<HttpResponse, ApiError> {
    let pool = db(&state)?;
    auth::require_owner(pool, &req).await?;
    let packs = models::list_model_packs(pool).await?;

    Ok(HttpResponse::Ok().json(packs))
}

/// Installs model-pack metadata from a validated manifest.
#[post("/model-packs")]
pub async fn install_model_pack_route(
    state: web::Data<AppState>,
    req: HttpRequest,
    manifest: web::Json<ModelPackManifest>,
) -> Result<HttpResponse, ApiError> {
    let pool = db(&state)?;
    auth::require_unsafe_owner(pool, &req).await?;
    let pack = models::install_model_pack(pool, manifest.into_inner()).await?;

    Ok(HttpResponse::Created().json(pack))
}

/// Records model-pack self-test status.
#[post("/model-packs/{model_pack_id}/self-test")]
pub async fn record_model_pack_self_test_route(
    state: web::Data<AppState>,
    req: HttpRequest,
    path: web::Path<Uuid>,
    body: web::Json<RecordSelfTestRequest>,
) -> Result<HttpResponse, ApiError> {
    let pool = db(&state)?;
    auth::require_unsafe_owner(pool, &req).await?;
    let pack = models::record_model_pack_self_test(
        pool,
        path.into_inner(),
        body.passed,
        body.error_message.as_deref(),
    )
    .await?;

    Ok(HttpResponse::Ok().json(pack))
}

/// Runs the model-pack self-tests with the configured image/text runtime.
#[post("/model-packs/{model_pack_id}/self-test/run")]
pub async fn run_model_pack_self_test_route(
    state: web::Data<AppState>,
    req: HttpRequest,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, ApiError> {
    let pool = db(&state)?;
    let storage = storage(&state)?;
    auth::require_unsafe_owner(pool, &req).await?;
    let Some(runtime) = req.app_data::<web::Data<SharedImageTextRuntime>>() else {
        return Err(ApiError::ServiceUnavailable(
            "ml_runtime_unavailable",
            "ml runtime is unavailable",
        ));
    };
    let pack =
        ml::run_model_pack_self_tests(pool, storage, runtime.get_ref(), path.into_inner()).await?;

    Ok(HttpResponse::Ok().json(pack))
}

/// Activates a self-tested model pack for its task kind.
#[post("/model-packs/{model_pack_id}/activate")]
pub async fn activate_model_pack_route(
    state: web::Data<AppState>,
    req: HttpRequest,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, ApiError> {
    let pool = db(&state)?;
    auth::require_unsafe_owner(pool, &req).await?;
    let pack = models::activate_model_pack(pool, path.into_inner()).await?;

    Ok(HttpResponse::Ok().json(pack))
}

/// Starts a reindex run for a self-tested model pack.
#[post("/model-packs/{model_pack_id}/reindex")]
pub async fn start_model_reindex_route(
    state: web::Data<AppState>,
    req: HttpRequest,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, ApiError> {
    let pool = db(&state)?;
    auth::require_unsafe_owner(pool, &req).await?;
    let run = models::start_model_reindex(pool, path.into_inner()).await?;

    Ok(HttpResponse::Accepted().json(run))
}

/// Lists recent reindex runs for one model pack.
#[get("/model-packs/{model_pack_id}/reindex-runs")]
pub async fn list_model_reindex_runs_route(
    state: web::Data<AppState>,
    req: HttpRequest,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, ApiError> {
    let pool = db(&state)?;
    auth::require_owner(pool, &req).await?;
    let runs = models::list_model_reindex_runs(pool, path.into_inner()).await?;

    Ok(HttpResponse::Ok().json(runs))
}

fn db(state: &AppState) -> Result<&sqlx::PgPool, ApiError> {
    state.db.as_ref().ok_or(ApiError::ServiceUnavailable(
        "database_unavailable",
        "database is unavailable",
    ))
}

fn storage(state: &AppState) -> Result<&crate::storage::ObjectStorage, ApiError> {
    state.storage.as_ref().ok_or(ApiError::ServiceUnavailable(
        "storage_unavailable",
        "storage is unavailable",
    ))
}
