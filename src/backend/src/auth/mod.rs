//! Authentication and first-run owner setup.
//!
//! C002: setup-token handling is intentionally narrow. The raw startup token is
//! logged for the admin once, but only a digest lives in process state.

mod device_tokens;
mod mfa;
mod password;
mod sessions;
mod setup_token;
mod tokens;

use crate::config::AuthSecret;
use sqlx::PgPool;
use thiserror::Error;
use uuid::Uuid;

pub use device_tokens::{
    AuthenticatedDeviceToken, DeviceTokenCreateInput, DeviceTokenCreateOutput, DeviceTokenError,
    authenticate_device_token, create_device_token, revoke_device_token, revoke_owner_device_token,
};
pub use mfa::{
    MfaError, MfaStatus, RecoveryCodesOutput, SecondFactorInput, TotpSetupOutput, begin_totp_setup,
    disable_totp, enable_totp, mfa_status, rotate_recovery_codes, second_factor_enabled,
    verify_second_factor,
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
#[derive(Debug, Error)]
pub enum OwnerSetupError {
    /// Setup is disabled because owner already exists or DB is unavailable.
    #[error("owner setup unavailable")]
    SetupUnavailable,
    /// Setup token missing, malformed, wrong, or already consumed.
    #[error("invalid setup token")]
    InvalidSetupToken,
    /// Display name is outside allowed bounds.
    #[error("invalid display name")]
    InvalidDisplayName,
    /// Password does not satisfy local policy.
    #[error("invalid password")]
    InvalidPassword,
    /// Owner row already exists.
    #[error("owner already exists")]
    OwnerAlreadyExists,
    /// Database error. Do not expose details to clients.
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
}

/// Owner login input after HTTP parsing.
#[derive(Debug)]
pub struct OwnerLoginInput {
    /// Candidate owner password.
    pub password: String,
    /// Optional TOTP or recovery code, required when TOTP is enabled.
    pub second_factor: Option<SecondFactorInput>,
    /// Browser user-agent metadata.
    pub user_agent: Option<String>,
    /// Human-friendly device/browser label.
    pub device_name: Option<String>,
}

/// Owner login failure.
#[derive(Debug, Error)]
pub enum OwnerLoginError {
    /// Owner does not exist or password is wrong.
    #[error("invalid credentials")]
    InvalidCredentials,
    /// Owner password was valid, but TOTP/recovery proof is required.
    #[error("second factor required")]
    SecondFactorRequired,
    /// Owner password was valid, but TOTP/recovery proof failed.
    #[error("invalid second factor")]
    InvalidSecondFactor,
    /// Session creation failed.
    #[error("session creation failed: {0}")]
    Session(#[from] SessionError),
    /// MFA verification failed internally.
    #[error("mfa failed: {0}")]
    Mfa(#[from] MfaError),
    /// Database failed.
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
}

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
    sqlx::query_scalar!(r#"SELECT EXISTS (SELECT 1 FROM owner_accounts WHERE id = 1) AS "exists!""#)
        .fetch_one(pool)
        .await
}

/// Verifies owner password and creates a DB-backed web session.
pub async fn login_owner(
    pool: &PgPool,
    auth_secret: &AuthSecret,
    input: OwnerLoginInput,
) -> Result<SessionCreateOutput, OwnerLoginError> {
    let owner = sqlx::query!(
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

    let Some(owner) = owner else {
        return Err(OwnerLoginError::InvalidCredentials);
    };

    if !verify_password(&input.password, &owner.password_hash) {
        return Err(OwnerLoginError::InvalidCredentials);
    }

    if second_factor_enabled(pool).await? {
        let Some(second_factor) = input.second_factor else {
            return Err(OwnerLoginError::InvalidCredentials);
        };
        match verify_second_factor(pool, auth_secret, second_factor).await {
            Ok(()) => {}
            Err(MfaError::SecondFactorRequired | MfaError::InvalidSecondFactor) => {
                return Err(OwnerLoginError::InvalidCredentials);
            }
            Err(error) => return Err(OwnerLoginError::Mfa(error)),
        }
    }

    create_session(
        pool,
        SessionCreateInput {
            owner_id: owner.id,
            user_agent: input.user_agent,
            device_name: input.device_name,
        },
    )
    .await
    .map_err(OwnerLoginError::Session)
}

/// Verifies the current owner password without creating a session.
pub async fn verify_owner_password(pool: &PgPool, password: &str) -> Result<bool, sqlx::Error> {
    let password_hash = sqlx::query_scalar!(
        r#"
        SELECT password_hash
        FROM owner_accounts
        WHERE id = 1
          AND disabled_at IS NULL
        "#
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

    let result = sqlx::query!(
        r#"
        INSERT INTO owner_accounts (public_id, display_name, password_hash)
        VALUES ($1, $2, $3)
        "#,
        owner_public_id,
        display_name,
        password_hash
    )
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
