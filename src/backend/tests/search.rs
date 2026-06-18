use std::{num::NonZeroUsize, sync::Arc};

use actix_web::{App, cookie::Cookie, http::StatusCode, test, web};
use mirror_backend::{
    assets::trash_asset,
    auth::{SessionCreateInput, SetupState, create_session},
    config::Config,
    http,
    ml::{ImageTextEmbedder, SharedImageTextRuntime},
    models::{
        activate_model_pack, install_model_pack, record_model_pack_self_test,
        validate_embedding_output,
    },
    search::{SearchAssetsInput, SearchError, search_assets},
    state::AppState,
};
use serde_json::Value;
use uuid::Uuid;

mod support;
use support::{
    FakeImageTextEmbedder, TestResult, create_promoted_asset, storage_test_deps,
    valid_model_pack_manifest,
};

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

#[tokio::test]
#[ignore = "requires MIRROR_TEST_DATABASE_URL pointing at a dedicated test database"]
async fn search_route_semantic_mode_uses_active_model_pack() -> TestResult {
    let deps = storage_test_deps().await?;
    let manifest = valid_model_pack_manifest();
    let pack = install_model_pack(&deps.pool, manifest.clone()).await?;
    record_model_pack_self_test(&deps.pool, pack.model_pack_id, true, None).await?;
    activate_model_pack(&deps.pool, pack.model_pack_id).await?;
    let asset = create_promoted_asset(&deps, "semantic-route.jpg").await?;
    create_promoted_asset(&deps, "not-indexed.jpg").await?;
    let internal_id = sqlx::query_scalar!(
        "SELECT id FROM assets WHERE public_id = $1",
        asset.public_id
    )
    .fetch_one(&deps.pool)
    .await?;
    let mut values = vec![0.0; manifest.embedding_dimension as usize];
    values[0] = 1.0;
    mirror_backend::semantic_index::upsert_asset_embedding(
        &deps.pool,
        internal_id,
        pack.model_pack_id,
        &validate_embedding_output(&manifest, values)?,
    )
    .await?;
    let session = create_session(
        &deps.pool,
        SessionCreateInput {
            owner_id: 1,
            user_agent: Some("semantic-search-route-test".to_owned()),
            device_name: Some("test browser".to_owned()),
        },
    )
    .await?;
    let embedder: Arc<dyn ImageTextEmbedder + Send + Sync> = Arc::new(FakeImageTextEmbedder);
    let runtime = SharedImageTextRuntime::new(embedder, NonZeroUsize::MIN);
    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(runtime))
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
        .uri("/search?mode=semantic&q=cat&limit=10")
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
    assert_eq!(body["items"][0]["original_filename"], "semantic-route.jpg");
    Ok(())
}
