//! DB-backed rate limits for sensitive owner flows.
//!
//! Keys are stored as keyed BLAKE3 hashes. Raw IPs, passwords, and tokens never
//! enter `rate_limit_buckets`.

use sqlx::PgPool;
use thiserror::Error;
use time::{Duration, OffsetDateTime};

use crate::config::RateLimitSecret;

/// Rate-limit persistence failure.
#[derive(Debug, Error)]
pub enum RateLimitError {
    /// Database operation failed.
    #[error("rate limit database error: {0}")]
    Database(#[from] sqlx::Error),
}

/// Returns whether a sensitive action is currently blocked for this key.
pub async fn is_blocked(
    pool: &PgPool,
    secret: &RateLimitSecret,
    action: &str,
    key: &str,
    now: OffsetDateTime,
) -> Result<bool, RateLimitError> {
    let key_hash = key_hash(secret, action, key);
    let blocked = sqlx::query_scalar!(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM rate_limit_buckets
            WHERE action = $1
              AND key_hash = $2
              AND blocked_until > $3
        )
        "#,
        action,
        key_hash.as_slice(),
        now
    )
    .fetch_one(pool)
    .await?;

    Ok(blocked.unwrap_or(false))
}

/// Records one failed sensitive action and returns whether it is now blocked.
pub async fn record_failure(
    pool: &PgPool,
    secret: &RateLimitSecret,
    input: FailureInput<'_>,
) -> Result<bool, RateLimitError> {
    record_counted_attempt(
        pool,
        secret,
        CountedAttempt {
            action: input.action,
            key: input.key,
            now: input.now,
            max_attempts: input.max_attempts,
            window: input.window,
            block_for: input.block_for,
        },
    )
    .await
}

/// Records one quota-counted action attempt and returns whether it is now blocked.
pub async fn record_quota_attempt(
    pool: &PgPool,
    secret: &RateLimitSecret,
    input: QuotaInput<'_>,
) -> Result<bool, RateLimitError> {
    record_counted_attempt(
        pool,
        secret,
        CountedAttempt {
            action: input.action,
            key: input.key,
            now: input.now,
            max_attempts: input.max_attempts,
            window: input.window,
            block_for: input.block_for,
        },
    )
    .await
}

async fn record_counted_attempt(
    pool: &PgPool,
    secret: &RateLimitSecret,
    input: CountedAttempt<'_>,
) -> Result<bool, RateLimitError> {
    let key_hash = key_hash(secret, input.action, input.key);
    let window_expires_at = input.now + input.window;
    let block_until = input.now + input.block_for;
    let blocked_until = sqlx::query_scalar!(
        r#"
        WITH upserted AS (
            INSERT INTO rate_limit_buckets (
                action,
                key_hash,
                key_hash_alg,
                attempts,
                window_start_at,
                blocked_until,
                expires_at,
                updated_at
            )
            VALUES (
                $1,
                $2,
                'blake3-keyed',
                1,
                $3,
                CASE WHEN $4 <= 1 THEN $5::timestamptz ELSE NULL END,
                $6,
                $3
            )
            ON CONFLICT (action, key_hash)
            DO UPDATE SET
                attempts = CASE
                    WHEN rate_limit_buckets.expires_at <= $3 THEN 1
                    ELSE rate_limit_buckets.attempts + 1
                END,
                window_start_at = CASE
                    WHEN rate_limit_buckets.expires_at <= $3 THEN $3
                    ELSE rate_limit_buckets.window_start_at
                END,
                blocked_until = CASE
                    WHEN (
                        CASE
                            WHEN rate_limit_buckets.expires_at <= $3 THEN 1
                            ELSE rate_limit_buckets.attempts + 1
                        END
                    ) >= $4 THEN $5
                    ELSE rate_limit_buckets.blocked_until
                END,
                expires_at = CASE
                    WHEN rate_limit_buckets.expires_at <= $3 THEN $6
                    ELSE rate_limit_buckets.expires_at
                END,
                updated_at = $3
            RETURNING blocked_until
        )
        SELECT blocked_until FROM upserted
        "#,
        input.action,
        key_hash.as_slice(),
        input.now,
        input.max_attempts,
        block_until,
        window_expires_at
    )
    .fetch_one(pool)
    .await?;

    Ok(blocked_until.is_some_and(|until| until > input.now))
}

/// Clears a sensitive action bucket after successful authentication.
pub async fn clear(
    pool: &PgPool,
    secret: &RateLimitSecret,
    action: &str,
    key: &str,
) -> Result<(), RateLimitError> {
    let key_hash = key_hash(secret, action, key);
    sqlx::query!(
        r#"
        DELETE FROM rate_limit_buckets
        WHERE action = $1
          AND key_hash = $2
        "#,
        action,
        key_hash.as_slice()
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Failed action accounting input.
#[derive(Debug, Clone, Copy)]
pub struct FailureInput<'a> {
    /// Stable action name.
    pub action: &'a str,
    /// Raw request key; never stored directly.
    pub key: &'a str,
    /// Current wall-clock time from caller.
    pub now: OffsetDateTime,
    /// Max failed attempts per window.
    pub max_attempts: i32,
    /// Rolling window length.
    pub window: Duration,
    /// Block length once max attempts are reached.
    pub block_for: Duration,
}

/// Quota-counted action input.
#[derive(Debug, Clone, Copy)]
pub struct QuotaInput<'a> {
    /// Stable action name.
    pub action: &'a str,
    /// Raw request key; never stored directly.
    pub key: &'a str,
    /// Current wall-clock time from caller.
    pub now: OffsetDateTime,
    /// Max action attempts per window.
    pub max_attempts: i32,
    /// Rolling window length.
    pub window: Duration,
    /// Block length once max attempts are reached.
    pub block_for: Duration,
}

#[derive(Debug, Clone, Copy)]
struct CountedAttempt<'a> {
    action: &'a str,
    key: &'a str,
    now: OffsetDateTime,
    max_attempts: i32,
    window: Duration,
    block_for: Duration,
}

fn key_hash(secret: &RateLimitSecret, action: &str, key: &str) -> [u8; 32] {
    let mut input = Vec::with_capacity(action.len() + key.len() + 1);
    input.extend_from_slice(action.as_bytes());
    input.push(0);
    input.extend_from_slice(key.as_bytes());
    blake3::keyed_hash(secret.as_key(), &input).into()
}
