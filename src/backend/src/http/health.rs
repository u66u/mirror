//! Health and readiness routes.
//!
//! `/health` is process liveness only. `/ready` reports dependency readiness;
//! until Postgres/storage tasks exist it reports config readiness only.

use actix_web::{HttpResponse, Responder, get, web};
use serde::Serialize;

use crate::{db, state::AppState};

/// Liveness response for load balancers and humans.
#[derive(Debug, Serialize)]
pub struct HealthResponse {
    /// Stable liveness marker.
    pub status: &'static str,
}

/// Readiness response for dependency checks.
#[derive(Debug, Serialize)]
pub struct ReadyResponse {
    /// Overall readiness marker.
    pub status: &'static str,
    /// Config subsystem readiness.
    pub config: &'static str,
    /// Database subsystem readiness.
    pub database: &'static str,
}

/// Process liveness endpoint. Does not check dependencies.
#[get("/health")]
pub async fn health() -> impl Responder {
    HttpResponse::Ok().json(HealthResponse { status: "ok" })
}

/// Readiness endpoint. Reports DB readiness without affecting `/health`.
#[get("/ready")]
pub async fn ready(state: web::Data<AppState>) -> impl Responder {
    let Some(pool) = state.db.as_ref() else {
        return HttpResponse::ServiceUnavailable().json(ReadyResponse {
            status: "not_ready",
            config: "ready",
            database: "missing",
        });
    };

    match db::ping(pool).await {
        Ok(()) => HttpResponse::Ok().json(ReadyResponse {
            status: "ready",
            config: "ready",
            database: "ready",
        }),
        Err(_) => HttpResponse::ServiceUnavailable().json(ReadyResponse {
            status: "not_ready",
            config: "ready",
            database: "unreachable",
        }),
    }
}
