//! Shared path safety boundaries.

use std::path::{Component, Path, PathBuf};

/// Ensures a path contains only valid, non-traversing components.
///
/// Disallows absolute paths, current-dir references (`.`), and parent-dir
/// references (`..`).
pub fn clean_path(path: &Path) -> Result<PathBuf, &'static str> {
    if path.as_os_str().is_empty() {
        return Err("Path is empty");
    }

    for component in path.components() {
        match component {
            Component::Normal(_) => continue,
            Component::RootDir | Component::Prefix(_) => return Err("Path cannot be absolute"),
            Component::CurDir | Component::ParentDir => {
                return Err("Path cannot traverse directories");
            }
        }
    }

    Ok(path.to_path_buf())
}

/// Ensures a relative slash-separated string contains no unsafe segments.
pub fn validate_relative_str(value: &str, max_len: usize) -> Result<(), &'static str> {
    if value.is_empty() {
        return Err("String path is empty");
    }
    if value.len() > max_len {
        return Err("String path is too long");
    }
    if value.starts_with('/') {
        return Err("String path cannot be absolute");
    }
    if value.contains('\\') {
        return Err("String path cannot contain backslashes");
    }
    if value
        .split('/')
        .any(|segment| segment.is_empty() || matches!(segment, "." | ".."))
    {
        return Err("String path contains unsafe traversal segments");
    }

    Ok(())
}
