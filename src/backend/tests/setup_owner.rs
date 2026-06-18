use actix_web::{App, http::StatusCode, test, web};
use mirror_backend::{
    auth::{self, OwnerSetupError, OwnerSetupInput, SetupState, verify_password},
    config::Config,
    http,
    rate_limit::{self, QuotaInput},
    state::AppState,
};
use serde_json::json;
use time::{Duration, OffsetDateTime};

mod support;
use support::{TestResult, connect_test_database};

#[tokio::test]
#[ignore = "requires MIRROR_TEST_DATABASE_URL pointing at a dedicated test database"]
async fn owner_setup_hashes_password_and_consumes_setup_token() -> TestResult {
    let pool = connect_test_database().await?;
    sqlx::query!("TRUNCATE owner_accounts CASCADE")
        .execute(&pool)
        .await?;

    let (setup, setup_token) = SetupState::pending()?;
    let password = "correct horse battery staple";

    let output = auth::create_owner(
        &pool,
        &setup,
        OwnerSetupInput {
            setup_token: setup_token.clone(),
            display_name: "Owner".to_owned(),
            password: password.to_owned(),
        },
    )
    .await?;

    let stored_hash: String = sqlx::query_scalar!(
        "SELECT password_hash FROM owner_accounts WHERE public_id = $1",
        output.owner_public_id
    )
    .fetch_one(&pool)
    .await?;

    assert!(verify_password(password, &stored_hash));
    assert!(!stored_hash.contains(password));

    let second = auth::create_owner(
        &pool,
        &setup,
        OwnerSetupInput {
            setup_token,
            display_name: "Owner 2".to_owned(),
            password: password.to_owned(),
        },
    )
    .await;

    assert!(matches!(second, Err(OwnerSetupError::InvalidSetupToken)));

    Ok(())
}

#[tokio::test]
#[ignore = "requires MIRROR_TEST_DATABASE_URL pointing at a dedicated test database"]
async fn setup_owner_route_is_ip_rate_limited() -> TestResult {
    let pool = connect_test_database().await?;
    sqlx::query!("TRUNCATE rate_limit_buckets CASCADE")
        .execute(&pool)
        .await?;

    let (setup, _) = SetupState::pending()?;
    let mut config = Config::from_env();
    config.rate_limits.setup_owner.max_per_window = 1;
    assert!(
        rate_limit::record_quota_attempt(
            &pool,
            &config.rate_limit_secret,
            QuotaInput {
                action: "setup_owner",
                key: "unknown-peer",
                now: OffsetDateTime::now_utc(),
                max_attempts: 1,
                window: Duration::hours(1),
                block_for: Duration::hours(1),
            },
        )
        .await
        .map_err(std::io::Error::other)?
    );
    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(AppState {
                config,
                db: Some(pool.clone()),
                setup,
                storage: None,
            }))
            .configure(http::configure),
    )
    .await;

    let first = test::TestRequest::post()
        .uri("/setup/owner")
        .set_json(json!({
            "setup_token": "not-used-when-rate-limited",
            "display_name": "Owner",
            "password": "correct horse battery staple",
        }))
        .to_request();
    assert_eq!(
        test::call_service(&app, first).await.status(),
        StatusCode::TOO_MANY_REQUESTS
    );

    Ok(())
}
