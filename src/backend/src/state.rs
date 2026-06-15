//! Shared API process state.
//!
//! This boundary holds infrastructure handles. Feature logic should receive the
//! narrow handle it needs instead of depending on Actix app state.

use sqlx::PgPool;

use crate::{auth::SetupState, config::Config, storage::ObjectStorage};

/// Cloneable state inserted into Actix app data.
#[derive(Clone)]
pub struct AppState {
    /// Process config. Secret values inside config must stay redacted.
    pub config: Config,
    /// Optional pool lets `/ready` report missing DB without blocking liveness.
    pub db: Option<PgPool>,
    /// First-run owner setup state. See C002.
    pub setup: SetupState,
    /// Vault object storage. Missing storage disables upload routes.
    pub storage: Option<ObjectStorage>,
}
