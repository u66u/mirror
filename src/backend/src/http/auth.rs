//! Authentication routes.
//!
//! C002/C003: handlers never log raw passwords/session tokens. Cookie security
//! currently uses local HTTPS config only; trusted-proxy handling comes later.

use actix_web::{
    HttpRequest, HttpResponse,
    cookie::{Cookie, SameSite, time::Duration},
    get, post, web,
};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    auth::{self, AuthenticatedSession, OwnerLoginInput},
    http::error::ApiError,
    state::AppState,
};

const SESSION_COOKIE: &str = "mirror_session";
const CSRF_COOKIE: &str = "mirror_csrf";
const CSRF_HEADER: &str = "x-csrf-token";
const SESSION_MAX_AGE: Duration = Duration::days(30);

/// Owner login request body.
#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    /// Owner password.
    pub password: String,
    /// Optional browser/device label.
    pub device_name: Option<String>,
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
}

/// Android device-token creation response.
#[derive(Debug, Serialize)]
pub struct CreateDeviceTokenResponse {
    /// Persisted device token ID.
    pub device_token_id: Uuid,
    /// Raw token returned once for Android encrypted storage.
    pub token: String,
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

    let output = auth::login_owner(
        pool,
        OwnerLoginInput {
            password: body.password.clone(),
            user_agent: user_agent(&req),
            device_name: body.device_name.clone(),
        },
    )
    .await?;

    Ok(HttpResponse::NoContent()
        .cookie(session_cookie(output.token.expose()))
        .cookie(csrf_cookie(output.csrf_token.expose()))
        .finish())
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
        .cookie(expired_session_cookie())
        .cookie(expired_csrf_cookie())
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

    if !auth::verify_owner_password(pool, &body.password)
        .await
        .map_err(|_| ApiError::Internal)?
    {
        return Err(ApiError::Unauthorized(
            "reauth_required",
            "password reauthentication failed",
        ));
    }

    let output = auth::create_device_token(
        pool,
        auth::DeviceTokenCreateInput {
            owner_id: current.owner_id,
            name: body.name.clone(),
            created_by_session_id: Some(current.session_id),
            user_agent: user_agent(&req),
        },
    )
    .await
    .map_err(|_| ApiError::Internal)?;

    Ok(HttpResponse::Created().json(CreateDeviceTokenResponse {
        device_token_id: output.device_token_id,
        token: output.token.expose().to_owned(),
    }))
}

/// Builds the session cookie sent after login.
pub fn session_cookie(value: &str) -> Cookie<'static> {
    Cookie::build(SESSION_COOKIE, value.to_owned())
        .path("/")
        .http_only(true)
        .same_site(SameSite::Lax)
        .max_age(SESSION_MAX_AGE)
        .finish()
}

/// Builds the readable CSRF cookie paired with the server-side CSRF digest.
pub fn csrf_cookie(value: &str) -> Cookie<'static> {
    Cookie::build(CSRF_COOKIE, value.to_owned())
        .path("/")
        .http_only(false)
        .same_site(SameSite::Lax)
        .max_age(SESSION_MAX_AGE)
        .finish()
}

fn expired_session_cookie() -> Cookie<'static> {
    Cookie::build(SESSION_COOKIE, "")
        .path("/")
        .http_only(true)
        .same_site(SameSite::Lax)
        .max_age(Duration::ZERO)
        .finish()
}

fn expired_csrf_cookie() -> Cookie<'static> {
    Cookie::build(CSRF_COOKIE, "")
        .path("/")
        .http_only(false)
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

fn user_agent(req: &HttpRequest) -> Option<String> {
    req.headers()
        .get("user-agent")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
}
