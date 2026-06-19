//! Authentication routes.
//!
//! C002/C003: handlers never log raw passwords/session tokens. Cookie security
//! currently uses local HTTPS config only; trusted-proxy handling comes later.

use actix_web::{
    HttpRequest, HttpResponse,
    cookie::{Cookie, SameSite, time::Duration},
    delete, get, post, web,
};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    auth::{self, AuthenticatedSession, OwnerLoginInput},
    http::{client_ip, error::ApiError},
    rate_limit::{self, FailureInput},
    state::AppState,
};

const SESSION_COOKIE: &str = "mirror_session";
const CSRF_COOKIE: &str = "mirror_csrf";
const CSRF_HEADER: &str = "x-csrf-token";
const SESSION_MAX_AGE: Duration = Duration::days(30);
const OWNER_PASSWORD_LOGIN_ACTION: &str = "owner_password_login";
const OWNER_PASSWORD_REAUTH_ACTION: &str = "owner_password_reauth";
const OWNER_MFA_ACTION: &str = "owner_mfa";

/// Owner login request body.
#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    /// Owner password.
    pub password: String,
    /// Six-digit TOTP code, required when TOTP is enabled unless using recovery code.
    pub totp_code: Option<String>,
    /// One-time recovery code, accepted when TOTP is enabled.
    pub recovery_code: Option<String>,
    /// Optional browser/device label.
    pub device_name: Option<String>,
}

/// Owner MFA status response.
#[derive(Debug, Serialize)]
pub struct MfaStatusResponse {
    /// Whether TOTP is active for login.
    pub totp_enabled: bool,
    /// Whether setup has generated a pending secret not yet verified.
    pub totp_setup_pending: bool,
    /// Number of unused recovery codes.
    pub recovery_codes_remaining: i64,
}

/// Password reauth request for MFA setup.
#[derive(Debug, Deserialize)]
pub struct MfaPasswordRequest {
    /// Current owner password.
    pub password: String,
}

/// TOTP setup response.
#[derive(Debug, Serialize)]
pub struct TotpSetupResponse {
    /// Base32 TOTP secret for manual authenticator entry.
    pub secret_base32: String,
    /// Standard otpauth URI for authenticator apps.
    pub provisioning_uri: String,
}

/// TOTP enable request.
#[derive(Debug, Deserialize)]
pub struct TotpEnableRequest {
    /// Current owner password.
    pub password: String,
    /// Six-digit TOTP code from the pending secret.
    pub totp_code: String,
}

/// Second-factor management request.
#[derive(Debug, Deserialize)]
pub struct MfaSecondFactorRequest {
    /// Current owner password.
    pub password: String,
    /// Six-digit TOTP code.
    pub totp_code: Option<String>,
    /// One-time recovery code.
    pub recovery_code: Option<String>,
}

/// Recovery codes response.
#[derive(Debug, Serialize)]
pub struct RecoveryCodesResponse {
    /// Raw one-time recovery codes returned only once.
    pub recovery_codes: Vec<String>,
}

/// Session inventory response row.
#[derive(Debug, Serialize)]
pub struct SessionResponse {
    /// Persisted session ID.
    pub session_id: Uuid,
    /// Optional browser/device label.
    pub device_name: Option<String>,
    /// User-agent metadata.
    pub user_agent: Option<String>,
    /// Creation timestamp.
    pub created_at: OffsetDateTime,
    /// Last successful authentication timestamp.
    pub last_seen_at: Option<OffsetDateTime>,
    /// Expiration timestamp.
    pub expires_at: OffsetDateTime,
    /// Whether this row is the caller's session.
    pub is_current: bool,
}

/// Android device-token creation request.
#[derive(Debug, Deserialize)]
pub struct CreateDeviceTokenRequest {
    /// Device display name.
    pub name: String,
    /// Current owner password for recent reauthentication.
    pub password: String,
    /// Six-digit TOTP code, required when TOTP is enabled unless using recovery code.
    pub totp_code: Option<String>,
    /// One-time recovery code, accepted when TOTP is enabled.
    pub recovery_code: Option<String>,
}

/// Android device-token creation response.
#[derive(Debug, Serialize)]
pub struct CreateDeviceTokenResponse {
    /// Persisted device token ID.
    pub device_token_id: Uuid,
    /// Raw token returned once for Android encrypted storage.
    pub token: String,
}

/// Authenticated owner credential accepted by first-party API routes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnerCredential {
    /// Browser session cookie. Unsafe requests also require CSRF validation.
    Session(AuthenticatedSession),
    /// Android bearer token. Authorization headers are not ambient credentials,
    /// so CSRF validation does not apply.
    Device(auth::AuthenticatedDeviceToken),
}

impl OwnerCredential {
    /// Returns single-owner database identity.
    pub fn owner_id(self) -> i16 {
        match self {
            Self::Session(session) => session.owner_id,
            Self::Device(device) => device.owner_id,
        }
    }

    /// Builds a stable, raw quota key for authenticated expensive routes.
    ///
    /// The returned value is immediately keyed-hashed before storage by
    /// `rate_limit`, so session/device IDs and IPs are not persisted directly.
    pub fn rate_limit_key(self, req: &HttpRequest, trusted_proxies: &[ipnet::IpNet]) -> String {
        let ip = client_ip::client_ip(req, trusted_proxies)
            .map(|ip| ip.to_string())
            .unwrap_or_else(|| "unknown-peer".to_owned());
        match self {
            Self::Session(session) => {
                format!(
                    "owner:{}:session:{}:ip:{ip}",
                    session.owner_id, session.session_id
                )
            }
            Self::Device(device) => {
                format!(
                    "owner:{}:device:{}:ip:{ip}",
                    device.owner_id, device.device_token_id
                )
            }
        }
    }
}

/// Creates a web session cookie.
#[post("/auth/login")]
pub async fn login(
    state: web::Data<AppState>,
    req: HttpRequest,
    body: web::Json<LoginRequest>,
) -> Result<HttpResponse, ApiError> {
    let Some(pool) = state.db.as_ref() else {
        return Err(ApiError::ServiceUnavailable(
            "database_unavailable",
            "database is unavailable",
        ));
    };
    let rate_limit_key = owner_password_login_key(&req, &state);
    reject_blocked_owner_password_login(&state, pool, &rate_limit_key).await?;
    if auth::second_factor_enabled(pool)
        .await
        .map_err(|_| ApiError::Internal)?
    {
        reject_blocked_owner_mfa(&state, pool, &rate_limit_key).await?;
    }

    let output = match auth::login_owner(
        pool,
        &state.config.auth_secret,
        OwnerLoginInput {
            password: body.password.clone(),
            second_factor: Some(second_factor_from_login(&body)),
            user_agent: user_agent(&req),
            device_name: body.device_name.clone(),
        },
    )
    .await
    {
        Ok(output) => {
            clear_owner_password_login_limit(&state, pool, &rate_limit_key).await?;
            clear_owner_mfa_limit(&state, pool, &rate_limit_key).await?;
            output
        }
        Err(auth::OwnerLoginError::InvalidCredentials) => {
            record_owner_password_login_failure(&state, pool, &rate_limit_key).await?;
            return Err(ApiError::Unauthorized(
                "invalid_credentials",
                "invalid credentials",
            ));
        }
        Err(auth::OwnerLoginError::SecondFactorRequired) => {
            return Err(ApiError::Unauthorized(
                "second_factor_required",
                "second factor required",
            ));
        }
        Err(auth::OwnerLoginError::InvalidSecondFactor) => {
            record_owner_mfa_failure(&state, pool, &rate_limit_key).await?;
            return Err(ApiError::Unauthorized(
                "invalid_second_factor",
                "invalid second factor",
            ));
        }
        Err(error) => return Err(error.into()),
    };

    Ok(HttpResponse::NoContent()
        .cookie(session_cookie(
            output.token.expose(),
            state.config.cookie_secure,
        ))
        .cookie(csrf_cookie(
            output.csrf_token.expose(),
            state.config.cookie_secure,
        ))
        .finish())
}

/// Verifies owner password and returns one Android bearer token.
#[post("/auth/device-login")]
pub async fn device_login(
    state: web::Data<AppState>,
    req: HttpRequest,
    body: web::Json<CreateDeviceTokenRequest>,
) -> Result<HttpResponse, ApiError> {
    let Some(pool) = state.db.as_ref() else {
        return Err(ApiError::ServiceUnavailable(
            "database_unavailable",
            "database is unavailable",
        ));
    };
    let rate_limit_key = owner_password_login_key(&req, &state);
    reject_blocked_owner_password_login(&state, pool, &rate_limit_key).await?;

    if !auth::verify_owner_password(pool, &body.password)
        .await
        .map_err(|_| ApiError::Internal)?
    {
        record_owner_password_login_failure(&state, pool, &rate_limit_key).await?;
        return Err(ApiError::Unauthorized(
            "invalid_credentials",
            "invalid credentials",
        ));
    }
    verify_login_second_factor(
        &state,
        pool,
        &rate_limit_key,
        second_factor_from_device_login(&body),
    )
    .await?;
    clear_owner_password_login_limit(&state, pool, &rate_limit_key).await?;

    let output = auth::create_device_token(
        pool,
        auth::DeviceTokenCreateInput {
            owner_id: 1,
            name: body.name.clone(),
            created_by_session_id: None,
            user_agent: user_agent(&req),
        },
    )
    .await?;

    Ok(HttpResponse::Created().json(CreateDeviceTokenResponse {
        device_token_id: output.device_token_id,
        token: output.token.expose().to_owned(),
    }))
}

/// Returns owner MFA status.
#[get("/auth/mfa")]
pub async fn mfa_status_route(
    state: web::Data<AppState>,
    req: HttpRequest,
) -> Result<HttpResponse, ApiError> {
    let Some(pool) = state.db.as_ref() else {
        return Err(ApiError::ServiceUnavailable(
            "database_unavailable",
            "database is unavailable",
        ));
    };
    require_current_session(pool, &req).await?;
    let status = auth::mfa_status(pool).await?;
    Ok(HttpResponse::Ok().json(MfaStatusResponse {
        totp_enabled: status.totp_enabled,
        totp_setup_pending: status.totp_setup_pending,
        recovery_codes_remaining: status.recovery_codes_remaining,
    }))
}

/// Starts TOTP setup and returns the pending secret.
#[post("/auth/totp/setup")]
pub async fn setup_totp_route(
    state: web::Data<AppState>,
    req: HttpRequest,
    body: web::Json<MfaPasswordRequest>,
) -> Result<HttpResponse, ApiError> {
    let Some(pool) = state.db.as_ref() else {
        return Err(ApiError::ServiceUnavailable(
            "database_unavailable",
            "database is unavailable",
        ));
    };
    let current = require_current_session(pool, &req).await?;
    require_csrf(pool, &req, current.session_id).await?;
    require_owner_password(&state, pool, &req, current.session_id, &body.password).await?;
    let setup = auth::begin_totp_setup(pool, &state.config.auth_secret).await?;
    Ok(HttpResponse::Ok().json(TotpSetupResponse {
        secret_base32: setup.secret_base32,
        provisioning_uri: setup.provisioning_uri,
    }))
}

/// Enables TOTP after verifying the pending secret and returns recovery codes.
#[post("/auth/totp/enable")]
pub async fn enable_totp_route(
    state: web::Data<AppState>,
    req: HttpRequest,
    body: web::Json<TotpEnableRequest>,
) -> Result<HttpResponse, ApiError> {
    let Some(pool) = state.db.as_ref() else {
        return Err(ApiError::ServiceUnavailable(
            "database_unavailable",
            "database is unavailable",
        ));
    };
    let current = require_current_session(pool, &req).await?;
    require_csrf(pool, &req, current.session_id).await?;
    require_owner_password(&state, pool, &req, current.session_id, &body.password).await?;
    let mfa_key = mfa_session_key(current.session_id);
    reject_blocked_owner_mfa(&state, pool, &mfa_key).await?;
    let codes = match auth::enable_totp(pool, &state.config.auth_secret, &body.totp_code).await {
        Ok(codes) => {
            clear_owner_mfa_limit(&state, pool, &mfa_key).await?;
            codes
        }
        Err(auth::MfaError::InvalidSecondFactor) => {
            record_owner_mfa_failure(&state, pool, &mfa_key).await?;
            return Err(ApiError::Unauthorized(
                "invalid_second_factor",
                "invalid second factor",
            ));
        }
        Err(error) => return Err(error.into()),
    };
    Ok(HttpResponse::Ok().json(RecoveryCodesResponse {
        recovery_codes: codes.recovery_codes,
    }))
}

/// Disables TOTP after password reauth and second-factor proof.
#[post("/auth/totp/disable")]
pub async fn disable_totp_route(
    state: web::Data<AppState>,
    req: HttpRequest,
    body: web::Json<MfaSecondFactorRequest>,
) -> Result<HttpResponse, ApiError> {
    let Some(pool) = state.db.as_ref() else {
        return Err(ApiError::ServiceUnavailable(
            "database_unavailable",
            "database is unavailable",
        ));
    };
    let current = require_current_session(pool, &req).await?;
    require_csrf(pool, &req, current.session_id).await?;
    require_owner_password(&state, pool, &req, current.session_id, &body.password).await?;
    let mfa_key = mfa_session_key(current.session_id);
    reject_blocked_owner_mfa(&state, pool, &mfa_key).await?;
    match auth::disable_totp(
        pool,
        &state.config.auth_secret,
        second_factor_from_management(&body),
    )
    .await
    {
        Ok(()) => clear_owner_mfa_limit(&state, pool, &mfa_key).await?,
        Err(auth::MfaError::SecondFactorRequired | auth::MfaError::InvalidSecondFactor) => {
            record_owner_mfa_failure(&state, pool, &mfa_key).await?;
            return Err(ApiError::Unauthorized(
                "invalid_second_factor",
                "invalid second factor",
            ));
        }
        Err(error) => return Err(error.into()),
    }
    Ok(HttpResponse::NoContent().finish())
}

/// Rotates recovery codes after password reauth and second-factor proof.
#[post("/auth/recovery-codes/rotate")]
pub async fn rotate_recovery_codes_route(
    state: web::Data<AppState>,
    req: HttpRequest,
    body: web::Json<MfaSecondFactorRequest>,
) -> Result<HttpResponse, ApiError> {
    let Some(pool) = state.db.as_ref() else {
        return Err(ApiError::ServiceUnavailable(
            "database_unavailable",
            "database is unavailable",
        ));
    };
    let current = require_current_session(pool, &req).await?;
    require_csrf(pool, &req, current.session_id).await?;
    require_owner_password(&state, pool, &req, current.session_id, &body.password).await?;
    let mfa_key = mfa_session_key(current.session_id);
    reject_blocked_owner_mfa(&state, pool, &mfa_key).await?;
    let codes = match auth::rotate_recovery_codes(
        pool,
        &state.config.auth_secret,
        second_factor_from_management(&body),
    )
    .await
    {
        Ok(codes) => {
            clear_owner_mfa_limit(&state, pool, &mfa_key).await?;
            codes
        }
        Err(auth::MfaError::SecondFactorRequired | auth::MfaError::InvalidSecondFactor) => {
            record_owner_mfa_failure(&state, pool, &mfa_key).await?;
            return Err(ApiError::Unauthorized(
                "invalid_second_factor",
                "invalid second factor",
            ));
        }
        Err(error) => return Err(error.into()),
    };
    Ok(HttpResponse::Ok().json(RecoveryCodesResponse {
        recovery_codes: codes.recovery_codes,
    }))
}

/// Revokes the current web session if a session cookie exists.
#[post("/auth/logout")]
pub async fn logout(
    state: web::Data<AppState>,
    req: HttpRequest,
) -> Result<HttpResponse, ApiError> {
    let Some(pool) = state.db.as_ref() else {
        return Err(ApiError::ServiceUnavailable(
            "database_unavailable",
            "database is unavailable",
        ));
    };

    if let Some(session) = current_session(pool, &req).await? {
        let Some(csrf_token) = csrf_header(&req) else {
            return Err(ApiError::Unauthorized(
                "csrf_required",
                "csrf token required",
            ));
        };
        if !auth::verify_session_csrf(pool, session.session_id, &csrf_token)
            .await
            .map_err(|_| ApiError::Internal)?
        {
            return Err(ApiError::Unauthorized("csrf_invalid", "invalid csrf token"));
        }
        auth::revoke_session(pool, session.session_id)
            .await
            .map_err(|_| ApiError::Internal)?;
    }

    Ok(HttpResponse::NoContent()
        .cookie(expired_session_cookie(state.config.cookie_secure))
        .cookie(expired_csrf_cookie(state.config.cookie_secure))
        .finish())
}

/// Lists active web sessions for the owner.
#[get("/sessions")]
pub async fn sessions(
    state: web::Data<AppState>,
    req: HttpRequest,
) -> Result<HttpResponse, ApiError> {
    let Some(pool) = state.db.as_ref() else {
        return Err(ApiError::ServiceUnavailable(
            "database_unavailable",
            "database is unavailable",
        ));
    };
    let Some(current) = current_session(pool, &req).await? else {
        return Err(ApiError::Unauthorized(
            "authentication_required",
            "authentication required",
        ));
    };

    let sessions = auth::list_sessions(pool, current.owner_id, current.session_id)
        .await
        .map_err(|_| ApiError::Internal)?
        .into_iter()
        .map(|session| SessionResponse {
            session_id: session.session_id,
            device_name: session.device_name,
            user_agent: session.user_agent,
            created_at: session.created_at,
            last_seen_at: session.last_seen_at,
            expires_at: session.expires_at,
            is_current: session.is_current,
        })
        .collect::<Vec<_>>();

    Ok(HttpResponse::Ok().json(sessions))
}

/// Creates an Android device token after current-session CSRF and password reauth.
#[post("/device-tokens")]
pub async fn create_device_token_route(
    state: web::Data<AppState>,
    req: HttpRequest,
    body: web::Json<CreateDeviceTokenRequest>,
) -> Result<HttpResponse, ApiError> {
    let Some(pool) = state.db.as_ref() else {
        return Err(ApiError::ServiceUnavailable(
            "database_unavailable",
            "database is unavailable",
        ));
    };
    let current = require_current_session(pool, &req).await?;
    require_csrf(pool, &req, current.session_id).await?;
    require_owner_password(&state, pool, &req, current.session_id, &body.password).await?;
    verify_login_second_factor(
        &state,
        pool,
        &mfa_session_key(current.session_id),
        second_factor_from_device_login(&body),
    )
    .await?;

    let output = auth::create_device_token(
        pool,
        auth::DeviceTokenCreateInput {
            owner_id: current.owner_id,
            name: body.name.clone(),
            created_by_session_id: Some(current.session_id),
            user_agent: user_agent(&req),
        },
    )
    .await?;

    Ok(HttpResponse::Created().json(CreateDeviceTokenResponse {
        device_token_id: output.device_token_id,
        token: output.token.expose().to_owned(),
    }))
}

/// Revokes one owner device token.
///
/// Browser sessions require CSRF. Device credentials may revoke only
/// themselves, preventing a stolen token from disabling other devices.
#[delete("/device-tokens/{device_token_id}")]
pub async fn revoke_device_token_route(
    state: web::Data<AppState>,
    req: HttpRequest,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, ApiError> {
    let Some(pool) = state.db.as_ref() else {
        return Err(ApiError::ServiceUnavailable(
            "database_unavailable",
            "database is unavailable",
        ));
    };
    let credential = require_unsafe_owner(pool, &req).await?;
    let device_token_id = path.into_inner();

    if let OwnerCredential::Device(device) = credential
        && device.device_token_id != device_token_id
    {
        return Err(ApiError::NotFound(
            "device_token_not_found",
            "device token not found",
        ));
    }

    if auth::revoke_owner_device_token(pool, credential.owner_id(), device_token_id).await? {
        Ok(HttpResponse::NoContent().finish())
    } else {
        Err(ApiError::NotFound(
            "device_token_not_found",
            "device token not found",
        ))
    }
}

/// Builds the session cookie sent after login.
pub fn session_cookie(value: &str, secure: bool) -> Cookie<'static> {
    Cookie::build(SESSION_COOKIE, value.to_owned())
        .path("/")
        .http_only(true)
        .secure(secure)
        .same_site(SameSite::Lax)
        .max_age(SESSION_MAX_AGE)
        .finish()
}

/// Builds the readable CSRF cookie paired with the server-side CSRF digest.
pub fn csrf_cookie(value: &str, secure: bool) -> Cookie<'static> {
    Cookie::build(CSRF_COOKIE, value.to_owned())
        .path("/")
        .http_only(false)
        .secure(secure)
        .same_site(SameSite::Lax)
        .max_age(SESSION_MAX_AGE)
        .finish()
}

fn expired_session_cookie(secure: bool) -> Cookie<'static> {
    Cookie::build(SESSION_COOKIE, "")
        .path("/")
        .http_only(true)
        .secure(secure)
        .same_site(SameSite::Lax)
        .max_age(Duration::ZERO)
        .finish()
}

fn expired_csrf_cookie(secure: bool) -> Cookie<'static> {
    Cookie::build(CSRF_COOKIE, "")
        .path("/")
        .http_only(false)
        .secure(secure)
        .same_site(SameSite::Lax)
        .max_age(Duration::ZERO)
        .finish()
}

fn csrf_header(req: &HttpRequest) -> Option<String> {
    req.headers()
        .get(CSRF_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
}

async fn current_session(
    pool: &sqlx::PgPool,
    req: &HttpRequest,
) -> Result<Option<AuthenticatedSession>, ApiError> {
    let Some(cookie) = req.cookie(SESSION_COOKIE) else {
        return Ok(None);
    };
    auth::authenticate_session(pool, cookie.value())
        .await
        .map_err(|_| ApiError::Internal)
}

pub(crate) async fn require_current_session(
    pool: &sqlx::PgPool,
    req: &HttpRequest,
) -> Result<AuthenticatedSession, ApiError> {
    current_session(pool, req)
        .await?
        .ok_or(ApiError::Unauthorized(
            "authentication_required",
            "authentication required",
        ))
}

/// Authenticates either browser cookie or Android bearer token.
///
/// Supplying both credential forms is rejected to avoid ambiguous policy
/// selection, especially around CSRF requirements.
pub async fn require_owner(
    pool: &sqlx::PgPool,
    req: &HttpRequest,
) -> Result<OwnerCredential, ApiError> {
    let session_token = req.cookie(SESSION_COOKIE);
    let bearer_token = bearer_token(req)?;

    if session_token.is_some() && bearer_token.is_some() {
        return Err(ApiError::Unauthorized(
            "ambiguous_credentials",
            "provide one authentication credential",
        ));
    }

    if let Some(token) = bearer_token {
        return auth::authenticate_device_token(pool, token)
            .await
            .map_err(|_| ApiError::Internal)?
            .map(OwnerCredential::Device)
            .ok_or(ApiError::Unauthorized(
                "invalid_device_token",
                "invalid device token",
            ));
    }

    if let Some(cookie) = session_token {
        return auth::authenticate_session(pool, cookie.value())
            .await
            .map_err(|_| ApiError::Internal)?
            .map(OwnerCredential::Session)
            .ok_or(ApiError::Unauthorized(
                "authentication_required",
                "authentication required",
            ));
    }

    Err(ApiError::Unauthorized(
        "authentication_required",
        "authentication required",
    ))
}

/// Authenticates owner and applies CSRF only to cookie sessions.
pub async fn require_unsafe_owner(
    pool: &sqlx::PgPool,
    req: &HttpRequest,
) -> Result<OwnerCredential, ApiError> {
    let credential = require_owner(pool, req).await?;
    if let OwnerCredential::Session(session) = credential {
        require_csrf(pool, req, session.session_id).await?;
    }
    Ok(credential)
}

pub(crate) async fn require_csrf(
    pool: &sqlx::PgPool,
    req: &HttpRequest,
    session_id: Uuid,
) -> Result<(), ApiError> {
    let Some(csrf_token) = csrf_header(req) else {
        return Err(ApiError::Unauthorized(
            "csrf_required",
            "csrf token required",
        ));
    };

    if auth::verify_session_csrf(pool, session_id, &csrf_token)
        .await
        .map_err(|_| ApiError::Internal)?
    {
        Ok(())
    } else {
        Err(ApiError::Unauthorized("csrf_invalid", "invalid csrf token"))
    }
}

fn bearer_token(req: &HttpRequest) -> Result<Option<&str>, ApiError> {
    let Some(value) = req.headers().get("authorization") else {
        return Ok(None);
    };
    let value = value.to_str().map_err(|_| {
        ApiError::Unauthorized("invalid_authorization", "invalid authorization header")
    })?;
    let Some((scheme, token)) = value.split_once(' ') else {
        return Err(ApiError::Unauthorized(
            "invalid_authorization",
            "invalid authorization header",
        ));
    };
    if !scheme.eq_ignore_ascii_case("bearer")
        || token.is_empty()
        || token.contains(char::is_whitespace)
    {
        return Err(ApiError::Unauthorized(
            "invalid_authorization",
            "invalid authorization header",
        ));
    }
    Ok(Some(token))
}

fn user_agent(req: &HttpRequest) -> Option<String> {
    req.headers()
        .get("user-agent")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
}

fn second_factor_from_login(body: &LoginRequest) -> auth::SecondFactorInput {
    auth::SecondFactorInput {
        totp_code: body.totp_code.clone(),
        recovery_code: body.recovery_code.clone(),
    }
}

fn second_factor_from_device_login(body: &CreateDeviceTokenRequest) -> auth::SecondFactorInput {
    auth::SecondFactorInput {
        totp_code: body.totp_code.clone(),
        recovery_code: body.recovery_code.clone(),
    }
}

fn second_factor_from_management(body: &MfaSecondFactorRequest) -> auth::SecondFactorInput {
    auth::SecondFactorInput {
        totp_code: body.totp_code.clone(),
        recovery_code: body.recovery_code.clone(),
    }
}

fn mfa_session_key(session_id: Uuid) -> String {
    format!("session:{session_id}")
}

async fn require_owner_password(
    state: &AppState,
    pool: &sqlx::PgPool,
    req: &HttpRequest,
    session_id: Uuid,
    password: &str,
) -> Result<(), ApiError> {
    let key = owner_password_reauth_key(req, state, session_id);
    reject_blocked_owner_password_reauth(state, pool, &key).await?;
    if auth::verify_owner_password(pool, password)
        .await
        .map_err(|_| ApiError::Internal)?
    {
        clear_owner_password_reauth_limit(state, pool, &key).await?;
        Ok(())
    } else {
        record_owner_password_reauth_failure(state, pool, &key).await?;
        Err(ApiError::Unauthorized(
            "reauth_required",
            "password reauthentication failed",
        ))
    }
}

async fn reject_blocked_owner_password_reauth(
    state: &AppState,
    pool: &sqlx::PgPool,
    key: &str,
) -> Result<(), ApiError> {
    if rate_limit::is_blocked(
        pool,
        &state.config.rate_limit_secret,
        OWNER_PASSWORD_REAUTH_ACTION,
        key,
        OffsetDateTime::now_utc(),
    )
    .await
    .map_err(|_| ApiError::Internal)?
    {
        Err(ApiError::TooManyRequests(
            "rate_limited",
            "too many password reauthentication attempts",
        ))
    } else {
        Ok(())
    }
}

async fn verify_login_second_factor(
    state: &AppState,
    pool: &sqlx::PgPool,
    key: &str,
    second_factor: auth::SecondFactorInput,
) -> Result<(), ApiError> {
    if !auth::second_factor_enabled(pool)
        .await
        .map_err(|_| ApiError::Internal)?
    {
        return Ok(());
    }
    reject_blocked_owner_mfa(state, pool, key).await?;
    match auth::verify_second_factor(pool, &state.config.auth_secret, second_factor).await {
        Ok(()) => {
            clear_owner_mfa_limit(state, pool, key).await?;
            Ok(())
        }
        Err(auth::MfaError::SecondFactorRequired) => Err(ApiError::Unauthorized(
            "second_factor_required",
            "second factor required",
        )),
        Err(auth::MfaError::InvalidSecondFactor) => {
            record_owner_mfa_failure(state, pool, key).await?;
            Err(ApiError::Unauthorized(
                "invalid_second_factor",
                "invalid second factor",
            ))
        }
        Err(error) => Err(error.into()),
    }
}

async fn reject_blocked_owner_password_login(
    state: &AppState,
    pool: &sqlx::PgPool,
    key: &str,
) -> Result<(), ApiError> {
    if rate_limit::is_blocked(
        pool,
        &state.config.rate_limit_secret,
        OWNER_PASSWORD_LOGIN_ACTION,
        key,
        OffsetDateTime::now_utc(),
    )
    .await
    .map_err(|_| ApiError::Internal)?
    {
        Err(ApiError::TooManyRequests(
            "rate_limited",
            "too many failed login attempts",
        ))
    } else {
        Ok(())
    }
}

async fn reject_blocked_owner_mfa(
    state: &AppState,
    pool: &sqlx::PgPool,
    key: &str,
) -> Result<(), ApiError> {
    if rate_limit::is_blocked(
        pool,
        &state.config.rate_limit_secret,
        OWNER_MFA_ACTION,
        key,
        OffsetDateTime::now_utc(),
    )
    .await
    .map_err(|_| ApiError::Internal)?
    {
        Err(ApiError::TooManyRequests(
            "rate_limited",
            "too many second-factor attempts",
        ))
    } else {
        Ok(())
    }
}

async fn record_owner_mfa_failure(
    state: &AppState,
    pool: &sqlx::PgPool,
    key: &str,
) -> Result<(), ApiError> {
    if rate_limit::record_failure(
        pool,
        &state.config.rate_limit_secret,
        FailureInput {
            action: OWNER_MFA_ACTION,
            key,
            now: OffsetDateTime::now_utc(),
            max_attempts: state.config.rate_limits.owner_mfa.max_per_window,
            window: state.config.rate_limits.owner_mfa.window,
            block_for: state.config.rate_limits.owner_mfa.block_for,
        },
    )
    .await
    .map_err(|_| ApiError::Internal)?
    {
        Err(ApiError::TooManyRequests(
            "rate_limited",
            "too many second-factor attempts",
        ))
    } else {
        Ok(())
    }
}

async fn clear_owner_mfa_limit(
    state: &AppState,
    pool: &sqlx::PgPool,
    key: &str,
) -> Result<(), ApiError> {
    rate_limit::clear(pool, &state.config.rate_limit_secret, OWNER_MFA_ACTION, key)
        .await
        .map_err(|_| ApiError::Internal)
}

async fn record_owner_password_reauth_failure(
    state: &AppState,
    pool: &sqlx::PgPool,
    key: &str,
) -> Result<(), ApiError> {
    if rate_limit::record_failure(
        pool,
        &state.config.rate_limit_secret,
        FailureInput {
            action: OWNER_PASSWORD_REAUTH_ACTION,
            key,
            now: OffsetDateTime::now_utc(),
            max_attempts: state.config.rate_limits.owner_password_login.max_per_window,
            window: state.config.rate_limits.owner_password_login.window,
            block_for: state.config.rate_limits.owner_password_login.block_for,
        },
    )
    .await
    .map_err(|_| ApiError::Internal)?
    {
        Err(ApiError::TooManyRequests(
            "rate_limited",
            "too many password reauthentication attempts",
        ))
    } else {
        Ok(())
    }
}

async fn clear_owner_password_reauth_limit(
    state: &AppState,
    pool: &sqlx::PgPool,
    key: &str,
) -> Result<(), ApiError> {
    rate_limit::clear(
        pool,
        &state.config.rate_limit_secret,
        OWNER_PASSWORD_REAUTH_ACTION,
        key,
    )
    .await
    .map_err(|_| ApiError::Internal)
}

async fn record_owner_password_login_failure(
    state: &AppState,
    pool: &sqlx::PgPool,
    key: &str,
) -> Result<(), ApiError> {
    if rate_limit::record_failure(
        pool,
        &state.config.rate_limit_secret,
        FailureInput {
            action: OWNER_PASSWORD_LOGIN_ACTION,
            key,
            now: OffsetDateTime::now_utc(),
            max_attempts: state.config.rate_limits.owner_password_login.max_per_window,
            window: state.config.rate_limits.owner_password_login.window,
            block_for: state.config.rate_limits.owner_password_login.block_for,
        },
    )
    .await
    .map_err(|_| ApiError::Internal)?
    {
        Err(ApiError::TooManyRequests(
            "rate_limited",
            "too many failed login attempts",
        ))
    } else {
        Ok(())
    }
}

async fn clear_owner_password_login_limit(
    state: &AppState,
    pool: &sqlx::PgPool,
    key: &str,
) -> Result<(), ApiError> {
    rate_limit::clear(
        pool,
        &state.config.rate_limit_secret,
        OWNER_PASSWORD_LOGIN_ACTION,
        key,
    )
    .await
    .map_err(|_| ApiError::Internal)
}

fn owner_password_login_key(req: &HttpRequest, state: &AppState) -> String {
    client_ip::client_ip(req, &state.config.trusted_proxies)
        .map(|ip| ip.to_string())
        .unwrap_or_else(|| "unknown-peer".to_owned())
}

fn owner_password_reauth_key(req: &HttpRequest, state: &AppState, session_id: Uuid) -> String {
    let ip = client_ip::client_ip(req, &state.config.trusted_proxies)
        .map(|ip| ip.to_string())
        .unwrap_or_else(|| "unknown-peer".to_owned());
    format!("session:{session_id}:ip:{ip}")
}
