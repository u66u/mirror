//! HTTP error mapping.
//!
//! Responses expose stable categories and avoid leaking database or secret
//! details.

use actix_web::{HttpResponse, ResponseError, http::StatusCode};
use serde::Serialize;

use crate::{
    assets::{AssetMutationError, AssetReadError, ListAssetsError, PromoteError},
    auth::{DeviceTokenError, OwnerLoginError, OwnerSetupError},
    exports::ExportError,
    ml::MlError,
    models::ModelPackError,
    search::SearchError,
    semantic_index::SemanticIndexError,
    shares::ShareError,
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
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    /// Request violates local validation.
    #[error("{1}")]
    BadRequest(&'static str, &'static str),
    /// Request body exceeds a route-local size limit.
    #[error("{1}")]
    PayloadTooLarge(&'static str, &'static str),
    /// Authentication or setup token failed.
    #[error("{1}")]
    Unauthorized(&'static str, &'static str),
    /// Request is temporarily blocked by rate limiting.
    #[error("{1}")]
    TooManyRequests(&'static str, &'static str),
    /// Requested state transition conflicts with persisted state.
    #[error("{1}")]
    Conflict(&'static str, &'static str),
    /// Requested resource does not exist for the caller.
    #[error("{1}")]
    NotFound(&'static str, &'static str),
    /// Required dependency is unavailable.
    #[error("{1}")]
    ServiceUnavailable(&'static str, &'static str),
    /// Internal failure. Details are logged server-side only.
    #[error("internal error")]
    Internal,
}

impl ResponseError for ApiError {
    fn status_code(&self) -> StatusCode {
        match self {
            Self::BadRequest(_, _) => StatusCode::BAD_REQUEST,
            Self::PayloadTooLarge(_, _) => StatusCode::PAYLOAD_TOO_LARGE,
            Self::Unauthorized(_, _) => StatusCode::UNAUTHORIZED,
            Self::TooManyRequests(_, _) => StatusCode::TOO_MANY_REQUESTS,
            Self::Conflict(_, _) => StatusCode::CONFLICT,
            Self::NotFound(_, _) => StatusCode::NOT_FOUND,
            Self::ServiceUnavailable(_, _) => StatusCode::SERVICE_UNAVAILABLE,
            Self::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn error_response(&self) -> HttpResponse {
        let (error, message) = match self {
            Self::BadRequest(error, message)
            | Self::PayloadTooLarge(error, message)
            | Self::Unauthorized(error, message)
            | Self::TooManyRequests(error, message)
            | Self::Conflict(error, message)
            | Self::NotFound(error, message)
            | Self::ServiceUnavailable(error, message) => (*error, *message),
            Self::Internal => ("internal_error", "internal error"),
        };

        HttpResponse::build(self.status_code()).json(ErrorBody { error, message })
    }
}

macro_rules! map_api_errors {
    (
        impl From<$err_type:ty> for ApiError;
        $(
            $( $variant:pat_param )|+ => $response:expr,
        )*
        @internal => $( $internal_variant:pat_param )|+ $(,)?
    ) => {
        impl From<$err_type> for ApiError {
            fn from(error: $err_type) -> Self {
                match error {
                    $( $( $variant )|+ => $response, )*
                    $( $internal_variant )|+ => {
                        tracing::error!(action = "http_error_mapped", ?error, "internal server error");
                        Self::Internal
                    }
                }
            }
        }
    };
}

map_api_errors! {
    impl From<OwnerSetupError> for ApiError;
    OwnerSetupError::SetupUnavailable => Self::Conflict("setup_unavailable", "owner setup is unavailable"),
    OwnerSetupError::InvalidSetupToken => Self::Unauthorized("invalid_setup_token", "invalid setup token"),
    OwnerSetupError::InvalidDisplayName => Self::BadRequest("invalid_display_name", "invalid display name"),
    OwnerSetupError::InvalidPassword => Self::BadRequest("invalid_password", "invalid password"),
    OwnerSetupError::OwnerAlreadyExists => Self::Conflict("owner_exists", "owner already exists"),
    @internal => OwnerSetupError::Database(_)
}

map_api_errors! {
    impl From<OwnerLoginError> for ApiError;
    OwnerLoginError::InvalidCredentials => Self::Unauthorized("invalid_credentials", "invalid credentials"),
    @internal => OwnerLoginError::Session(_) | OwnerLoginError::Database(_)
}

map_api_errors! {
    impl From<DeviceTokenError> for ApiError;
    DeviceTokenError::InvalidName => Self::BadRequest("invalid_device_name", "invalid device name"),
    @internal => DeviceTokenError::TokenGeneration | DeviceTokenError::Database(_)
}

map_api_errors! {
    impl From<UploadError> for ApiError;
    UploadError::InvalidInput => Self::BadRequest("invalid_upload", "invalid upload"),
    UploadError::PartOutOfRange => Self::BadRequest("upload_part_out_of_range", "upload part index is out of range"),
    UploadError::PartLengthMismatch => Self::BadRequest("upload_part_wrong_length", "upload part has the wrong length"),
    UploadError::NotFound => Self::BadRequest("upload_not_found", "upload not found"),
    UploadError::NotOpen => Self::Conflict("upload_not_open", "upload is not open"),
    UploadError::VerificationFailed => Self::BadRequest("upload_verification_failed", "upload verification failed"),
    @internal => UploadError::Storage(_) | UploadError::Database(_)
}

map_api_errors! {
    impl From<PromoteError> for ApiError;
    PromoteError::UploadNotVerified => Self::Conflict("upload_not_verified", "upload is not verified"),
    PromoteError::VerificationFailed => Self::BadRequest("upload_verification_failed", "upload verification failed"),
    @internal => PromoteError::Storage(_) | PromoteError::Database(_)
}

map_api_errors! {
    impl From<ListAssetsError> for ApiError;
    ListAssetsError::InvalidInput => Self::BadRequest("invalid_asset_list", "invalid asset list"),
    @internal => ListAssetsError::Database(_)
}

map_api_errors! {
    impl From<AssetReadError> for ApiError;
    AssetReadError::NotFound => Self::NotFound("asset_not_found", "asset not found"),
    AssetReadError::InvalidInput => Self::BadRequest("invalid_asset_request", "invalid asset request"),
    @internal => AssetReadError::Database(_)
}

map_api_errors! {
    impl From<AssetMutationError> for ApiError;
    AssetMutationError::NotFound => Self::NotFound("asset_not_found", "asset not found"),
    AssetMutationError::NotTrashed => Self::Conflict("asset_not_trashed", "asset is not in trash"),
    @internal => AssetMutationError::Database(_)
}

map_api_errors! {
    impl From<ShareError> for ApiError;
    ShareError::InvalidInput => Self::BadRequest("invalid_share", "invalid share"),
    ShareError::NotFound => Self::NotFound("share_not_found", "share not found"),
    @internal => ShareError::TokenGeneration | ShareError::Database(_)
}

map_api_errors! {
    impl From<ExportError> for ApiError;
    ExportError::NotFound => Self::NotFound("export_not_found", "export not found"),
    @internal => ExportError::InvalidStorageKey | ExportError::Database(_)
}

map_api_errors! {
    impl From<SearchError> for ApiError;
    SearchError::InvalidInput => Self::BadRequest("invalid_search", "invalid search"),
    @internal => SearchError::Database(_)
}

map_api_errors! {
    impl From<MlError> for ApiError;
    MlError::InvalidTextQuery | MlError::SemanticIndex(SemanticIndexError::InvalidLimit) => Self::BadRequest("invalid_search", "invalid search"),
    MlError::NotFound | MlError::RuntimeUnavailable => Self::ServiceUnavailable("semantic_search_unavailable", "semantic search is unavailable"),
    @internal => MlError::UnsupportedJobKind | MlError::InvalidJobPayload | MlError::UnsupportedMediaType | MlError::ImageTooLarge | MlError::InvalidImage | MlError::InvalidStorageKey(_) | MlError::Storage(_) | MlError::Model(_) | MlError::SemanticIndex(_) | MlError::Database(_)
}

map_api_errors! {
    impl From<ModelPackError> for ApiError;
    ModelPackError::InvalidManifest(_) => Self::BadRequest("invalid_model_pack", "invalid model pack"),
    ModelPackError::NotFound => Self::NotFound("model_pack_not_found", "model pack not found"),
    ModelPackError::SelfTestRequired => Self::Conflict("model_pack_self_test_required", "model pack self-test has not passed"),
    ModelPackError::FileVerificationFailed => Self::BadRequest("model_pack_file_verification_failed", "model pack file verification failed"),
    @internal => ModelPackError::InvalidFilePath | ModelPackError::InvalidEmbedding(_) | ModelPackError::Io(_) | ModelPackError::Json(_) | ModelPackError::Storage(_) | ModelPackError::StorageKey(_) | ModelPackError::Job(_) | ModelPackError::Database(_)
}
