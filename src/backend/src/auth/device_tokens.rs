//! Revocable Android device tokens.
//!
//! Raw device tokens are returned once at creation. Persist and query by
//! `TokenHash` only.

use sqlx::PgPool;
use thiserror::Error;
use uuid::Uuid;

use crate::auth::{OpaqueToken, TokenHash};

/// Device-token creation input after recent reauthentication.
#[derive(Debug, Clone)]
pub struct DeviceTokenCreateInput {
    /// Single owner account ID.
    pub owner_id: i16,
    /// Human-friendly device name.
    pub name: String,
    /// Session that created the token.
    pub created_by_session_id: Option<Uuid>,
    /// User-agent metadata.
    pub user_agent: Option<String>,
}

/// New device token returned once.
#[derive(Debug, Clone)]
pub struct DeviceTokenCreateOutput {
    /// Persisted device token ID.
    pub device_token_id: Uuid,
    /// Raw opaque token for Android encrypted storage.
    pub token: OpaqueToken,
}

/// Authenticated Android device token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuthenticatedDeviceToken {
    /// Persisted device token ID.
    pub device_token_id: Uuid,
    /// Owning account ID.
    pub owner_id: i16,
}

/// Device-token operation failure.
#[derive(Debug, Error)]
pub enum DeviceTokenError {
    /// Token generation failed.
    #[error("device token generation failed")]
    TokenGeneration,
    /// Device name violates policy.
    #[error("invalid device token name")]
    InvalidName,
    /// Database failed.
    #[error("device token database error: {0}")]
    Database(#[from] sqlx::Error),
}

/// Creates a revocable Android device token.
pub async fn create_device_token(
    pool: &PgPool,
    input: DeviceTokenCreateInput,
) -> Result<DeviceTokenCreateOutput, DeviceTokenError> {
    let name = normalize_name(&input.name)?;
    let token = OpaqueToken::generate().map_err(|_| DeviceTokenError::TokenGeneration)?;
    let token_hash = token.hash();
    let device_token_id = Uuid::now_v7();

    sqlx::query!(
        r#"
        INSERT INTO device_tokens (
            id,
            owner_id,
            token_hash,
            name,
            created_by_session_id,
            user_agent
        )
        VALUES ($1, $2, $3, $4, $5, $6)
        "#,
        device_token_id,
        input.owner_id,
        token_hash.as_bytes().as_slice(),
        name,
        input.created_by_session_id,
        input.user_agent
    )
    .execute(pool)
    .await
    .map_err(DeviceTokenError::Database)?;

    Ok(DeviceTokenCreateOutput {
        device_token_id,
        token,
    })
}

/// Authenticates an Android device token by digest lookup.
pub async fn authenticate_device_token(
    pool: &PgPool,
    raw_token: &str,
) -> Result<Option<AuthenticatedDeviceToken>, DeviceTokenError> {
    let token_hash = TokenHash::from_raw(raw_token);
    let token_hash_bytes = token_hash.as_bytes().as_slice();
    let token = sqlx::query_as!(
        crate::auth::device_tokens::AuthenticatedDeviceToken,
        r#"
        UPDATE device_tokens
        SET last_used_at = now()
        WHERE token_hash = $1
          AND revoked_at IS NULL
          AND (expires_at IS NULL OR expires_at > now())
        RETURNING id as "device_token_id!", owner_id as "owner_id!"
        "#,
        token_hash_bytes
    )
    .fetch_optional(pool)
    .await
    .map_err(DeviceTokenError::Database)?;

    Ok(token)
}

/// Revokes an Android device token. Idempotent for missing/already-revoked IDs.
pub async fn revoke_device_token(
    pool: &PgPool,
    device_token_id: Uuid,
) -> Result<(), DeviceTokenError> {
    sqlx::query!(
        r#"
        UPDATE device_tokens
        SET revoked_at = COALESCE(revoked_at, now()),
            revocation_reason = COALESCE(revocation_reason, 'revoked')
        WHERE id = $1
        "#,
        device_token_id
    )
    .execute(pool)
    .await
    .map_err(DeviceTokenError::Database)?;

    Ok(())
}

/// Revokes one device token owned by the authenticated owner.
///
/// Returns false when the ID is missing or belongs to another owner.
pub async fn revoke_owner_device_token(
    pool: &PgPool,
    owner_id: i16,
    device_token_id: Uuid,
) -> Result<bool, DeviceTokenError> {
    let result = sqlx::query!(
        r#"
        UPDATE device_tokens
        SET revoked_at = COALESCE(revoked_at, now()),
            revocation_reason = COALESCE(revocation_reason, 'revoked')
        WHERE id = $1
          AND owner_id = $2
        "#,
        device_token_id,
        owner_id
    )
    .execute(pool)
    .await
    .map_err(DeviceTokenError::Database)?;

    Ok(result.rows_affected() == 1)
}

fn normalize_name(value: &str) -> Result<String, DeviceTokenError> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.chars().count() > 120 {
        return Err(DeviceTokenError::InvalidName);
    }
    Ok(trimmed.to_owned())
}
