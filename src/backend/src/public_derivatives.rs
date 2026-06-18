//! Public derivative policy shared by owner and share routes.

/// Returns whether a derivative kind can be served over HTTP.
#[must_use]
pub(crate) fn public_kind_allowed(kind: &str) -> bool {
    matches!(kind, "thumbnail" | "preview")
}

/// Maps stored derivative format to an HTTP content type.
#[must_use]
pub(crate) fn public_format_content_type(format: &str) -> Option<&'static str> {
    match format {
        "webp" => Some("image/webp"),
        _ => None,
    }
}
