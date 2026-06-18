//! Face indexing job foundation.
//!
//! Real detection/embedding arrives behind this boundary. For v1 foundation,
//! the job kind is intentionally disabled unless the worker is explicitly
//! configured to lease it.

use thiserror::Error;

use crate::jobs::{JobKind, LeasedJob};

/// Face indexing failure.
#[derive(Debug, Error)]
pub enum FaceIndexError {
    /// Worker does not run face indexing yet.
    #[error("face indexing runtime is unavailable")]
    RuntimeUnavailable,
    /// Wrong queue kind reached the face handler.
    #[error("unsupported face job kind")]
    UnsupportedJobKind,
}

/// Runs one face indexing job.
pub async fn run_face_index_job(job: &LeasedJob) -> Result<(), FaceIndexError> {
    if job.kind != JobKind::IndexFaces {
        return Err(FaceIndexError::UnsupportedJobKind);
    }
    Err(FaceIndexError::RuntimeUnavailable)
}
