//! Storage backend boundary.
//!
//! C001: object writes are not atomic with Postgres transactions. Callers that
//! promote objects before DB commit must handle orphan cleanup later.

mod keys;

use std::path::Path;

use futures_util::TryStreamExt;
use opendal::{Operator, services::Fs};
use tokio::io::AsyncWriteExt;

pub use keys::{StorageKey, StorageKeyError};

/// Object storage wrapper used by upload/media code.
#[derive(Clone)]
pub struct ObjectStorage {
    operator: Operator,
}

impl ObjectStorage {
    /// Builds a local filesystem-backed OpenDAL operator rooted outside any web
    /// server public directory.
    pub fn local(root: impl AsRef<Path>) -> Result<Self, StorageError> {
        let builder = Fs::default().root(
            root.as_ref()
                .to_str()
                .ok_or(StorageError::InvalidLocalRoot)?,
        );
        let operator = Operator::new(builder)
            .map_err(StorageError::OpenDal)?
            .finish();

        Ok(Self { operator })
    }

    /// Writes bytes to a generated storage key.
    pub async fn write(&self, key: &StorageKey, bytes: Vec<u8>) -> Result<(), StorageError> {
        self.operator
            .write(key.as_str(), bytes)
            .await
            .map(|_| ())
            .map_err(StorageError::OpenDal)
    }

    /// Reads a complete object.
    pub async fn read(&self, key: &StorageKey) -> Result<Vec<u8>, StorageError> {
        self.operator
            .read(key.as_str())
            .await
            .map(|buffer| buffer.to_vec())
            .map_err(StorageError::OpenDal)
    }

    /// Streams an object into a new file while enforcing a byte limit.
    ///
    /// C005: large video originals must not be materialized as one in-memory
    /// buffer before external media tools inspect them.
    pub async fn copy_to_path_bounded(
        &self,
        key: &StorageKey,
        destination: &Path,
        max_bytes: u64,
    ) -> Result<u64, StorageError> {
        let reader = self
            .operator
            .reader(key.as_str())
            .await
            .map_err(StorageError::OpenDal)?;
        let mut stream = reader
            .into_stream(..)
            .await
            .map_err(StorageError::OpenDal)?;
        let mut file = tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(destination)
            .await
            .map_err(StorageError::Io)?;
        let mut written = 0_u64;

        while let Some(buffer) = stream.try_next().await.map_err(StorageError::OpenDal)? {
            for chunk in buffer {
                written = written
                    .checked_add(
                        u64::try_from(chunk.len()).map_err(|_| StorageError::ObjectTooLarge)?,
                    )
                    .ok_or(StorageError::ObjectTooLarge)?;
                if written > max_bytes {
                    drop(file);
                    let _ = tokio::fs::remove_file(destination).await;
                    return Err(StorageError::ObjectTooLarge);
                }
                file.write_all(&chunk).await.map_err(StorageError::Io)?;
            }
        }
        file.flush().await.map_err(StorageError::Io)?;
        Ok(written)
    }

    /// Returns whether the object exists.
    pub async fn exists(&self, key: &StorageKey) -> Result<bool, StorageError> {
        self.operator
            .exists(key.as_str())
            .await
            .map_err(StorageError::OpenDal)
    }

    /// Deletes an object. OpenDAL delete is idempotent for missing paths.
    pub async fn delete(&self, key: &StorageKey) -> Result<(), StorageError> {
        self.operator
            .delete(key.as_str())
            .await
            .map_err(StorageError::OpenDal)
    }

    /// Lists object keys under a generated prefix.
    pub async fn list(&self, prefix: &StorageKey) -> Result<Vec<String>, StorageError> {
        self.operator
            .list(prefix.as_str())
            .await
            .map(|entries| {
                entries
                    .into_iter()
                    .filter(|entry| !entry.metadata().is_dir())
                    .map(|entry| entry.path().to_owned())
                    .collect()
            })
            .map_err(StorageError::OpenDal)
    }

    /// Recursively lists object keys under a generated prefix.
    pub async fn list_recursive(&self, prefix: &StorageKey) -> Result<Vec<String>, StorageError> {
        self.operator
            .list_with(prefix.as_str())
            .recursive(true)
            .await
            .map(|entries| {
                entries
                    .into_iter()
                    .filter(|entry| !entry.metadata().is_dir())
                    .map(|entry| entry.path().to_owned())
                    .collect()
            })
            .map_err(StorageError::OpenDal)
    }

    /// Promotes a staged object to final storage by copy/delete.
    ///
    /// This is deliberately not named atomic. S3-compatible backends do not have
    /// identical rename semantics, so callers must tolerate a crash between copy
    /// and delete.
    pub async fn promote(
        &self,
        staged: &StorageKey,
        final_key: &StorageKey,
    ) -> Result<(), StorageError> {
        self.operator
            .copy(staged.as_str(), final_key.as_str())
            .await
            .map_err(StorageError::OpenDal)?;
        self.delete(staged).await
    }
}

/// Storage operation failure.
#[derive(Debug)]
pub enum StorageError {
    /// Local storage root path is not valid UTF-8.
    InvalidLocalRoot,
    /// OpenDAL returned an operation error.
    OpenDal(opendal::Error),
    /// Local staging file I/O failed.
    Io(std::io::Error),
    /// Object exceeded the caller's staging limit.
    ObjectTooLarge,
}

impl std::fmt::Display for StorageError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidLocalRoot => formatter.write_str("invalid local storage root"),
            Self::OpenDal(_) => formatter.write_str("storage backend error"),
            Self::Io(_) => formatter.write_str("storage staging I/O error"),
            Self::ObjectTooLarge => formatter.write_str("storage object exceeds staging limit"),
        }
    }
}

impl std::error::Error for StorageError {}
