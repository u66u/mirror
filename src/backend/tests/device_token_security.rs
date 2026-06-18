use actix_web::{App, cookie::Cookie, http::StatusCode, test, web};
use mirror_backend::auth::{
    self, DeviceTokenCreateInput, SessionCreateInput, authenticate_device_token,
    create_device_token, create_session, revoke_device_token,
};
use mirror_backend::{config::Config, http, state::AppState};
use serde_json::{Value, json};

mod support;
use support::{TestResult, fresh_owner_pool, required_json_string};

#[tokio::test]
#[ignore = "requires MIRROR_TEST_DATABASE_URL pointing at a dedicated test database"]
async fn android_login_token_authorizes_then_stops_after_self_revocation() -> TestResult {
    let pool = fresh_owner_pool().await?;
    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(AppState {
                config: Config::from_env(),
                db: Some(pool.clone()),
                setup: auth::SetupState::Disabled,
                storage: None,
            }))
            .configure(http::configure),
    )
    .await;

    let wrong_login = test::TestRequest::post()
        .uri("/auth/device-login")
        .set_json(json!({
            "name": "Pixel",
            "password": "wrong horse battery staple"
        }))
        .to_request();
    let wrong_response = test::call_service(&app, wrong_login).await;
    assert_eq!(wrong_response.status(), StatusCode::UNAUTHORIZED);
    let token_count: i64 = sqlx::query_scalar!(r#"SELECT count(*) as "count!" FROM device_tokens"#)
        .fetch_one(&pool)
        .await?;
    assert_eq!(token_count, 0);

    let login = test::TestRequest::post()
        .uri("/auth/device-login")
        .set_json(json!({
            "name": "Pixel",
            "password": "correct horse battery staple"
        }))
        .to_request();
    let login_response = test::call_service(&app, login).await;
    assert_eq!(login_response.status(), StatusCode::CREATED);
    let body: Value = test::read_body_json(login_response).await;
    let device_token_id = required_json_string(&body, "device_token_id")?;
    let token = required_json_string(&body, "token")?;
    let authorization = format!("Bearer {token}");

    let timeline = test::TestRequest::get()
        .uri("/assets")
        .insert_header(("authorization", authorization.as_str()))
        .to_request();
    assert_eq!(
        test::call_service(&app, timeline).await.status(),
        StatusCode::OK
    );

    let session = create_session(
        &pool,
        SessionCreateInput {
            owner_id: 1,
            user_agent: Some("ambiguous-credential-test".to_owned()),
            device_name: Some("browser".to_owned()),
        },
    )
    .await?;
    let ambiguous = test::TestRequest::get()
        .uri("/assets")
        .insert_header(("authorization", authorization.as_str()))
        .cookie(Cookie::new(
            "mirror_session",
            session.token.expose().to_owned(),
        ))
        .to_request();
    assert_eq!(
        test::call_service(&app, ambiguous).await.status(),
        StatusCode::UNAUTHORIZED
    );

    let revoke = test::TestRequest::delete()
        .uri(&format!("/device-tokens/{device_token_id}"))
        .insert_header(("authorization", authorization.as_str()))
        .to_request();
    assert_eq!(
        test::call_service(&app, revoke).await.status(),
        StatusCode::NO_CONTENT
    );

    let rejected = test::TestRequest::get()
        .uri("/assets")
        .insert_header(("authorization", authorization))
        .to_request();
    assert_eq!(
        test::call_service(&app, rejected).await.status(),
        StatusCode::UNAUTHORIZED
    );

    Ok(())
}

#[tokio::test]
#[ignore = "requires MIRROR_TEST_DATABASE_URL pointing at a dedicated test database"]
async fn repeated_owner_password_failures_rate_limit_device_login() -> TestResult {
    let pool = fresh_owner_pool().await?;
    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(AppState {
                config: Config::from_env(),
                db: Some(pool.clone()),
                setup: auth::SetupState::Disabled,
                storage: None,
            }))
            .configure(http::configure),
    )
    .await;

    for attempt in 1..=5 {
        let request = test::TestRequest::post()
            .uri("/auth/device-login")
            .insert_header(("x-forwarded-for", format!("198.51.100.{attempt}")))
            .set_json(json!({
                "name": "Pixel",
                "password": "wrong horse battery staple"
            }))
            .to_request();
        let status = test::call_service(&app, request).await.status();
        if attempt < 5 {
            assert_eq!(status, StatusCode::UNAUTHORIZED);
        } else {
            assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
        }
    }

    let correct_password = test::TestRequest::post()
        .uri("/auth/device-login")
        .insert_header(("x-forwarded-for", "198.51.100.250"))
        .set_json(json!({
            "name": "Pixel",
            "password": "correct horse battery staple"
        }))
        .to_request();
    assert_eq!(
        test::call_service(&app, correct_password).await.status(),
        StatusCode::TOO_MANY_REQUESTS
    );
    let token_count: i64 = sqlx::query_scalar!(r#"SELECT count(*) as "count!" FROM device_tokens"#)
        .fetch_one(&pool)
        .await?;
    assert_eq!(token_count, 0);

    Ok(())
}

#[tokio::test]
#[ignore = "requires MIRROR_TEST_DATABASE_URL pointing at a dedicated test database"]
async fn device_token_can_be_tied_to_creating_session_after_password_check() -> TestResult {
    let pool = fresh_owner_pool().await?;
    assert!(!auth::verify_owner_password(&pool, "wrong horse battery staple").await?);
    assert!(auth::verify_owner_password(&pool, "correct horse battery staple").await?);

    let session = create_session(
        &pool,
        SessionCreateInput {
            owner_id: 1,
            user_agent: Some("device-token-creator".to_owned()),
            device_name: Some("browser".to_owned()),
        },
    )
    .await?;
    let device = create_device_token(
        &pool,
        DeviceTokenCreateInput {
            owner_id: 1,
            name: "Pixel".to_owned(),
            created_by_session_id: Some(session.session_id),
            user_agent: None,
        },
    )
    .await?;

    let created_by: Option<uuid::Uuid> =
        sqlx::query_scalar("SELECT created_by_session_id FROM device_tokens WHERE id = $1")
            .bind(device.device_token_id)
            .fetch_one(&pool)
            .await?;

    assert_eq!(created_by, Some(session.session_id));

    Ok(())
}

#[tokio::test]
#[ignore = "requires MIRROR_TEST_DATABASE_URL pointing at a dedicated test database"]
async fn device_token_authenticates_by_hash_and_rejects_wrong_or_revoked_token() -> TestResult {
    let pool = fresh_owner_pool().await?;

    let created = create_device_token(
        &pool,
        DeviceTokenCreateInput {
            owner_id: 1,
            name: "Pixel".to_owned(),
            created_by_session_id: None,
            user_agent: Some("device-token-test".to_owned()),
        },
    )
    .await?;

    let raw_token = created.token.expose().to_owned();
    let stored_hash: Vec<u8> =
        sqlx::query_scalar("SELECT token_hash FROM device_tokens WHERE id = $1")
            .bind(created.device_token_id)
            .fetch_one(&pool)
            .await?;

    assert_eq!(stored_hash.len(), 32);
    assert_ne!(stored_hash, raw_token.as_bytes());
    assert_eq!(
        authenticate_device_token(&pool, &raw_token)
            .await?
            .map(|token| token.device_token_id),
        Some(created.device_token_id)
    );
    assert!(
        authenticate_device_token(&pool, "wrong-token")
            .await?
            .is_none()
    );

    revoke_device_token(&pool, created.device_token_id).await?;

    assert!(
        authenticate_device_token(&pool, &raw_token)
            .await?
            .is_none()
    );

    Ok(())
}
