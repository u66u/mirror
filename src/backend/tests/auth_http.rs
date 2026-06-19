use actix_web::{App, cookie::SameSite, http::StatusCode, test as actix_test, web};
use mirror_backend::{
    auth::{self, SessionCreateInput},
    config::{AuthSecret, Config},
    http,
    http::auth::{csrf_cookie, session_cookie},
    state::AppState,
};
use serde_json::{Value, json};

mod support;
use support::{TestResult, current_totp_code, fresh_owner_pool, required_json_string};

#[test]
fn session_cookie_has_csrf_and_script_theft_defense_flags() {
    let cookie = session_cookie("opaque-session-token", true);

    assert_eq!(cookie.name(), "mirror_session");
    assert_eq!(cookie.path(), Some("/"));
    assert_eq!(cookie.http_only(), Some(true));
    assert_eq!(cookie.secure(), Some(true));
    assert_eq!(cookie.same_site(), Some(SameSite::Lax));
    assert!(cookie.max_age().is_some());
}

#[test]
fn csrf_cookie_is_readable_by_client_code_but_same_site_lax() {
    let cookie = csrf_cookie("opaque-csrf-token", true);

    assert_eq!(cookie.name(), "mirror_csrf");
    assert_eq!(cookie.path(), Some("/"));
    assert_eq!(cookie.http_only(), Some(false));
    assert_eq!(cookie.secure(), Some(true));
    assert_eq!(cookie.same_site(), Some(SameSite::Lax));
    assert!(cookie.max_age().is_some());
}

#[test]
fn auth_cookie_secure_flag_can_be_disabled_for_local_http() {
    assert_eq!(session_cookie("token", false).secure(), Some(false));
    assert_eq!(csrf_cookie("csrf", false).secure(), Some(false));
}

#[tokio::test]
#[ignore = "requires MIRROR_TEST_DATABASE_URL pointing at a dedicated test database"]
async fn totp_routes_enable_login_and_recovery_code_device_tokens() -> TestResult {
    let pool = fresh_owner_pool().await?;
    let mut config = Config::from_env();
    config.auth_secret = AuthSecret::from_secret("route-auth-secret");
    config.auth_secret_configured = true;
    let session = auth::create_session(
        &pool,
        SessionCreateInput {
            owner_id: 1,
            user_agent: Some("mfa-route-test".to_owned()),
            device_name: Some("browser".to_owned()),
        },
    )
    .await?;
    let app = actix_test::init_service(
        App::new()
            .app_data(web::Data::new(AppState {
                config,
                db: Some(pool.clone()),
                setup: auth::SetupState::Disabled,
                storage: None,
            }))
            .configure(http::configure),
    )
    .await;

    let status_request = actix_test::TestRequest::get()
        .uri("/auth/mfa")
        .cookie(actix_web::cookie::Cookie::new(
            "mirror_session",
            session.token.expose().to_owned(),
        ))
        .to_request();
    let status_response = actix_test::call_service(&app, status_request).await;
    assert_eq!(status_response.status(), StatusCode::OK);
    let status_body: Value = actix_test::read_body_json(status_response).await;
    assert_eq!(status_body["totp_enabled"], false);

    let setup_request = actix_test::TestRequest::post()
        .uri("/auth/totp/setup")
        .cookie(actix_web::cookie::Cookie::new(
            "mirror_session",
            session.token.expose().to_owned(),
        ))
        .insert_header(("x-csrf-token", session.csrf_token.expose()))
        .set_json(json!({ "password": "correct horse battery staple" }))
        .to_request();
    let setup_response = actix_test::call_service(&app, setup_request).await;
    assert_eq!(setup_response.status(), StatusCode::OK);
    let setup_body: Value = actix_test::read_body_json(setup_response).await;
    let secret = required_json_string(&setup_body, "secret_base32")?;
    assert!(required_json_string(&setup_body, "provisioning_uri")?.starts_with("otpauth://totp/"));

    let enable_request = actix_test::TestRequest::post()
        .uri("/auth/totp/enable")
        .cookie(actix_web::cookie::Cookie::new(
            "mirror_session",
            session.token.expose().to_owned(),
        ))
        .insert_header(("x-csrf-token", session.csrf_token.expose()))
        .set_json(json!({
            "password": "correct horse battery staple",
            "totp_code": current_totp_code(&secret)?,
        }))
        .to_request();
    let enable_response = actix_test::call_service(&app, enable_request).await;
    assert_eq!(enable_response.status(), StatusCode::OK);
    let enable_body: Value = actix_test::read_body_json(enable_response).await;
    let recovery_codes = enable_body["recovery_codes"]
        .as_array()
        .ok_or_else(|| std::io::Error::other("recovery_codes missing"))?
        .iter()
        .map(|code| {
            code.as_str()
                .map(str::to_owned)
                .ok_or_else(|| std::io::Error::other("recovery code not string").into())
        })
        .collect::<TestResult<Vec<String>>>()?;
    assert_eq!(recovery_codes.len(), 10);

    let password_only = actix_test::TestRequest::post()
        .uri("/auth/login")
        .set_json(json!({
            "password": "correct horse battery staple",
            "device_name": "browser"
        }))
        .to_request();
    assert_eq!(
        actix_test::call_service(&app, password_only).await.status(),
        StatusCode::UNAUTHORIZED
    );

    let totp_login = actix_test::TestRequest::post()
        .uri("/auth/login")
        .set_json(json!({
            "password": "correct horse battery staple",
            "totp_code": current_totp_code(&secret)?,
            "device_name": "browser"
        }))
        .to_request();
    assert_eq!(
        actix_test::call_service(&app, totp_login).await.status(),
        StatusCode::NO_CONTENT
    );

    let device_without_factor = actix_test::TestRequest::post()
        .uri("/device-tokens")
        .cookie(actix_web::cookie::Cookie::new(
            "mirror_session",
            session.token.expose().to_owned(),
        ))
        .insert_header(("x-csrf-token", session.csrf_token.expose()))
        .set_json(json!({
            "name": "Pixel",
            "password": "correct horse battery staple"
        }))
        .to_request();
    assert_eq!(
        actix_test::call_service(&app, device_without_factor)
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );

    let device_with_recovery = actix_test::TestRequest::post()
        .uri("/device-tokens")
        .cookie(actix_web::cookie::Cookie::new(
            "mirror_session",
            session.token.expose().to_owned(),
        ))
        .insert_header(("x-csrf-token", session.csrf_token.expose()))
        .set_json(json!({
            "name": "Pixel",
            "password": "correct horse battery staple",
            "recovery_code": recovery_codes[0]
        }))
        .to_request();
    let device_response = actix_test::call_service(&app, device_with_recovery).await;
    assert_eq!(device_response.status(), StatusCode::CREATED);
    let device_body: Value = actix_test::read_body_json(device_response).await;
    assert!(device_body["token"].as_str().is_some());

    Ok(())
}

#[tokio::test]
#[ignore = "requires MIRROR_TEST_DATABASE_URL pointing at a dedicated test database"]
async fn totp_password_reauth_failures_are_rate_limited() -> TestResult {
    let pool = fresh_owner_pool().await?;
    let mut config = Config::from_env();
    config.auth_secret = AuthSecret::from_secret("route-auth-secret");
    config.auth_secret_configured = true;
    config.rate_limits.owner_password_login.max_per_window = 2;
    let session = auth::create_session(
        &pool,
        SessionCreateInput {
            owner_id: 1,
            user_agent: Some("reauth-rate-limit-test".to_owned()),
            device_name: Some("browser".to_owned()),
        },
    )
    .await?;
    let app = actix_test::init_service(
        App::new()
            .app_data(web::Data::new(AppState {
                config,
                db: Some(pool.clone()),
                setup: auth::SetupState::Disabled,
                storage: None,
            }))
            .configure(http::configure),
    )
    .await;

    let first = actix_test::TestRequest::post()
        .uri("/auth/totp/setup")
        .cookie(actix_web::cookie::Cookie::new(
            "mirror_session",
            session.token.expose().to_owned(),
        ))
        .insert_header(("x-csrf-token", session.csrf_token.expose()))
        .set_json(json!({ "password": "wrong password" }))
        .to_request();
    assert_eq!(
        actix_test::call_service(&app, first).await.status(),
        StatusCode::UNAUTHORIZED
    );

    let second = actix_test::TestRequest::post()
        .uri("/auth/totp/setup")
        .cookie(actix_web::cookie::Cookie::new(
            "mirror_session",
            session.token.expose().to_owned(),
        ))
        .insert_header(("x-csrf-token", session.csrf_token.expose()))
        .set_json(json!({ "password": "wrong password" }))
        .to_request();
    assert_eq!(
        actix_test::call_service(&app, second).await.status(),
        StatusCode::TOO_MANY_REQUESTS
    );

    let blocked = actix_test::TestRequest::post()
        .uri("/auth/totp/setup")
        .cookie(actix_web::cookie::Cookie::new(
            "mirror_session",
            session.token.expose().to_owned(),
        ))
        .insert_header(("x-csrf-token", session.csrf_token.expose()))
        .set_json(json!({ "password": "wrong password" }))
        .to_request();
    assert_eq!(
        actix_test::call_service(&app, blocked).await.status(),
        StatusCode::TOO_MANY_REQUESTS
    );

    Ok(())
}
