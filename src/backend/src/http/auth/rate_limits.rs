use actix_web::HttpRequest;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    config::RateLimitQuota,
    http::{client_ip, error::ApiError},
    rate_limit::{self, FailureInput},
    state::AppState,
};

const OWNER_PASSWORD_LOGIN_ACTION: &str = "owner_password_login";
const OWNER_PASSWORD_REAUTH_ACTION: &str = "owner_password_reauth";
const OWNER_MFA_ACTION: &str = "owner_mfa";

#[derive(Clone, Copy)]
pub(super) enum OwnerRateLimit {
    PasswordLogin,
    PasswordReauth,
    Mfa,
}

impl OwnerRateLimit {
    fn action(self) -> &'static str {
        match self {
            Self::PasswordLogin => OWNER_PASSWORD_LOGIN_ACTION,
            Self::PasswordReauth => OWNER_PASSWORD_REAUTH_ACTION,
            Self::Mfa => OWNER_MFA_ACTION,
        }
    }

    fn quota(self, state: &AppState) -> RateLimitQuota {
        match self {
            Self::PasswordLogin | Self::PasswordReauth => {
                state.config.rate_limits.owner_password_login
            }
            Self::Mfa => state.config.rate_limits.owner_mfa,
        }
    }

    fn blocked_message(self) -> &'static str {
        match self {
            Self::PasswordLogin => "too many failed login attempts",
            Self::PasswordReauth => "too many password reauthentication attempts",
            Self::Mfa => "too many second-factor attempts",
        }
    }
}

pub(super) async fn reject_blocked(
    state: &AppState,
    pool: &sqlx::PgPool,
    key: &str,
    limit: OwnerRateLimit,
) -> Result<(), ApiError> {
    if rate_limit::is_blocked(
        pool,
        &state.config.rate_limit_secret,
        limit.action(),
        key,
        OffsetDateTime::now_utc(),
    )
    .await
    .map_err(|_| ApiError::Internal)?
    {
        Err(rate_limit_error(limit))
    } else {
        Ok(())
    }
}

pub(super) async fn record_owner_failure(
    state: &AppState,
    pool: &sqlx::PgPool,
    key: &str,
    limit: OwnerRateLimit,
) -> Result<(), ApiError> {
    let quota = limit.quota(state);
    let blocked = rate_limit::record_failure(
        pool,
        &state.config.rate_limit_secret,
        FailureInput {
            action: limit.action(),
            key,
            now: OffsetDateTime::now_utc(),
            max_attempts: quota.max_per_window,
            window: quota.window,
            block_for: quota.block_for,
        },
    )
    .await
    .map_err(|_| ApiError::Internal)?;
    if blocked {
        Err(rate_limit_error(limit))
    } else {
        Ok(())
    }
}

pub(super) async fn clear_limit(
    state: &AppState,
    pool: &sqlx::PgPool,
    key: &str,
    limit: OwnerRateLimit,
) -> Result<(), ApiError> {
    rate_limit::clear(pool, &state.config.rate_limit_secret, limit.action(), key)
        .await
        .map_err(|_| ApiError::Internal)
}

pub(super) fn owner_password_login_key(req: &HttpRequest, state: &AppState) -> String {
    client_ip::client_ip(req, &state.config.trusted_proxies)
        .map(|ip| ip.to_string())
        .unwrap_or_else(|| "unknown-peer".to_owned())
}

pub(super) fn owner_password_reauth_key(
    req: &HttpRequest,
    state: &AppState,
    session_id: Uuid,
) -> String {
    let ip = client_ip::client_ip(req, &state.config.trusted_proxies)
        .map(|ip| ip.to_string())
        .unwrap_or_else(|| "unknown-peer".to_owned());
    format!("session:{session_id}:ip:{ip}")
}

fn rate_limit_error(limit: OwnerRateLimit) -> ApiError {
    ApiError::TooManyRequests("rate_limited", limit.blocked_message())
}
