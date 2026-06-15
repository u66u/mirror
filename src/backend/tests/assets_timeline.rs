use actix_web::{App, cookie::Cookie, http::StatusCode, test, web};
use mirror_backend::{
    assets::{ListAssetsError, ListAssetsInput, list_assets, promote_verified_upload},
    auth::{SessionCreateInput, SetupState, create_session},
    config::Config,
    http,
    media::generate_derivatives,
    state::AppState,
};
use serde_json::Value;
use uuid::Uuid;

mod support;
use support::{
    FakeImageProcessor, FakeVideoProcessor, StorageTestDeps, TestResult,
    create_verified_jpeg_upload, storage_test_deps,
};

#[tokio::test]
#[ignore = "requires MIRROR_TEST_DATABASE_URL pointing at a dedicated test database"]
async fn asset_timeline_cursor_pages_without_duplicates() -> TestResult {
    let deps = storage_test_deps().await?;
    create_promoted_asset(&deps, "first.jpg").await?;
    create_promoted_asset(&deps, "second.jpg").await?;
    create_promoted_asset(&deps, "third.jpg").await?;

    let first_page = list_assets(
        &deps.pool,
        ListAssetsInput {
            owner_id: 1,
            limit: Some(2),
            cursor: None,
        },
    )
    .await?;
    let cursor = first_page.next_cursor.clone();
    let second_page = list_assets(
        &deps.pool,
        ListAssetsInput {
            owner_id: 1,
            limit: Some(2),
            cursor,
        },
    )
    .await?;

    assert_eq!(first_page.items.len(), 2);
    assert_eq!(second_page.items.len(), 1);
    assert_ne!(first_page.items[0].asset_id, first_page.items[1].asset_id);
    assert!(
        !first_page
            .items
            .iter()
            .any(|item| item.asset_id == second_page.items[0].asset_id)
    );
    assert!(second_page.next_cursor.is_none());

    Ok(())
}

#[tokio::test]
#[ignore = "requires MIRROR_TEST_DATABASE_URL pointing at a dedicated test database"]
async fn asset_timeline_rejects_invalid_limit_and_cursor() -> TestResult {
    let deps = storage_test_deps().await?;

    let bad_limit = list_assets(
        &deps.pool,
        ListAssetsInput {
            owner_id: 1,
            limit: Some(0),
            cursor: None,
        },
    )
    .await;
    let bad_cursor = list_assets(
        &deps.pool,
        ListAssetsInput {
            owner_id: 1,
            limit: Some(10),
            cursor: Some("not-a-valid-cursor".to_owned()),
        },
    )
    .await;

    assert!(matches!(bad_limit, Err(ListAssetsError::InvalidInput)));
    assert!(matches!(bad_cursor, Err(ListAssetsError::InvalidInput)));

    Ok(())
}

#[tokio::test]
#[ignore = "requires MIRROR_TEST_DATABASE_URL pointing at a dedicated test database"]
async fn assets_route_returns_authenticated_owner_timeline() -> TestResult {
    let deps = storage_test_deps().await?;
    let asset_id = create_promoted_asset(&deps, "route.jpg").await?;
    generate_derivatives(
        &deps.pool,
        &deps.storage,
        &FakeImageProcessor,
        &FakeVideoProcessor,
        asset_id,
    )
    .await?;
    let session = create_session(
        &deps.pool,
        SessionCreateInput {
            owner_id: 1,
            user_agent: Some("assets-route-test".to_owned()),
            device_name: Some("test browser".to_owned()),
        },
    )
    .await?;
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
    let req = test::TestRequest::get()
        .uri("/assets?limit=1")
        .cookie(Cookie::new(
            "mirror_session",
            session.token.expose().to_owned(),
        ))
        .to_request();

    let response = test::call_service(&app, req).await;

    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = test::read_body_json(response).await;
    assert_eq!(body["items"].as_array().map(Vec::len), Some(1));
    assert_eq!(body["items"][0]["media_type"], "image/jpeg");
    assert_eq!(body["items"][0]["original_filename"], "route.jpg");
    assert_eq!(body["items"][0]["thumbnail"]["format"], "webp");
    assert_eq!(body["items"][0]["preview"]["width"], 1600);

    Ok(())
}

#[tokio::test]
#[ignore = "requires MIRROR_TEST_DATABASE_URL pointing at a dedicated test database"]
async fn derivative_route_returns_authenticated_derivative_bytes() -> TestResult {
    let deps = storage_test_deps().await?;
    let asset_id = create_promoted_asset(&deps, "derivative.jpg").await?;
    generate_derivatives(
        &deps.pool,
        &deps.storage,
        &FakeImageProcessor,
        &FakeVideoProcessor,
        asset_id,
    )
    .await?;
    let public_id: Uuid = sqlx::query_scalar("SELECT public_id FROM assets WHERE id = $1")
        .bind(asset_id)
        .fetch_one(&deps.pool)
        .await?;
    let session = create_session(
        &deps.pool,
        SessionCreateInput {
            owner_id: 1,
            user_agent: Some("derivative-route-test".to_owned()),
            device_name: Some("test browser".to_owned()),
        },
    )
    .await?;
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
    let req = test::TestRequest::get()
        .uri(&format!("/assets/{public_id}/derivatives/thumbnail"))
        .cookie(Cookie::new(
            "mirror_session",
            session.token.expose().to_owned(),
        ))
        .to_request();

    let response = test::call_service(&app, req).await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok()),
        Some("image/webp")
    );
    assert_eq!(test::read_body(response).await, "thumbnail");

    Ok(())
}

async fn create_promoted_asset(deps: &StorageTestDeps, filename: &str) -> TestResult<Uuid> {
    let upload_id = create_verified_jpeg_upload(&deps.pool, &deps.storage, filename).await?;
    let promoted = promote_verified_upload(&deps.pool, &deps.storage, 1, upload_id).await?;
    let internal_asset_id = sqlx::query_scalar("SELECT id FROM assets WHERE public_id = $1")
        .bind(promoted.asset_id)
        .fetch_one(&deps.pool)
        .await?;

    Ok(internal_asset_id)
}
