//! Generated storage keys.
//!
//! Keys are not user paths. Constructors validate shape so callers cannot
//! smuggle absolute paths, parent traversal, or backend-specific separators.

use uuid::Uuid;
use thiserror::Error;

/// Valid object key relative to the storage root.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StorageKey(String);

impl StorageKey {
    /// Builds the prefix containing all immutable BLAKE3 originals.
    pub fn originals_blake3_prefix() -> Self {
        Self("originals/blake3/".to_owned())
    }

    /// Builds the prefix containing generated derivatives.
    pub fn derivatives_prefix() -> Self {
        Self("derivatives/".to_owned())
    }

    /// Builds the prefix containing installed model-pack files.
    pub fn model_packs_prefix() -> Self {
        Self("model-packs/".to_owned())
    }

    /// Builds a staging key for an upload object.
    pub fn staging_upload(upload_id: Uuid, object_name: &str) -> Result<Self, StorageKeyError> {
        Self::new(format!("staging/uploads/{upload_id}/{object_name}"))
    }

    /// Builds the prefix that contains all staged objects for one upload.
    pub fn staging_upload_prefix(upload_id: Uuid) -> Self {
        Self(format!("staging/uploads/{upload_id}/"))
    }

    /// Builds immutable original key from BLAKE3 hex digest.
    pub fn original_blake3(hash_hex: &str) -> Result<Self, StorageKeyError> {
        validate_blake3_hex(hash_hex)?;
        Self::new(format!(
            "originals/blake3/{}/{}/{}",
            &hash_hex[0..2],
            &hash_hex[2..4],
            hash_hex
        ))
    }

    /// Builds a reproducible derivative key from original content and generator metadata.
    pub fn derivative(
        hash_hex: &str,
        kind: &str,
        format: &str,
        generator_version: &str,
    ) -> Result<Self, StorageKeyError> {
        validate_blake3_hex(hash_hex)?;
        validate_segment(kind)?;
        validate_segment(format)?;
        validate_segment(generator_version)?;
        Self::new(format!(
            "derivatives/{generator_version}/{kind}/{format}/{}/{}/{}.{}",
            &hash_hex[0..2],
            &hash_hex[2..4],
            hash_hex,
            format
        ))
    }

    /// Builds a storage key for a validated model-pack file.
    pub fn model_pack_file(
        model_pack_id: Uuid,
        relative_path: &str,
    ) -> Result<Self, StorageKeyError> {
        validate_model_pack_path(relative_path)?;
        Self::new(format!("model-packs/{model_pack_id}/{relative_path}"))
    }

    /// Validates a relative backend key.
    pub fn new(key: impl Into<String>) -> Result<Self, StorageKeyError> {
        let key = key.into();
        validate_key(&key)?;
        Ok(Self(key))
    }

    /// Returns the backend path.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Returns whether this is a canonical content-addressed original key.
    #[must_use]
    pub fn is_original_blake3_object(&self) -> bool {
        self.0
            .rsplit('/')
            .next()
            .and_then(|hash| Self::original_blake3(hash).ok())
            .is_some_and(|canonical| canonical == *self)
    }
}

/// Storage key validation failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum StorageKeyError {
    /// Key is empty.
    #[error("empty storage key")]
    Empty,
    /// Key attempts absolute pathing or parent traversal.
    #[error("unsafe storage key")]
    UnsafePath,
    /// BLAKE3 hex digest is malformed.
    #[error("invalid blake3 hash")]
    InvalidHash,
}

fn validate_key(key: &str) -> Result<(), StorageKeyError> {
    if key.is_empty() {
        return Err(StorageKeyError::Empty);
    }
    if key.starts_with('/') || key.starts_with('\\') || key.contains('\\') {
        return Err(StorageKeyError::UnsafePath);
    }
    if key
        .split('/')
        .any(|component| component.is_empty() || component == "." || component == "..")
    {
        return Err(StorageKeyError::UnsafePath);
    }
    Ok(())
}

fn validate_blake3_hex(hash_hex: &str) -> Result<(), StorageKeyError> {
    if hash_hex.len() != 64 || !hash_hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(StorageKeyError::InvalidHash);
    }
    Ok(())
}

fn validate_segment(segment: &str) -> Result<(), StorageKeyError> {
    if segment.is_empty()
        || !segment
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        return Err(StorageKeyError::UnsafePath);
    }
    Ok(())
}

fn validate_model_pack_path(path: &str) -> Result<(), StorageKeyError> {
    if path.is_empty()
        || path.len() > 300
        || path.starts_with('/')
        || path.contains('\\')
        || path
            .split('/')
            .any(|segment| segment.is_empty() || matches!(segment, "." | ".."))
    {
        Err(StorageKeyError::UnsafePath)
    } else {
        Ok(())
    }
}
