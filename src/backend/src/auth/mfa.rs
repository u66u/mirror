//! Optional owner TOTP and one-time recovery codes.
//!
//! Raw TOTP seeds and recovery codes are returned only at setup boundaries.
//! Persisted TOTP seeds are encrypted with `MIRROR_AUTH_SECRET`; recovery codes
//! are stored as SHA-256 digests and consumed once.

use chacha20poly1305::{
    AeadCore, ChaCha20Poly1305, KeyInit, Nonce,
    aead::{Aead, OsRng},
};
use hmac::{Hmac, Mac};
use sha1::Sha1;
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row};
use subtle::ConstantTimeEq;
use thiserror::Error;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::config::AuthSecret;

const TOTP_SECRET_BYTES: usize = 20;
const TOTP_DIGITS: u32 = 6;
const TOTP_PERIOD_SECONDS: i64 = 30;
const TOTP_WINDOW_STEPS: i64 = 1;
const RECOVERY_CODE_COUNT: usize = 10;
const RECOVERY_CODE_BYTES: usize = 12;
const TOTP_CIPHERTEXT_PREFIX: &[u8] = b"mrtotp1\0";

type HmacSha1 = Hmac<Sha1>;

/// Login or reauth second factor.
#[derive(Debug, Clone)]
pub struct SecondFactorInput {
    /// Six-digit TOTP code.
    pub totp_code: Option<String>,
    /// One-time recovery code.
    pub recovery_code: Option<String>,
}

/// Current MFA status for owner settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MfaStatus {
    /// Whether TOTP is active for login.
    pub totp_enabled: bool,
    /// Whether a pending secret exists but is not enabled yet.
    pub totp_setup_pending: bool,
    /// Active unused recovery codes.
    pub recovery_codes_remaining: i64,
}

/// Generated TOTP setup material.
#[derive(Clone)]
pub struct TotpSetupOutput {
    /// Base32 TOTP secret shown once to the owner.
    pub secret_base32: String,
    /// Standard authenticator-app provisioning URI.
    pub provisioning_uri: String,
}

/// Generated recovery codes shown once to the owner.
#[derive(Clone)]
pub struct RecoveryCodesOutput {
    /// Raw one-time recovery codes.
    pub recovery_codes: Vec<String>,
}

/// MFA operation failure.
#[derive(Debug, Error)]
pub enum MfaError {
    /// TOTP is required before a session/token can be created.
    #[error("second factor required")]
    SecondFactorRequired,
    /// Supplied TOTP/recovery code is malformed, wrong, missing, or already used.
    #[error("invalid second factor")]
    InvalidSecondFactor,
    /// TOTP is not enabled for operations that require it.
    #[error("totp not enabled")]
    TotpNotEnabled,
    /// TOTP setup is already enabled.
    #[error("totp already enabled")]
    TotpAlreadyEnabled,
    /// TOTP setup has not been initialized.
    #[error("totp setup missing")]
    TotpSetupMissing,
    /// Random generation failed.
    #[error("mfa random generation failed")]
    RandomFailed,
    /// Secret encryption or decryption failed.
    #[error("mfa secret crypto failed")]
    CryptoFailed,
    /// Database failed.
    #[error("mfa database error: {0}")]
    Database(#[from] sqlx::Error),
}

/// Loads current owner MFA state.
pub async fn mfa_status(pool: &PgPool) -> Result<MfaStatus, MfaError> {
    let row = sqlx::query(
        r#"
        SELECT
            totp_secret_ciphertext IS NOT NULL
                AND totp_enabled_at IS NULL AS setup_pending,
            totp_enabled_at IS NOT NULL AS enabled,
            (
                SELECT count(*)
                FROM owner_recovery_codes
                WHERE owner_id = owner_accounts.id
                  AND used_at IS NULL
            ) AS recovery_count
        FROM owner_accounts
        WHERE id = 1
          AND disabled_at IS NULL
        "#,
    )
    .fetch_optional(pool)
    .await?;

    let Some(row) = row else {
        return Ok(MfaStatus {
            totp_enabled: false,
            totp_setup_pending: false,
            recovery_codes_remaining: 0,
        });
    };

    Ok(MfaStatus {
        totp_enabled: row.get::<bool, _>("enabled"),
        totp_setup_pending: row.get::<bool, _>("setup_pending"),
        recovery_codes_remaining: row.get::<i64, _>("recovery_count"),
    })
}

/// Returns whether TOTP is active for owner login.
pub async fn second_factor_enabled(pool: &PgPool) -> Result<bool, MfaError> {
    let enabled = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM owner_accounts
            WHERE id = 1
              AND disabled_at IS NULL
              AND totp_enabled_at IS NOT NULL
        )
        "#,
    )
    .fetch_one(pool)
    .await?;
    Ok(enabled)
}

/// Creates or replaces a pending TOTP setup secret.
pub async fn begin_totp_setup(
    pool: &PgPool,
    auth_secret: &AuthSecret,
) -> Result<TotpSetupOutput, MfaError> {
    let mut tx = pool.begin().await?;
    let enabled = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT totp_enabled_at IS NOT NULL
        FROM owner_accounts
        WHERE id = 1
          AND disabled_at IS NULL
        FOR UPDATE
        "#,
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(MfaError::TotpSetupMissing)?;
    if enabled {
        return Err(MfaError::TotpAlreadyEnabled);
    }

    let secret = random_bytes::<TOTP_SECRET_BYTES>()?;
    let encrypted = encrypt_totp_secret(auth_secret, &secret)?;
    sqlx::query(
        r#"
        UPDATE owner_accounts
        SET totp_secret_ciphertext = $1,
            totp_enabled_at = NULL,
            updated_at = now()
        WHERE id = 1
          AND disabled_at IS NULL
        "#,
    )
    .bind(encrypted)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    let secret_base32 = base32_encode(&secret);
    Ok(TotpSetupOutput {
        provisioning_uri: provisioning_uri(&secret_base32),
        secret_base32,
    })
}

/// Verifies the pending TOTP secret, enables it, and returns fresh recovery codes.
pub async fn enable_totp(
    pool: &PgPool,
    auth_secret: &AuthSecret,
    code: &str,
) -> Result<RecoveryCodesOutput, MfaError> {
    let mut tx = pool.begin().await?;
    let row = sqlx::query(
        r#"
        SELECT
            totp_secret_ciphertext,
            totp_enabled_at IS NOT NULL AS enabled
        FROM owner_accounts
        WHERE id = 1
          AND disabled_at IS NULL
        FOR UPDATE
        "#,
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(MfaError::TotpSetupMissing)?;
    if row.get::<bool, _>("enabled") {
        return Err(MfaError::TotpAlreadyEnabled);
    }
    let encrypted = row
        .get::<Option<Vec<u8>>, _>("totp_secret_ciphertext")
        .ok_or(MfaError::TotpSetupMissing)?;
    let secret = decrypt_totp_secret(auth_secret, &encrypted)?;
    if !verify_totp_code(&secret, code, OffsetDateTime::now_utc()) {
        return Err(MfaError::InvalidSecondFactor);
    }

    let version = sqlx::query_scalar::<_, i32>(
        r#"
        UPDATE owner_accounts
        SET totp_enabled_at = now(),
            recovery_codes_version = recovery_codes_version + 1,
            updated_at = now()
        WHERE id = 1
          AND disabled_at IS NULL
        RETURNING recovery_codes_version
        "#,
    )
    .fetch_one(&mut *tx)
    .await?;
    sqlx::query("DELETE FROM owner_recovery_codes WHERE owner_id = 1")
        .execute(&mut *tx)
        .await?;
    let output = insert_recovery_codes(&mut tx, version).await?;
    tx.commit().await?;
    Ok(output)
}

/// Disables TOTP and deletes active recovery codes.
pub async fn disable_totp(
    pool: &PgPool,
    auth_secret: &AuthSecret,
    second_factor: SecondFactorInput,
) -> Result<(), MfaError> {
    if !second_factor_enabled(pool).await? {
        return Err(MfaError::TotpNotEnabled);
    }
    verify_second_factor(pool, auth_secret, second_factor).await?;
    let mut tx = pool.begin().await?;
    let enabled = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT totp_enabled_at IS NOT NULL
        FROM owner_accounts
        WHERE id = 1
          AND disabled_at IS NULL
        FOR UPDATE
        "#,
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(MfaError::TotpNotEnabled)?;
    if !enabled {
        return Err(MfaError::TotpNotEnabled);
    }
    sqlx::query(
        r#"
        UPDATE owner_accounts
        SET totp_secret_ciphertext = NULL,
            totp_enabled_at = NULL,
            recovery_codes_version = recovery_codes_version + 1,
            updated_at = now()
        WHERE id = 1
          AND disabled_at IS NULL
        "#,
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query("DELETE FROM owner_recovery_codes WHERE owner_id = 1")
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

/// Rotates recovery codes after valid second-factor proof.
pub async fn rotate_recovery_codes(
    pool: &PgPool,
    auth_secret: &AuthSecret,
    second_factor: SecondFactorInput,
) -> Result<RecoveryCodesOutput, MfaError> {
    verify_second_factor(pool, auth_secret, second_factor).await?;
    let mut tx = pool.begin().await?;
    let enabled = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT totp_enabled_at IS NOT NULL
        FROM owner_accounts
        WHERE id = 1
          AND disabled_at IS NULL
        FOR UPDATE
        "#,
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(MfaError::TotpNotEnabled)?;
    if !enabled {
        return Err(MfaError::TotpNotEnabled);
    }
    let version = sqlx::query_scalar::<_, i32>(
        r#"
        UPDATE owner_accounts
        SET recovery_codes_version = recovery_codes_version + 1,
            updated_at = now()
        WHERE id = 1
          AND disabled_at IS NULL
        RETURNING recovery_codes_version
        "#,
    )
    .fetch_one(&mut *tx)
    .await?;
    sqlx::query("DELETE FROM owner_recovery_codes WHERE owner_id = 1")
        .execute(&mut *tx)
        .await?;
    let output = insert_recovery_codes(&mut tx, version).await?;
    tx.commit().await?;
    Ok(output)
}

/// Verifies a TOTP or consumes one valid recovery code.
pub async fn verify_second_factor(
    pool: &PgPool,
    auth_secret: &AuthSecret,
    input: SecondFactorInput,
) -> Result<(), MfaError> {
    if !second_factor_enabled(pool).await? {
        return Ok(());
    }

    if let Some(code) = input.totp_code.as_deref()
        && !code.trim().is_empty()
    {
        let encrypted = load_totp_secret_ciphertext(pool)
            .await?
            .ok_or(MfaError::TotpNotEnabled)?;
        let secret = decrypt_totp_secret(auth_secret, &encrypted)?;
        if verify_totp_code(&secret, code, OffsetDateTime::now_utc()) {
            return Ok(());
        }
        return Err(MfaError::InvalidSecondFactor);
    }

    if let Some(code) = input.recovery_code.as_deref()
        && !code.trim().is_empty()
    {
        return consume_recovery_code(pool, code).await;
    }

    Err(MfaError::SecondFactorRequired)
}

async fn load_totp_secret_ciphertext(pool: &PgPool) -> Result<Option<Vec<u8>>, MfaError> {
    sqlx::query_scalar::<_, Vec<u8>>(
        r#"
        SELECT totp_secret_ciphertext
        FROM owner_accounts
        WHERE id = 1
          AND disabled_at IS NULL
          AND totp_secret_ciphertext IS NOT NULL
        "#,
    )
    .fetch_optional(pool)
    .await
    .map_err(MfaError::Database)
}

async fn consume_recovery_code(pool: &PgPool, code: &str) -> Result<(), MfaError> {
    let Some(normalized) = normalize_recovery_code(code) else {
        return Err(MfaError::InvalidSecondFactor);
    };
    let code_hash = recovery_code_hash(&normalized);
    let consumed = sqlx::query_scalar::<_, Uuid>(
        r#"
        UPDATE owner_recovery_codes
        SET used_at = now()
        WHERE owner_id = 1
          AND code_hash = $1
          AND used_at IS NULL
        RETURNING id
        "#,
    )
    .bind(code_hash.as_slice())
    .fetch_optional(pool)
    .await?;
    consumed.map(|_| ()).ok_or(MfaError::InvalidSecondFactor)
}

async fn insert_recovery_codes(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    version: i32,
) -> Result<RecoveryCodesOutput, MfaError> {
    let mut codes = Vec::with_capacity(RECOVERY_CODE_COUNT);
    for _ in 0..RECOVERY_CODE_COUNT {
        let bytes = random_bytes::<RECOVERY_CODE_BYTES>()?;
        let code = format_recovery_code(&base32_encode(&bytes));
        let normalized = normalize_recovery_code(&code).ok_or(MfaError::RandomFailed)?;
        let code_hash = recovery_code_hash(&normalized);
        sqlx::query(
            r#"
            INSERT INTO owner_recovery_codes (
                id,
                owner_id,
                code_hash,
                code_hash_alg,
                version
            )
            VALUES ($1, 1, $2, 'sha256', $3)
            "#,
        )
        .bind(Uuid::now_v7())
        .bind(code_hash.as_slice())
        .bind(version)
        .execute(&mut **tx)
        .await?;
        codes.push(code);
    }
    Ok(RecoveryCodesOutput {
        recovery_codes: codes,
    })
}

fn encrypt_totp_secret(secret: &AuthSecret, plaintext: &[u8]) -> Result<Vec<u8>, MfaError> {
    let cipher = ChaCha20Poly1305::new(secret.as_key().into());
    let nonce = ChaCha20Poly1305::generate_nonce(&mut OsRng);
    let ciphertext = cipher
        .encrypt(&nonce, plaintext)
        .map_err(|_| MfaError::CryptoFailed)?;
    let mut output =
        Vec::with_capacity(TOTP_CIPHERTEXT_PREFIX.len() + nonce.len() + ciphertext.len());
    output.extend_from_slice(TOTP_CIPHERTEXT_PREFIX);
    output.extend_from_slice(&nonce);
    output.extend_from_slice(&ciphertext);
    Ok(output)
}

fn decrypt_totp_secret(secret: &AuthSecret, ciphertext: &[u8]) -> Result<Vec<u8>, MfaError> {
    if ciphertext.len() <= TOTP_CIPHERTEXT_PREFIX.len() + 12
        || !bool::from(ciphertext[..TOTP_CIPHERTEXT_PREFIX.len()].ct_eq(TOTP_CIPHERTEXT_PREFIX))
    {
        return Err(MfaError::CryptoFailed);
    }
    let nonce_start = TOTP_CIPHERTEXT_PREFIX.len();
    let nonce_end = nonce_start + 12;
    let nonce = Nonce::from_slice(&ciphertext[nonce_start..nonce_end]);
    let cipher = ChaCha20Poly1305::new(secret.as_key().into());
    cipher
        .decrypt(nonce, &ciphertext[nonce_end..])
        .map_err(|_| MfaError::CryptoFailed)
}

fn verify_totp_code(secret: &[u8], code: &str, now: OffsetDateTime) -> bool {
    let Some(candidate) = normalize_totp_code(code) else {
        return false;
    };
    let current_step = now.unix_timestamp() / TOTP_PERIOD_SECONDS;
    for offset in -TOTP_WINDOW_STEPS..=TOTP_WINDOW_STEPS {
        let counter = current_step + offset;
        if counter < 0 {
            continue;
        }
        let expected = totp_code(secret, counter as u64);
        if bool::from(candidate.as_bytes().ct_eq(expected.as_bytes())) {
            return true;
        }
    }
    false
}

fn totp_code(secret: &[u8], counter: u64) -> String {
    let Ok(mut mac) = <HmacSha1 as Mac>::new_from_slice(secret) else {
        return String::new();
    };
    mac.update(&counter.to_be_bytes());
    let digest = mac.finalize().into_bytes();
    let offset = usize::from(digest[19] & 0x0f);
    let binary = (u32::from(digest[offset] & 0x7f) << 24)
        | (u32::from(digest[offset + 1]) << 16)
        | (u32::from(digest[offset + 2]) << 8)
        | u32::from(digest[offset + 3]);
    let value = binary % 10_u32.pow(TOTP_DIGITS);
    format!("{value:06}")
}

fn normalize_totp_code(code: &str) -> Option<String> {
    let trimmed = code.trim();
    if trimmed.len() == 6 && trimmed.bytes().all(|value| value.is_ascii_digit()) {
        Some(trimmed.to_owned())
    } else {
        None
    }
}

fn normalize_recovery_code(code: &str) -> Option<String> {
    let normalized = code
        .chars()
        .filter(|value| !value.is_whitespace() && *value != '-')
        .map(|value| value.to_ascii_uppercase())
        .collect::<String>();
    if normalized.len() == 20
        && normalized
            .bytes()
            .all(|value| value.is_ascii_alphanumeric())
    {
        Some(normalized)
    } else {
        None
    }
}

fn recovery_code_hash(code: &str) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"mirror-recovery-code-v1");
    hasher.update([0]);
    hasher.update(code.as_bytes());
    hasher.finalize().into()
}

fn format_recovery_code(encoded: &str) -> String {
    encoded
        .chars()
        .take(20)
        .collect::<Vec<_>>()
        .chunks(5)
        .map(|chunk| chunk.iter().collect::<String>())
        .collect::<Vec<_>>()
        .join("-")
}

fn random_bytes<const N: usize>() -> Result<[u8; N], MfaError> {
    let mut bytes = [0_u8; N];
    getrandom::getrandom(&mut bytes).map_err(|_| MfaError::RandomFailed)?;
    Ok(bytes)
}

fn provisioning_uri(secret_base32: &str) -> String {
    let issuer = "Mirror";
    let account = "owner";
    let label = format!(
        "{}:{}",
        url::form_urlencoded::byte_serialize(issuer.as_bytes()).collect::<String>(),
        url::form_urlencoded::byte_serialize(account.as_bytes()).collect::<String>()
    );
    format!(
        "otpauth://totp/{label}?secret={secret_base32}&issuer={issuer}&algorithm=SHA1&digits=6&period={TOTP_PERIOD_SECONDS}"
    )
}

fn base32_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    let mut output = String::with_capacity((bytes.len() * 8).div_ceil(5));
    let mut buffer = 0_u16;
    let mut bits = 0_u8;
    for byte in bytes {
        buffer = (buffer << 8) | u16::from(*byte);
        bits += 8;
        while bits >= 5 {
            let index = ((buffer >> (bits - 5)) & 0b1_1111) as usize;
            output.push(char::from(ALPHABET[index]));
            bits -= 5;
        }
    }
    if bits > 0 {
        let index = ((buffer << (5 - bits)) & 0b1_1111) as usize;
        output.push(char::from(ALPHABET[index]));
    }
    output
}
