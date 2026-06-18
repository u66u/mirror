//! First-run setup routes.
//!
//! C002: handlers must not log raw setup tokens or passwords from requests.

use actix_web::{HttpRequest, HttpResponse, post, web};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    auth::{self, OwnerSetupInput},
    http::{client_ip, error::ApiError},
    rate_limit::{self, QuotaInput},
    state::AppState,
};

const SETUP_OWNER_ACTION: &str = "setup_owner";

/// Owner setup request body.
#[derive(Debug, Deserialize)]
pub struct SetupOwnerRequest {
    /// One-time startup setup token.
    pub setup_token: String,
    /// Owner display name.
    pub display_name: String,
    /// Initial owner password.
    pub password: String,
}

/// Owner setup response body.
#[derive(Debug, Serialize)]
pub struct SetupOwnerResponse {
    /// Stable public owner ID.
    pub owner_public_id: Uuid,
}

/// Creates the single owner account during first-run setup.
#[post("/setup/owner")]
pub async fn setup_owner(
    state: web::Data<AppState>,
    req: HttpRequest,
    body: web::Json<SetupOwnerRequest>,
) -> Result<HttpResponse, ApiError> {
    let Some(pool) = state.db.as_ref() else {
        return Err(ApiError::ServiceUnavailable(
            "database_unavailable",
            "database is unavailable",
        ));
    };
    reject_blocked_setup_owner(&state, pool, &req).await?;

    let output = auth::create_owner(
        pool,
        &state.setup,
        OwnerSetupInput {
            setup_token: body.setup_token.clone(),
            display_name: body.display_name.clone(),
            password: body.password.clone(),
        },
    )
    .await?;

    Ok(HttpResponse::Created().json(SetupOwnerResponse {
        owner_public_id: output.owner_public_id,
    }))
}

async fn reject_blocked_setup_owner(
    state: &AppState,
    pool: &sqlx::PgPool,
    req: &HttpRequest,
) -> Result<(), ApiError> {
    let key = client_ip::client_ip(req, &state.config.trusted_proxies)
        .map(|ip| ip.to_string())
        .unwrap_or_else(|| "unknown-peer".to_owned());
    let now = time::OffsetDateTime::now_utc();
    if rate_limit::is_blocked(
        pool,
        &state.config.rate_limit_secret,
        SETUP_OWNER_ACTION,
        &key,
        now,
    )
    .await
    .map_err(|_| ApiError::Internal)?
    {
        return Err(ApiError::TooManyRequests(
            "rate_limited",
            "too many setup attempts",
        ));
    }

    let quota = state.config.rate_limits.setup_owner;
    let blocked = rate_limit::record_quota_attempt(
        pool,
        &state.config.rate_limit_secret,
        QuotaInput {
            action: SETUP_OWNER_ACTION,
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
            "too many setup attempts",
        ))
    } else {
        Ok(())
    }
}
