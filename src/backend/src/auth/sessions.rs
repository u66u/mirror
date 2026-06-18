//! DB-backed web sessions.
//!
//! C002/C003: raw session tokens are returned once and never persisted. Client
//! IP handling must eventually pass through trusted-proxy validation.

use sqlx::PgPool;
use thiserror::Error;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::auth::{OpaqueToken, TokenHash};

/// Session creation input after authentication succeeds.
#[derive(Debug, Clone)]
pub struct SessionCreateInput {
    /// Single owner account ID.
    pub owner_id: i16,
    /// Browser user-agent metadata for session inventory.
    pub user_agent: Option<String>,
    /// Human-friendly device/browser label.
    pub device_name: Option<String>,
}

/// New session result. The raw token must be sent to the client once.
#[derive(Debug, Clone)]
pub struct SessionCreateOutput {
    /// Persisted session ID.
    pub session_id: Uuid,
    /// Raw opaque session token.
    pub token: OpaqueToken,
    /// Raw CSRF token bound to this session.
    pub csrf_token: OpaqueToken,
}

/// Authenticated session lookup result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuthenticatedSession {
    /// Persisted session ID.
    pub session_id: Uuid,
    /// Owning account ID.
    pub owner_id: i16,
}

/// Active session inventory row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionInfo {
    /// Persisted session ID.
    pub session_id: Uuid,
    /// Optional browser/device label.
    pub device_name: Option<String>,
    /// User-agent metadata.
    pub user_agent: Option<String>,
    /// Creation timestamp.
    pub created_at: OffsetDateTime,
    /// Last successful authentication timestamp.
    pub last_seen_at: Option<OffsetDateTime>,
    /// Expiration timestamp.
    pub expires_at: OffsetDateTime,
    /// Whether this row is the caller's session.
    pub is_current: bool,
}

/// Session operation failure.
#[derive(Debug, Error)]
pub enum SessionError {
    /// Token generation failed.
    #[error("session token generation failed")]
    TokenGeneration,
    /// Database failed.
    #[error("session database error: {0}")]
    Database(#[from] sqlx::Error),
}

/// Creates a 30-day web session and stores only its token hash.
pub async fn create_session(
    pool: &PgPool,
    input: SessionCreateInput,
) -> Result<SessionCreateOutput, SessionError> {
    let token = OpaqueToken::generate().map_err(|_| SessionError::TokenGeneration)?;
    let csrf_token = OpaqueToken::generate().map_err(|_| SessionError::TokenGeneration)?;
    let token_hash = token.hash();
    let csrf_token_hash = csrf_token.hash();
    let session_id = Uuid::now_v7();

    sqlx::query!(
        r#"
        INSERT INTO sessions (
            id,
            owner_id,
            token_hash,
            csrf_token_hash,
            expires_at,
            user_agent,
            device_name
        )
        VALUES ($1, $2, $3, $4, now() + interval '30 days', $5, $6)
        "#,
        session_id,
        input.owner_id,
        token_hash.as_bytes().as_slice(),
        csrf_token_hash.as_bytes().as_slice(),
        input.user_agent,
        input.device_name
    )
    .execute(pool)
    .await
    .map_err(SessionError::Database)?;

    Ok(SessionCreateOutput {
        session_id,
        token,
        csrf_token,
    })
}

/// Authenticates a raw session token by digest lookup.
pub async fn authenticate_session(
    pool: &PgPool,
    raw_token: &str,
) -> Result<Option<AuthenticatedSession>, SessionError> {
    let token_hash = TokenHash::from_raw(raw_token);
    let session = sqlx::query!(
        r#"
        UPDATE sessions
        SET last_seen_at = now()
        WHERE token_hash = $1
          AND revoked_at IS NULL
          AND expires_at > now()
        RETURNING id, owner_id
        "#,
        token_hash.as_bytes().as_slice()
    )
    .fetch_optional(pool)
    .await?;

    Ok(session.map(|row| AuthenticatedSession {
        session_id: row.id,
        owner_id: row.owner_id,
    }))
}

/// Revokes a session. Idempotent for nonexistent/already-revoked sessions.
pub async fn revoke_session(pool: &PgPool, session_id: Uuid) -> Result<(), SessionError> {
    sqlx::query!(
        r#"
        UPDATE sessions
        SET revoked_at = COALESCE(revoked_at, now()),
            revocation_reason = COALESCE(revocation_reason, 'logout')
        WHERE id = $1
        "#,
        session_id
    )
    .execute(pool)
    .await
    .map_err(SessionError::Database)?;

    Ok(())
}

/// Lists active sessions for session inventory/revocation UI.
pub async fn list_sessions(
    pool: &PgPool,
    owner_id: i16,
    current_session_id: Uuid,
) -> Result<Vec<SessionInfo>, SessionError> {
    let rows = sqlx::query!(
        r#"
        SELECT id, device_name, user_agent, created_at, last_seen_at, expires_at
        FROM sessions
        WHERE owner_id = $1
          AND revoked_at IS NULL
          AND expires_at > now()
        ORDER BY COALESCE(last_seen_at, created_at) DESC
        "#,
        owner_id
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| SessionInfo {
            session_id: row.id,
            device_name: row.device_name,
            user_agent: row.user_agent,
            created_at: row.created_at,
            last_seen_at: row.last_seen_at,
            expires_at: row.expires_at,
            is_current: row.id == current_session_id,
        })
        .collect())
}

/// Verifies a CSRF token against the digest bound to an active session.
pub async fn verify_session_csrf(
    pool: &PgPool,
    session_id: Uuid,
    raw_csrf_token: &str,
) -> Result<bool, SessionError> {
    let csrf_hash = TokenHash::from_raw(raw_csrf_token);
    let valid = sqlx::query_scalar!(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM sessions
            WHERE id = $1
              AND csrf_token_hash = $2
              AND revoked_at IS NULL
              AND expires_at > now()
        )
        "#,
        session_id,
        csrf_hash.as_bytes().as_slice()
    )
    .fetch_one(pool)
    .await?;

    Ok(valid.unwrap_or(false))
}
