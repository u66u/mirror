use actix_web::{App, cookie::Cookie, http::StatusCode, test, web};
use mirror_backend::{
    assets::trash_asset,
    auth::{SessionCreateInput, SetupState, create_session},
    config::Config,
    http,
    search::{SearchAssetsInput, SearchError, search_assets},
    state::AppState,
};
use serde_json::Value;

mod support;
use support::{TestResult, create_promoted_asset, storage_test_deps};

#[tokio::test]
#[ignore = "requires MIRROR_TEST_DATABASE_URL pointing at a dedicated test database"]
async fn filename_search_returns_active_matches_without_wildcard_expansion() -> TestResult {
    let deps = storage_test_deps().await?;
    let percent = create_promoted_asset(&deps, "100%percent.jpg").await?;
    create_promoted_asset(&deps, "plain.jpg").await?;
    let trashed = create_promoted_asset(&deps, "trashed_percent.jpg").await?;
    trash_asset(&deps.pool, 1, trashed.public_id).await?;

    let page = search_assets(
        &deps.pool,
        SearchAssetsInput {
            owner_id: 1,
            query: "%".to_owned(),
            limit: Some(10),
        },
    )
    .await?;

    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].asset_id, percent.public_id);
    assert_eq!(
        page.items[0].original_filename.as_deref(),
        Some("100%percent.jpg")
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires MIRROR_TEST_DATABASE_URL pointing at a dedicated test database"]
async fn filename_search_rejects_empty_and_bad_limits() -> TestResult {
    let deps = storage_test_deps().await?;

    let empty = search_assets(
        &deps.pool,
        SearchAssetsInput {
            owner_id: 1,
            query: "  ".to_owned(),
            limit: Some(10),
        },
    )
    .await;
    let bad_limit = search_assets(
        &deps.pool,
        SearchAssetsInput {
            owner_id: 1,
            query: "photo".to_owned(),
            limit: Some(0),
        },
    )
    .await;

    assert!(matches!(empty, Err(SearchError::InvalidInput)));
    assert!(matches!(bad_limit, Err(SearchError::InvalidInput)));
    Ok(())
}

#[tokio::test]
#[ignore = "requires MIRROR_TEST_DATABASE_URL pointing at a dedicated test database"]
async fn search_route_returns_authenticated_owner_results() -> TestResult {
    let deps = storage_test_deps().await?;
    let asset = create_promoted_asset(&deps, "route-search.jpg").await?;
    create_promoted_asset(&deps, "other.jpg").await?;
    let session = create_session(
        &deps.pool,
        SessionCreateInput {
            owner_id: 1,
            user_agent: Some("search-route-test".to_owned()),
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
        .uri("/search?q=route&limit=10")
        .cookie(Cookie::new(
            "mirror_session",
            session.token.expose().to_owned(),
        ))
        .to_request();

    let response = test::call_service(&app, req).await;

    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = test::read_body_json(response).await;
    assert_eq!(body["items"].as_array().map(Vec::len), Some(1));
    assert_eq!(body["items"][0]["asset_id"], asset.public_id.to_string());
    assert_eq!(body["items"][0]["original_filename"], "route-search.jpg");
    Ok(())
}
