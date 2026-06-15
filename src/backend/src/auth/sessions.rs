//! DB-backed web sessions.
//!
//! C002/C003: raw session tokens are returned once and never persisted. Client
//! IP handling must eventually pass through trusted-proxy validation.

use sqlx::PgPool;
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
#[derive(Debug)]
pub enum SessionError {
    /// Token generation failed.
    TokenGeneration,
    /// Database failed.
    Database(sqlx::Error),
}

impl std::fmt::Display for SessionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TokenGeneration => formatter.write_str("session token generation failed"),
            Self::Database(_) => formatter.write_str("session database error"),
        }
    }
}

impl std::error::Error for SessionError {}

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

    sqlx::query(
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
    )
    .bind(session_id)
    .bind(input.owner_id)
    .bind(token_hash.as_bytes().as_slice())
    .bind(csrf_token_hash.as_bytes().as_slice())
    .bind(input.user_agent)
    .bind(input.device_name)
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
    let session = sqlx::query_as::<_, (Uuid, i16)>(
        r#"
        UPDATE sessions
        SET last_seen_at = now()
        WHERE token_hash = $1
          AND revoked_at IS NULL
          AND expires_at > now()
        RETURNING id, owner_id
        "#,
    )
    .bind(token_hash.as_bytes().as_slice())
    .fetch_optional(pool)
    .await
    .map_err(SessionError::Database)?;

    Ok(session.map(|(session_id, owner_id)| AuthenticatedSession {
        session_id,
        owner_id,
    }))
}

/// Revokes a session. Idempotent for nonexistent/already-revoked sessions.
pub async fn revoke_session(pool: &PgPool, session_id: Uuid) -> Result<(), SessionError> {
    sqlx::query(
        r#"
        UPDATE sessions
        SET revoked_at = COALESCE(revoked_at, now()),
            revocation_reason = COALESCE(revocation_reason, 'logout')
        WHERE id = $1
        "#,
    )
    .bind(session_id)
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
    let rows = sqlx::query_as::<
        _,
        (
            Uuid,
            Option<String>,
            Option<String>,
            OffsetDateTime,
            Option<OffsetDateTime>,
            OffsetDateTime,
        ),
    >(
        r#"
        SELECT id, device_name, user_agent, created_at, last_seen_at, expires_at
        FROM sessions
        WHERE owner_id = $1
          AND revoked_at IS NULL
          AND expires_at > now()
        ORDER BY COALESCE(last_seen_at, created_at) DESC
        "#,
    )
    .bind(owner_id)
    .fetch_all(pool)
    .await
    .map_err(SessionError::Database)?;

    Ok(rows
        .into_iter()
        .map(
            |(session_id, device_name, user_agent, created_at, last_seen_at, expires_at)| {
                SessionInfo {
                    session_id,
                    device_name,
                    user_agent,
                    created_at,
                    last_seen_at,
                    expires_at,
                    is_current: session_id == current_session_id,
                }
            },
        )
        .collect())
}

/// Verifies a CSRF token against the digest bound to an active session.
pub async fn verify_session_csrf(
    pool: &PgPool,
    session_id: Uuid,
    raw_csrf_token: &str,
) -> Result<bool, SessionError> {
    let csrf_hash = TokenHash::from_raw(raw_csrf_token);
    sqlx::query_scalar::<_, bool>(
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
    )
    .bind(session_id)
    .bind(csrf_hash.as_bytes().as_slice())
    .fetch_one(pool)
    .await
    .map_err(SessionError::Database)
}
