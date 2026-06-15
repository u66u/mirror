//! Authentication and first-run owner setup.
//!
//! C002: setup-token handling is intentionally narrow. The raw startup token is
//! logged for the admin once, but only a digest lives in process state.

mod device_tokens;
mod password;
mod sessions;
mod setup_token;
mod tokens;

use sqlx::PgPool;
use uuid::Uuid;

pub use device_tokens::{
    AuthenticatedDeviceToken, DeviceTokenCreateInput, DeviceTokenCreateOutput, DeviceTokenError,
    authenticate_device_token, create_device_token, revoke_device_token,
};
pub use password::{PasswordError, hash_password, verify_password};
pub use sessions::{
    AuthenticatedSession, SessionCreateInput, SessionCreateOutput, SessionError, SessionInfo,
    authenticate_session, create_session, list_sessions, revoke_session, verify_session_csrf,
};
pub use setup_token::{SetupState, SetupTokenError, SetupTokenVerifier};
pub use tokens::{OpaqueToken, TokenError, TokenHash};

/// Owner setup request after HTTP parsing.
#[derive(Debug)]
pub struct OwnerSetupInput {
    /// One-time setup token printed at startup.
    pub setup_token: String,
    /// Owner display name.
    pub display_name: String,
    /// Initial owner password.
    pub password: String,
}

/// Owner setup result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OwnerSetupOutput {
    /// Stable public owner ID.
    pub owner_public_id: Uuid,
}

/// Owner setup failure without leaking secrets.
#[derive(Debug)]
pub enum OwnerSetupError {
    /// Setup is disabled because owner already exists or DB is unavailable.
    SetupUnavailable,
    /// Setup token missing, malformed, wrong, or already consumed.
    InvalidSetupToken,
    /// Display name is outside allowed bounds.
    InvalidDisplayName,
    /// Password does not satisfy local policy.
    InvalidPassword,
    /// Owner row already exists.
    OwnerAlreadyExists,
    /// Database error. Do not expose details to clients.
    Database(sqlx::Error),
}

/// Owner login input after HTTP parsing.
#[derive(Debug)]
pub struct OwnerLoginInput {
    /// Candidate owner password.
    pub password: String,
    /// Browser user-agent metadata.
    pub user_agent: Option<String>,
    /// Human-friendly device/browser label.
    pub device_name: Option<String>,
}

/// Owner login failure.
#[derive(Debug)]
pub enum OwnerLoginError {
    /// Owner does not exist or password is wrong.
    InvalidCredentials,
    /// Session creation failed.
    Session(SessionError),
    /// Database failed.
    Database(sqlx::Error),
}

impl std::fmt::Display for OwnerLoginError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::InvalidCredentials => "invalid credentials",
            Self::Session(_) => "session creation failed",
            Self::Database(_) => "database error",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for OwnerLoginError {}

impl std::fmt::Display for OwnerSetupError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::SetupUnavailable => "owner setup unavailable",
            Self::InvalidSetupToken => "invalid setup token",
            Self::InvalidDisplayName => "invalid display name",
            Self::InvalidPassword => "invalid password",
            Self::OwnerAlreadyExists => "owner already exists",
            Self::Database(_) => "database error",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for OwnerSetupError {}

impl From<PasswordError> for OwnerSetupError {
    fn from(error: PasswordError) -> Self {
        match error {
            PasswordError::InvalidPassword => Self::InvalidPassword,
            PasswordError::HashFailed => Self::InvalidPassword,
        }
    }
}

impl From<SetupTokenError> for OwnerSetupError {
    fn from(_: SetupTokenError) -> Self {
        Self::InvalidSetupToken
    }
}

/// Returns true when the single-owner instance is already claimed.
pub async fn owner_exists(pool: &PgPool) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar::<_, bool>("SELECT EXISTS (SELECT 1 FROM owner_accounts WHERE id = 1)")
        .fetch_one(pool)
        .await
}

/// Verifies owner password and creates a DB-backed web session.
pub async fn login_owner(
    pool: &PgPool,
    input: OwnerLoginInput,
) -> Result<SessionCreateOutput, OwnerLoginError> {
    let owner = sqlx::query_as::<_, (i16, String)>(
        r#"
        SELECT id, password_hash
        FROM owner_accounts
        WHERE id = 1
          AND disabled_at IS NULL
        "#,
    )
    .fetch_optional(pool)
    .await
    .map_err(OwnerLoginError::Database)?;

    let Some((owner_id, password_hash)) = owner else {
        return Err(OwnerLoginError::InvalidCredentials);
    };

    if !verify_password(&input.password, &password_hash) {
        return Err(OwnerLoginError::InvalidCredentials);
    }

    create_session(
        pool,
        SessionCreateInput {
            owner_id,
            user_agent: input.user_agent,
            device_name: input.device_name,
        },
    )
    .await
    .map_err(OwnerLoginError::Session)
}

/// Verifies the current owner password without creating a session.
pub async fn verify_owner_password(pool: &PgPool, password: &str) -> Result<bool, sqlx::Error> {
    let password_hash = sqlx::query_scalar::<_, String>(
        r#"
        SELECT password_hash
        FROM owner_accounts
        WHERE id = 1
          AND disabled_at IS NULL
        "#,
    )
    .fetch_optional(pool)
    .await?;

    Ok(password_hash
        .as_deref()
        .is_some_and(|hash| verify_password(password, hash)))
}

/// Creates the single owner account if setup token and DB invariants allow it.
pub async fn create_owner(
    pool: &PgPool,
    setup: &SetupState,
    input: OwnerSetupInput,
) -> Result<OwnerSetupOutput, OwnerSetupError> {
    setup.verify_available(&input.setup_token)?;
    let display_name = normalize_display_name(&input.display_name)?;
    let password_hash = hash_password(&input.password)?;
    let owner_public_id = Uuid::now_v7();

    let result = sqlx::query(
        r#"
        INSERT INTO owner_accounts (public_id, display_name, password_hash)
        VALUES ($1, $2, $3)
        "#,
    )
    .bind(owner_public_id)
    .bind(display_name)
    .bind(password_hash)
    .execute(pool)
    .await;

    match result {
        Ok(_) => {
            setup.mark_consumed();
            Ok(OwnerSetupOutput { owner_public_id })
        }
        Err(error) if is_owner_conflict(&error) => Err(OwnerSetupError::OwnerAlreadyExists),
        Err(error) => Err(OwnerSetupError::Database(error)),
    }
}

fn normalize_display_name(value: &str) -> Result<String, OwnerSetupError> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.chars().count() > 120 {
        return Err(OwnerSetupError::InvalidDisplayName);
    }
    Ok(trimmed.to_owned())
}

fn is_owner_conflict(error: &sqlx::Error) -> bool {
    let Some(db_error) = error.as_database_error() else {
        return false;
    };
    db_error.code().as_deref() == Some("23505")
}
