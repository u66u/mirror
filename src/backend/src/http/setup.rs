//! First-run setup routes.
//!
//! C002: handlers must not log raw setup tokens or passwords from requests.

use actix_web::{HttpResponse, post, web};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    auth::{self, OwnerSetupInput},
    http::error::ApiError,
    state::AppState,
};

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
    body: web::Json<SetupOwnerRequest>,
) -> Result<HttpResponse, ApiError> {
    let Some(pool) = state.db.as_ref() else {
        return Err(ApiError::ServiceUnavailable(
            "database_unavailable",
            "database is unavailable",
        ));
    };

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
