//! First-party resumable upload protocol.
//!
//! C001: completion verifies staged bytes but does not create durable asset rows
//! or promote originals. T204 owns promotion and DB asset state.

use blake3::Hasher;
use serde::Serialize;
use sqlx::PgPool;
use uuid::Uuid;

use crate::storage::{ObjectStorage, StorageKey};

/// Fixed byte length for every non-final upload part.
pub const UPLOAD_PART_SIZE_BYTES: usize = 4 * 1024 * 1024;
const UPLOAD_PART_SIZE_I64: i64 = 4 * 1024 * 1024;

/// Upload session status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UploadStatus {
    /// Parts can still be uploaded.
    Open,
    /// Bytes verified. Promotion is handled by T204.
    Verified,
    /// Upload was cancelled.
    Cancelled,
}

impl UploadStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Verified => "verified",
            Self::Cancelled => "cancelled",
        }
    }
}

/// Create upload input after HTTP validation.
#[derive(Debug)]
pub struct CreateUploadInput {
    /// Owner account ID.
    pub owner_id: i16,
    /// Original client filename for owner display only.
    pub original_filename: String,
    /// Expected total byte length.
    pub expected_size: i64,
    /// Expected BLAKE3 hex digest.
    pub expected_blake3: String,
    /// Declared supported media type.
    pub media_type: String,
    /// Stable client key making upload creation safe to retry.
    pub client_upload_key: Option<Uuid>,
}

/// Upload session view.
#[derive(Debug, Serialize)]
pub struct UploadSessionView {
    /// Upload session ID.
    pub upload_id: Uuid,
    /// Current session state.
    pub status: UploadStatus,
    /// Expected total byte length.
    pub expected_size: i64,
    /// Expected BLAKE3 hex digest.
    pub expected_blake3: String,
    /// Declared supported media type.
    pub media_type: String,
    /// Committed part indexes for resume.
    pub committed_parts: Vec<i32>,
}

/// Upload operation failure.
#[derive(Debug)]
pub enum UploadError {
    /// Input violates upload policy.
    InvalidInput,
    /// Part index is outside the range derived from expected upload size.
    PartOutOfRange,
    /// Part byte length does not match its deterministic frame.
    PartLengthMismatch,
    /// Upload session was not found for owner.
    NotFound,
    /// Upload is not open for mutation.
    NotOpen,
    /// Completed bytes do not match expected size/hash/media signature.
    VerificationFailed,
    /// Storage backend failed.
    Storage(crate::storage::StorageError),
    /// Database failed.
    Database(sqlx::Error),
}

impl std::fmt::Display for UploadError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::InvalidInput => "invalid upload input",
            Self::PartOutOfRange => "upload part index is out of range",
            Self::PartLengthMismatch => "upload part has the wrong length",
            Self::NotFound => "upload not found",
            Self::NotOpen => "upload is not open",
            Self::VerificationFailed => "upload verification failed",
            Self::Storage(_) => "upload storage error",
            Self::Database(_) => "upload database error",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for UploadError {}

/// Creates a resumable upload session.
pub async fn create_upload(
    pool: &PgPool,
    input: CreateUploadInput,
) -> Result<UploadSessionView, UploadError> {
    validate_create_input(&input)?;
    let proposed_upload_id = Uuid::now_v7();
    let inserted_upload_id = sqlx::query_scalar::<_, Uuid>(
        r#"
        INSERT INTO upload_sessions (
            id,
            owner_id,
            original_filename,
            expected_size,
            expected_blake3,
            media_type,
            client_upload_key
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        ON CONFLICT (owner_id, client_upload_key)
        WHERE client_upload_key IS NOT NULL
        DO NOTHING
        RETURNING id
        "#,
    )
    .bind(proposed_upload_id)
    .bind(input.owner_id)
    .bind(&input.original_filename)
    .bind(input.expected_size)
    .bind(&input.expected_blake3)
    .bind(&input.media_type)
    .bind(input.client_upload_key)
    .fetch_optional(pool)
    .await
    .map_err(UploadError::Database)?;

    let upload_id = if let Some(upload_id) = inserted_upload_id {
        upload_id
    } else {
        let Some(client_upload_key) = input.client_upload_key else {
            return Err(UploadError::InvalidInput);
        };
        let existing = sqlx::query_as::<_, (Uuid, String, i64, String, String)>(
            r#"
            SELECT id, original_filename, expected_size, expected_blake3, media_type
            FROM upload_sessions
            WHERE owner_id = $1
              AND client_upload_key = $2
            "#,
        )
        .bind(input.owner_id)
        .bind(client_upload_key)
        .fetch_optional(pool)
        .await
        .map_err(UploadError::Database)?
        .ok_or(UploadError::InvalidInput)?;

        if existing.1 != input.original_filename
            || existing.2 != input.expected_size
            || existing.3 != input.expected_blake3
            || existing.4 != input.media_type
        {
            return Err(UploadError::InvalidInput);
        }
        existing.0
    };

    get_upload(pool, input.owner_id, upload_id).await
}

/// Returns upload status and committed part indexes for resume.
pub async fn get_upload(
    pool: &PgPool,
    owner_id: i16,
    upload_id: Uuid,
) -> Result<UploadSessionView, UploadError> {
    let session = load_upload(pool, owner_id, upload_id).await?;
    let committed_parts = committed_parts(pool, upload_id).await?;
    Ok(UploadSessionView {
        upload_id,
        status: session.status,
        expected_size: session.expected_size,
        expected_blake3: session.expected_blake3,
        media_type: session.media_type,
        committed_parts,
    })
}

/// Writes or replaces one staged upload part.
pub async fn put_part(
    pool: &PgPool,
    storage: &ObjectStorage,
    owner_id: i16,
    upload_id: Uuid,
    part_index: i32,
    bytes: Vec<u8>,
) -> Result<(), UploadError> {
    let session = load_upload(pool, owner_id, upload_id).await?;
    if session.status != UploadStatus::Open {
        return Err(UploadError::NotOpen);
    }
    let expected_size =
        expected_part_size(session.expected_size, part_index).ok_or(UploadError::PartOutOfRange)?;
    let size = i64::try_from(bytes.len()).map_err(|_| UploadError::PartLengthMismatch)?;
    if size != expected_size {
        return Err(UploadError::PartLengthMismatch);
    }

    let key = StorageKey::staging_upload(upload_id, &format!("part-{part_index:08}"))
        .map_err(|_| UploadError::InvalidInput)?;
    let hash = blake3::hash(&bytes).to_hex().to_string();

    storage
        .write(&key, bytes)
        .await
        .map_err(UploadError::Storage)?;

    sqlx::query(
        r#"
        INSERT INTO upload_parts (upload_id, part_index, size_bytes, storage_key, blake3_hash)
        VALUES ($1, $2, $3, $4, $5)
        ON CONFLICT (upload_id, part_index)
        DO UPDATE SET
            size_bytes = EXCLUDED.size_bytes,
            storage_key = EXCLUDED.storage_key,
            blake3_hash = EXCLUDED.blake3_hash,
            created_at = now()
        "#,
    )
    .bind(upload_id)
    .bind(part_index)
    .bind(size)
    .bind(key.as_str())
    .bind(hash)
    .execute(pool)
    .await
    .map_err(UploadError::Database)?;

    Ok(())
}

/// Verifies staged parts match expected size, hash, and media signature.
pub async fn complete_upload(
    pool: &PgPool,
    storage: &ObjectStorage,
    owner_id: i16,
    upload_id: Uuid,
) -> Result<UploadSessionView, UploadError> {
    let session = load_upload(pool, owner_id, upload_id).await?;
    if session.status == UploadStatus::Verified {
        return get_upload(pool, owner_id, upload_id).await;
    }
    if session.status != UploadStatus::Open {
        return Err(UploadError::NotOpen);
    }

    let parts = part_rows(pool, upload_id).await?;
    let expected_part_count =
        expected_part_count(session.expected_size).ok_or(UploadError::VerificationFailed)?;
    if i64::try_from(parts.len()).map_err(|_| UploadError::VerificationFailed)?
        != expected_part_count
    {
        return Err(UploadError::VerificationFailed);
    }
    for (expected_part_index, part) in parts.iter().enumerate() {
        let expected_part_index =
            i32::try_from(expected_part_index).map_err(|_| UploadError::VerificationFailed)?;
        let expected_size = expected_part_size(session.expected_size, expected_part_index)
            .ok_or(UploadError::VerificationFailed)?;
        if part.part_index != expected_part_index || part.size_bytes != expected_size {
            return Err(UploadError::VerificationFailed);
        }
    }

    let mut hasher = Hasher::new();
    let mut total_size: i64 = 0;
    let mut first_bytes = Vec::new();

    for part in parts {
        let key = StorageKey::new(part.storage_key).map_err(|_| UploadError::VerificationFailed)?;
        let bytes = storage.read(&key).await.map_err(UploadError::Storage)?;
        let bytes_len = i64::try_from(bytes.len()).map_err(|_| UploadError::VerificationFailed)?;
        if bytes_len != part.size_bytes
            || blake3::hash(&bytes).to_hex().as_str() != part.blake3_hash
        {
            return Err(UploadError::VerificationFailed);
        }
        if first_bytes.len() < 12 {
            let remaining = 12 - first_bytes.len();
            first_bytes.extend(bytes.iter().copied().take(remaining));
        }
        total_size += bytes_len;
        hasher.update(&bytes);
    }

    let actual_hash = hasher.finalize().to_hex().to_string();
    if total_size != session.expected_size
        || actual_hash != session.expected_blake3
        || !media_signature_matches(&session.media_type, &first_bytes)
    {
        return Err(UploadError::VerificationFailed);
    }

    sqlx::query(
        r#"
        UPDATE upload_sessions
        SET status = $1,
            completed_at = COALESCE(completed_at, now()),
            updated_at = now()
        WHERE id = $2
          AND owner_id = $3
          AND status = 'open'
        "#,
    )
    .bind(UploadStatus::Verified.as_str())
    .bind(upload_id)
    .bind(owner_id)
    .execute(pool)
    .await
    .map_err(UploadError::Database)?;

    get_upload(pool, owner_id, upload_id).await
}

/// Cancels an open upload session. Staged object cleanup is separate.
pub async fn cancel_upload(
    pool: &PgPool,
    owner_id: i16,
    upload_id: Uuid,
) -> Result<(), UploadError> {
    let result = sqlx::query(
        r#"
        UPDATE upload_sessions
        SET status = 'cancelled',
            cancelled_at = COALESCE(cancelled_at, now()),
            updated_at = now()
        WHERE id = $1
          AND owner_id = $2
          AND status = 'open'
        "#,
    )
    .bind(upload_id)
    .bind(owner_id)
    .execute(pool)
    .await
    .map_err(UploadError::Database)?;

    if result.rows_affected() == 0 {
        return Err(UploadError::NotFound);
    }

    Ok(())
}

#[derive(Debug)]
struct UploadRow {
    status: UploadStatus,
    expected_size: i64,
    expected_blake3: String,
    media_type: String,
}

#[derive(Debug)]
struct PartRow {
    part_index: i32,
    size_bytes: i64,
    storage_key: String,
    blake3_hash: String,
}

async fn load_upload(
    pool: &PgPool,
    owner_id: i16,
    upload_id: Uuid,
) -> Result<UploadRow, UploadError> {
    let row = sqlx::query_as::<_, (String, i64, String, String)>(
        r#"
        SELECT status, expected_size, expected_blake3, media_type
        FROM upload_sessions
        WHERE id = $1
          AND owner_id = $2
        "#,
    )
    .bind(upload_id)
    .bind(owner_id)
    .fetch_optional(pool)
    .await
    .map_err(UploadError::Database)?
    .ok_or(UploadError::NotFound)?;

    Ok(UploadRow {
        status: parse_status(&row.0)?,
        expected_size: row.1,
        expected_blake3: row.2,
        media_type: row.3,
    })
}

async fn committed_parts(pool: &PgPool, upload_id: Uuid) -> Result<Vec<i32>, UploadError> {
    sqlx::query_scalar::<_, i32>(
        r#"
        SELECT part_index
        FROM upload_parts
        WHERE upload_id = $1
        ORDER BY part_index
        "#,
    )
    .bind(upload_id)
    .fetch_all(pool)
    .await
    .map_err(UploadError::Database)
}

async fn part_rows(pool: &PgPool, upload_id: Uuid) -> Result<Vec<PartRow>, UploadError> {
    let rows = sqlx::query_as::<_, (i32, i64, String, String)>(
        r#"
        SELECT part_index, size_bytes, storage_key, blake3_hash
        FROM upload_parts
        WHERE upload_id = $1
        ORDER BY part_index
        "#,
    )
    .bind(upload_id)
    .fetch_all(pool)
    .await
    .map_err(UploadError::Database)?;

    Ok(rows
        .into_iter()
        .map(
            |(part_index, size_bytes, storage_key, blake3_hash)| PartRow {
                part_index,
                size_bytes,
                storage_key,
                blake3_hash,
            },
        )
        .collect())
}

fn validate_create_input(input: &CreateUploadInput) -> Result<(), UploadError> {
    if input.original_filename.trim().is_empty()
        || input.original_filename.chars().count() > 255
        || input.expected_size <= 0
        || expected_part_count(input.expected_size)
            .is_none_or(|count| count > i64::from(i32::MAX) + 1)
        || !is_blake3_hex(&input.expected_blake3)
        || !is_supported_media_type(&input.media_type)
    {
        return Err(UploadError::InvalidInput);
    }
    Ok(())
}

fn expected_part_count(expected_size: i64) -> Option<i64> {
    (expected_size > 0).then(|| ((expected_size - 1) / UPLOAD_PART_SIZE_I64) + 1)
}

fn expected_part_size(expected_size: i64, part_index: i32) -> Option<i64> {
    let part_count = expected_part_count(expected_size)?;
    let part_index = i64::from(part_index);
    if part_index < 0 || part_index >= part_count {
        return None;
    }
    if part_index + 1 < part_count {
        return Some(UPLOAD_PART_SIZE_I64);
    }

    let remainder = expected_size % UPLOAD_PART_SIZE_I64;
    Some(if remainder == 0 {
        UPLOAD_PART_SIZE_I64
    } else {
        remainder
    })
}

fn parse_status(status: &str) -> Result<UploadStatus, UploadError> {
    match status {
        "open" => Ok(UploadStatus::Open),
        "verified" => Ok(UploadStatus::Verified),
        "cancelled" => Ok(UploadStatus::Cancelled),
        _ => Err(UploadError::InvalidInput),
    }
}

fn is_blake3_hex(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn is_supported_media_type(value: &str) -> bool {
    matches!(
        value,
        "image/jpeg" | "image/png" | "image/gif" | "image/webp" | "video/mp4"
    )
}

fn media_signature_matches(media_type: &str, bytes: &[u8]) -> bool {
    match media_type {
        "image/jpeg" => bytes.starts_with(&[0xff, 0xd8, 0xff]),
        "image/png" => bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        "image/gif" => bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a"),
        "image/webp" => bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP",
        "video/mp4" => bytes.len() >= 12 && &bytes[4..8] == b"ftyp",
        _ => false,
    }
}
