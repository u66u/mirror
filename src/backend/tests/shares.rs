use actix_web::{App, http::StatusCode, test, web};
use mirror_backend::{
    auth::{DeviceTokenCreateInput, SetupState, TokenHash, create_device_token},
    config::Config,
    http,
    media::generate_derivatives,
    state::AppState,
};
use serde_json::{Value, json};

mod support;
use support::{
    FakeImageProcessor, FakeVideoProcessor, TestResult, create_promoted_asset,
    required_json_string, storage_test_deps,
};

#[tokio::test]
#[ignore = "requires MIRROR_TEST_DATABASE_URL pointing at a dedicated test database"]
async fn share_route_hashes_token_serves_derivative_and_revokes() -> TestResult {
    let deps = storage_test_deps().await?;
    let asset = create_promoted_asset(&deps, "share.jpg").await?;
    generate_derivatives(
        &deps.pool,
        &deps.storage,
        &FakeImageProcessor,
        &FakeVideoProcessor,
        asset.internal_id,
    )
    .await?;
    let device = create_device_token(
        &deps.pool,
        DeviceTokenCreateInput {
            owner_id: 1,
            name: "share-test".to_owned(),
            created_by_session_id: None,
            user_agent: Some("share-route-test".to_owned()),
        },
    )
    .await?;
    let authorization = format!("Bearer {}", device.token.expose());
    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(AppState {
                config: Config::from_env(),
                db: Some(deps.pool.clone()),
                setup: SetupState::Disabled,
                storage: Some(deps.storage.clone()),
            }))
            .configure(http::configure),
    )
    .await;

    let create = test::TestRequest::post()
        .uri(&format!("/assets/{}/shares", asset.public_id))
        .insert_header(("authorization", authorization.as_str()))
        .set_json(json!({
            "expires_in_seconds": 3600,
            "allow_original_download": false
        }))
        .to_request();
    let create_response = test::call_service(&app, create).await;
    assert_eq!(create_response.status(), StatusCode::CREATED);
    let created: Value = test::read_body_json(create_response).await;
    let share_id = required_json_string(&created, "share_id")?;
    let token = required_json_string(&created, "token")?;

    let stored_hash: Vec<u8> =
        sqlx::query_scalar("SELECT token_hash FROM asset_shares WHERE public_id::text = $1")
            .bind(&share_id)
            .fetch_one(&deps.pool)
            .await?;
    assert_eq!(
        stored_hash,
        TokenHash::from_raw(&token).as_bytes().as_slice()
    );
    assert_ne!(stored_hash, token.as_bytes());

    let metadata = test::TestRequest::get()
        .uri(&format!("/shares/{token}"))
        .to_request();
    let metadata_response = test::call_service(&app, metadata).await;
    assert_eq!(metadata_response.status(), StatusCode::OK);
    assert_privacy_headers(&metadata_response);
    let body: Value = test::read_body_json(metadata_response).await;
    let asset_public_id = asset.public_id.to_string();
    assert_eq!(body["asset_id"].as_str(), Some(asset_public_id.as_str()));
    assert_eq!(body["media_type"], "image/jpeg");
    assert_eq!(body["thumbnail"]["format"], "webp");
    assert!(body.get("original_filename").is_none());
    assert!(body.get("owner_metadata").is_none());

    let derivative = test::TestRequest::get()
        .uri(&format!("/shares/{token}/derivatives/thumbnail"))
        .to_request();
    let derivative_response = test::call_service(&app, derivative).await;
    assert_eq!(derivative_response.status(), StatusCode::OK);
    assert_privacy_headers(&derivative_response);
    assert_eq!(
        derivative_response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok()),
        Some("image/webp")
    );
    assert_eq!(test::read_body(derivative_response).await, "thumbnail");

    sqlx::query("UPDATE assets SET trashed_at = now() WHERE public_id = $1")
        .bind(asset.public_id)
        .execute(&deps.pool)
        .await?;
    let after_trash = test::TestRequest::get()
        .uri(&format!("/shares/{token}"))
        .to_request();
    assert_eq!(
        test::call_service(&app, after_trash).await.status(),
        StatusCode::NOT_FOUND
    );

    let revoke = test::TestRequest::delete()
        .uri(&format!("/shares/{share_id}"))
        .insert_header(("authorization", authorization))
        .to_request();
    assert_eq!(
        test::call_service(&app, revoke).await.status(),
        StatusCode::NO_CONTENT
    );

    let after_revoke = test::TestRequest::get()
        .uri(&format!("/shares/{token}"))
        .to_request();
    assert_eq!(
        test::call_service(&app, after_revoke).await.status(),
        StatusCode::NOT_FOUND
    );

    Ok(())
}

#[tokio::test]
#[ignore = "requires MIRROR_TEST_DATABASE_URL pointing at a dedicated test database"]
async fn share_creation_is_owner_rate_limited_before_token_minting() -> TestResult {
    let deps = storage_test_deps().await?;
    let asset = create_promoted_asset(&deps, "share-rate-limit.jpg").await?;
    let device = create_device_token(
        &deps.pool,
        DeviceTokenCreateInput {
            owner_id: 1,
            name: "share-rate-limit-test".to_owned(),
            created_by_session_id: None,
            user_agent: Some("share-rate-limit-test".to_owned()),
        },
    )
    .await?;
    let authorization = format!("Bearer {}", device.token.expose());
    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(AppState {
                config: Config::from_env(),
                db: Some(deps.pool.clone()),
                setup: SetupState::Disabled,
                storage: Some(deps.storage.clone()),
            }))
            .configure(http::configure),
    )
    .await;

    for attempt in 1..=21 {
        let request = test::TestRequest::post()
            .uri(&format!("/assets/{}/shares", asset.public_id))
            .insert_header(("authorization", authorization.as_str()))
            .insert_header(("x-forwarded-for", format!("198.51.100.{attempt}")))
            .set_json(json!({ "expires_in_seconds": 3600 }))
            .to_request();
        let status = test::call_service(&app, request).await.status();
        if attempt <= 20 {
            assert_eq!(status, StatusCode::CREATED);
        } else {
            assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
        }
    }

    let share_count: i64 = sqlx::query_scalar("SELECT count(*) FROM asset_shares")
        .fetch_one(&deps.pool)
        .await?;
    assert_eq!(share_count, 20);

    Ok(())
}

fn assert_privacy_headers<B>(response: &actix_web::dev::ServiceResponse<B>) {
    assert_eq!(
        response
            .headers()
            .get("cache-control")
            .and_then(|value| value.to_str().ok()),
        Some("private, no-store")
    );
    assert_eq!(
        response
            .headers()
            .get("referrer-policy")
            .and_then(|value| value.to_str().ok()),
        Some("no-referrer")
    );
    assert_eq!(
        response
            .headers()
            .get("x-content-type-options")
            .and_then(|value| value.to_str().ok()),
        Some("nosniff")
    );
    assert_eq!(
        response
            .headers()
            .get("x-robots-tag")
            .and_then(|value| value.to_str().ok()),
        Some("noindex")
    );
}
