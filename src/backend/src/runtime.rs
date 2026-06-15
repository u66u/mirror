//! Shared process-startup helpers.

use std::io;

/// Converts a typed startup failure into an `io::Error` for binary entrypoints.
pub fn io_other(error: impl std::error::Error + Send + Sync + 'static) -> io::Error {
    io::Error::other(error)
}
