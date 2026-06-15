//! HTTP error mapping.
//!
//! Responses expose stable categories and avoid leaking database or secret
//! details.

use actix_web::{HttpResponse, ResponseError, http::StatusCode};
use serde::Serialize;

use crate::{
    assets::{AssetReadError, ListAssetsError, PromoteError},
    auth::{OwnerLoginError, OwnerSetupError},
    uploads::UploadError,
};

/// API error response.
#[derive(Debug, Serialize)]
pub struct ErrorBody {
    /// Stable machine-readable code.
    pub error: &'static str,
    /// Human-readable non-secret message.
    pub message: &'static str,
}

/// HTTP boundary error.
#[derive(Debug)]
pub enum ApiError {
    /// Request violates local validation.
    BadRequest(&'static str, &'static str),
    /// Authentication or setup token failed.
    Unauthorized(&'static str, &'static str),
    /// Requested state transition conflicts with persisted state.
    Conflict(&'static str, &'static str),
    /// Requested resource does not exist for the caller.
    NotFound(&'static str, &'static str),
    /// Required dependency is unavailable.
    ServiceUnavailable(&'static str, &'static str),
    /// Internal failure. Details are logged server-side only.
    Internal,
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::BadRequest(_, message)
            | Self::Unauthorized(_, message)
            | Self::Conflict(_, message)
            | Self::NotFound(_, message)
            | Self::ServiceUnavailable(_, message) => *message,
            Self::Internal => "internal error",
        };
        formatter.write_str(message)
    }
}

impl ResponseError for ApiError {
    fn status_code(&self) -> StatusCode {
        match self {
            Self::BadRequest(_, _) => StatusCode::BAD_REQUEST,
            Self::Unauthorized(_, _) => StatusCode::UNAUTHORIZED,
            Self::Conflict(_, _) => StatusCode::CONFLICT,
            Self::NotFound(_, _) => StatusCode::NOT_FOUND,
            Self::ServiceUnavailable(_, _) => StatusCode::SERVICE_UNAVAILABLE,
            Self::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn error_response(&self) -> HttpResponse {
        let (error, message) = match self {
            Self::BadRequest(error, message)
            | Self::Unauthorized(error, message)
            | Self::Conflict(error, message)
            | Self::NotFound(error, message)
            | Self::ServiceUnavailable(error, message) => (*error, *message),
            Self::Internal => ("internal_error", "internal error"),
        };

        HttpResponse::build(self.status_code()).json(ErrorBody { error, message })
    }
}

impl From<OwnerSetupError> for ApiError {
    fn from(error: OwnerSetupError) -> Self {
        match error {
            OwnerSetupError::SetupUnavailable => {
                Self::Conflict("setup_unavailable", "owner setup is unavailable")
            }
            OwnerSetupError::InvalidSetupToken => {
                Self::Unauthorized("invalid_setup_token", "invalid setup token")
            }
            OwnerSetupError::InvalidDisplayName => {
                Self::BadRequest("invalid_display_name", "invalid display name")
            }
            OwnerSetupError::InvalidPassword => {
                Self::BadRequest("invalid_password", "invalid password")
            }
            OwnerSetupError::OwnerAlreadyExists => {
                Self::Conflict("owner_exists", "owner already exists")
            }
            OwnerSetupError::Database(_) => Self::Internal,
        }
    }
}

impl From<OwnerLoginError> for ApiError {
    fn from(error: OwnerLoginError) -> Self {
        match error {
            OwnerLoginError::InvalidCredentials => {
                Self::Unauthorized("invalid_credentials", "invalid credentials")
            }
            OwnerLoginError::Session(_) | OwnerLoginError::Database(_) => Self::Internal,
        }
    }
}

impl From<UploadError> for ApiError {
    fn from(error: UploadError) -> Self {
        match error {
            UploadError::InvalidInput => Self::BadRequest("invalid_upload", "invalid upload"),
            UploadError::NotFound => Self::BadRequest("upload_not_found", "upload not found"),
            UploadError::NotOpen => Self::Conflict("upload_not_open", "upload is not open"),
            UploadError::VerificationFailed => {
                Self::BadRequest("upload_verification_failed", "upload verification failed")
            }
            UploadError::Storage(_) | UploadError::Database(_) => Self::Internal,
        }
    }
}

impl From<PromoteError> for ApiError {
    fn from(error: PromoteError) -> Self {
        match error {
            PromoteError::UploadNotVerified => {
                Self::Conflict("upload_not_verified", "upload is not verified")
            }
            PromoteError::VerificationFailed => {
                Self::BadRequest("upload_verification_failed", "upload verification failed")
            }
            PromoteError::Storage(_) | PromoteError::Database(_) => Self::Internal,
        }
    }
}

impl From<ListAssetsError> for ApiError {
    fn from(error: ListAssetsError) -> Self {
        match error {
            ListAssetsError::InvalidInput => {
                Self::BadRequest("invalid_asset_list", "invalid asset list")
            }
            ListAssetsError::Database(_) => Self::Internal,
        }
    }
}

impl From<AssetReadError> for ApiError {
    fn from(error: AssetReadError) -> Self {
        match error {
            AssetReadError::NotFound => Self::NotFound("asset_not_found", "asset not found"),
            AssetReadError::InvalidInput => {
                Self::BadRequest("invalid_asset_request", "invalid asset request")
            }
            AssetReadError::Database(_) => Self::Internal,
        }
    }
}
