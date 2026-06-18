use std::collections::BTreeSet;

use actix_web::{App, cookie::Cookie, http::StatusCode, test, web};
use mirror_backend::{
    assets::{
        ListAssetsError, ListAssetsInput, favorite_asset, list_assets, trash_asset,
        unfavorite_asset,
    },
    auth::{SessionCreateInput, SetupState, create_session},
    config::Config,
    http,
    media::generate_derivatives,
    shares::{CreateShareInput, create_share},
    state::AppState,
};
use serde_json::Value;
use uuid::Uuid;

mod support;
use support::{
    FakeImageProcessor, FakeVideoProcessor, TestResult, create_promoted_asset, storage_test_deps,
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
    let asset_id = create_promoted_asset(&deps, "route.jpg").await?.internal_id;
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
    let asset_id = create_promoted_asset(&deps, "derivative.jpg")
        .await?
        .internal_id;
    generate_derivatives(
        &deps.pool,
        &deps.storage,
        &FakeImageProcessor,
        &FakeVideoProcessor,
        asset_id,
    )
    .await?;
    let public_id: Uuid = sqlx::query_scalar!("SELECT public_id FROM assets WHERE id = $1", asset_id)
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

#[tokio::test]
#[ignore = "requires MIRROR_TEST_DATABASE_URL pointing at a dedicated test database"]
async fn favorite_routes_update_timeline_marker_idempotently() -> TestResult {
    let deps = storage_test_deps().await?;
    let asset = create_promoted_asset(&deps, "favorite.jpg").await?;
    favorite_asset(&deps.pool, 1, asset.public_id).await?;
    favorite_asset(&deps.pool, 1, asset.public_id).await?;
    let favorited = list_assets(
        &deps.pool,
        ListAssetsInput {
            owner_id: 1,
            limit: Some(10),
            cursor: None,
        },
    )
    .await?;
    assert!(favorited.items[0].favorite_at.is_some());
    unfavorite_asset(&deps.pool, 1, asset.public_id).await?;

    let session = create_session(
        &deps.pool,
        SessionCreateInput {
            owner_id: 1,
            user_agent: Some("favorite-route-test".to_owned()),
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
    let session_cookie = Cookie::new("mirror_session", session.token.expose().to_owned());

    let favorite = test::TestRequest::post()
        .uri(&format!("/assets/{}/favorite", asset.public_id))
        .cookie(session_cookie.clone())
        .insert_header(("x-csrf-token", session.csrf_token.expose()))
        .to_request();
    assert_eq!(
        test::call_service(&app, favorite).await.status(),
        StatusCode::NO_CONTENT
    );
    let timeline = test::TestRequest::get()
        .uri("/assets?limit=10")
        .cookie(session_cookie.clone())
        .to_request();
    let timeline_response = test::call_service(&app, timeline).await;
    assert_eq!(timeline_response.status(), StatusCode::OK);
    let timeline_body: Value = test::read_body_json(timeline_response).await;
    assert!(!timeline_body["items"][0]["favorite_at"].is_null());

    let unfavorite = test::TestRequest::delete()
        .uri(&format!("/assets/{}/favorite", asset.public_id))
        .cookie(session_cookie.clone())
        .insert_header(("x-csrf-token", session.csrf_token.expose()))
        .to_request();
    assert_eq!(
        test::call_service(&app, unfavorite).await.status(),
        StatusCode::NO_CONTENT
    );
    let unfavorited = test::TestRequest::get()
        .uri("/assets?limit=10")
        .cookie(session_cookie)
        .to_request();
    let unfavorited_response = test::call_service(&app, unfavorited).await;
    assert_eq!(unfavorited_response.status(), StatusCode::OK);
    let unfavorited_body: Value = test::read_body_json(unfavorited_response).await;
    assert!(unfavorited_body["items"][0]["favorite_at"].is_null());

    Ok(())
}

#[tokio::test]
#[ignore = "requires MIRROR_TEST_DATABASE_URL pointing at a dedicated test database"]
async fn trash_listing_pages_without_active_assets_or_duplicates() -> TestResult {
    let deps = storage_test_deps().await?;
    let active = create_promoted_asset(&deps, "active.jpg").await?;
    let first = create_promoted_asset(&deps, "trashed-first.jpg").await?;
    let second = create_promoted_asset(&deps, "trashed-second.jpg").await?;
    let third = create_promoted_asset(&deps, "trashed-third.jpg").await?;
    trash_asset(&deps.pool, 1, first.public_id).await?;
    trash_asset(&deps.pool, 1, second.public_id).await?;
    trash_asset(&deps.pool, 1, third.public_id).await?;
    let session = create_session(
        &deps.pool,
        SessionCreateInput {
            owner_id: 1,
            user_agent: Some("trash-list-route-test".to_owned()),
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
    let session_cookie = Cookie::new("mirror_session", session.token.expose().to_owned());

    let first_page = test::TestRequest::get()
        .uri("/trash/assets?limit=2")
        .cookie(session_cookie.clone())
        .to_request();
    let first_response = test::call_service(&app, first_page).await;
    assert_eq!(first_response.status(), StatusCode::OK);
    let first_body: Value = test::read_body_json(first_response).await;
    assert_eq!(first_body["items"].as_array().map(Vec::len), Some(2));
    let cursor = first_body["next_cursor"].as_str().unwrap_or_default();
    assert!(!cursor.is_empty());

    let second_page = test::TestRequest::get()
        .uri(&format!("/trash/assets?limit=2&cursor={cursor}"))
        .cookie(session_cookie)
        .to_request();
    let second_response = test::call_service(&app, second_page).await;
    assert_eq!(second_response.status(), StatusCode::OK);
    let second_body: Value = test::read_body_json(second_response).await;
    assert_eq!(second_body["items"].as_array().map(Vec::len), Some(1));
    assert!(second_body["next_cursor"].is_null());

    let mut listed = BTreeSet::new();
    for item in first_body["items"]
        .as_array()
        .into_iter()
        .chain(second_body["items"].as_array())
        .flatten()
    {
        assert!(!item["trashed_at"].is_null());
        listed.insert(item["asset_id"].as_str().unwrap_or_default().to_owned());
    }
    assert_eq!(listed.len(), 3);
    assert!(!listed.contains(&active.public_id.to_string()));
    assert!(listed.contains(&first.public_id.to_string()));
    assert!(listed.contains(&second.public_id.to_string()));
    assert!(listed.contains(&third.public_id.to_string()));

    Ok(())
}

#[tokio::test]
#[ignore = "requires MIRROR_TEST_DATABASE_URL pointing at a dedicated test database"]
async fn trash_route_hides_timeline_and_derivatives_until_restore() -> TestResult {
    let deps = storage_test_deps().await?;
    let asset_id = create_promoted_asset(&deps, "trash.jpg").await?.internal_id;
    generate_derivatives(
        &deps.pool,
        &deps.storage,
        &FakeImageProcessor,
        &FakeVideoProcessor,
        asset_id,
    )
    .await?;
    let public_id: Uuid = sqlx::query_scalar!("SELECT public_id FROM assets WHERE id = $1", asset_id)
        .fetch_one(&deps.pool)
        .await?;
    let session = create_session(
        &deps.pool,
        SessionCreateInput {
            owner_id: 1,
            user_agent: Some("trash-route-test".to_owned()),
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
    let session_cookie = Cookie::new("mirror_session", session.token.expose().to_owned());

    let trash = test::TestRequest::delete()
        .uri(&format!("/assets/{public_id}"))
        .cookie(session_cookie.clone())
        .insert_header(("x-csrf-token", session.csrf_token.expose()))
        .to_request();
    assert_eq!(
        test::call_service(&app, trash).await.status(),
        StatusCode::NO_CONTENT
    );

    let timeline = test::TestRequest::get()
        .uri("/assets?limit=10")
        .cookie(session_cookie.clone())
        .to_request();
    let timeline_response = test::call_service(&app, timeline).await;
    assert_eq!(timeline_response.status(), StatusCode::OK);
    let timeline_body: Value = test::read_body_json(timeline_response).await;
    assert_eq!(timeline_body["items"].as_array().map(Vec::len), Some(0));

    let derivative = test::TestRequest::get()
        .uri(&format!("/assets/{public_id}/derivatives/thumbnail"))
        .cookie(session_cookie.clone())
        .to_request();
    assert_eq!(
        test::call_service(&app, derivative).await.status(),
        StatusCode::NOT_FOUND
    );

    let restore = test::TestRequest::post()
        .uri(&format!("/assets/{public_id}/restore"))
        .cookie(session_cookie.clone())
        .insert_header(("x-csrf-token", session.csrf_token.expose()))
        .to_request();
    assert_eq!(
        test::call_service(&app, restore).await.status(),
        StatusCode::NO_CONTENT
    );

    let restored = test::TestRequest::get()
        .uri("/assets?limit=10")
        .cookie(session_cookie)
        .to_request();
    let restored_response = test::call_service(&app, restored).await;
    assert_eq!(restored_response.status(), StatusCode::OK);
    let restored_body: Value = test::read_body_json(restored_response).await;
    assert_eq!(restored_body["items"].as_array().map(Vec::len), Some(1));

    Ok(())
}

#[tokio::test]
#[ignore = "requires MIRROR_TEST_DATABASE_URL pointing at a dedicated test database"]
async fn purge_route_requires_trash_and_audits_permanent_removal() -> TestResult {
    let deps = storage_test_deps().await?;
    let asset = create_promoted_asset(&deps, "purge.jpg").await?;
    create_share(
        &deps.pool,
        CreateShareInput {
            owner_id: 1,
            asset_public_id: asset.public_id,
            expires_in_seconds: Some(3600),
            allow_original_download: false,
        },
    )
    .await?;
    let session = create_session(
        &deps.pool,
        SessionCreateInput {
            owner_id: 1,
            user_agent: Some("purge-route-test".to_owned()),
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
    let session_cookie = Cookie::new("mirror_session", session.token.expose().to_owned());

    let active_purge = test::TestRequest::delete()
        .uri(&format!("/assets/{}/purge", asset.public_id))
        .cookie(session_cookie.clone())
        .insert_header(("x-csrf-token", session.csrf_token.expose()))
        .to_request();
    assert_eq!(
        test::call_service(&app, active_purge).await.status(),
        StatusCode::CONFLICT
    );

    let trash = test::TestRequest::delete()
        .uri(&format!("/assets/{}", asset.public_id))
        .cookie(session_cookie.clone())
        .insert_header(("x-csrf-token", session.csrf_token.expose()))
        .to_request();
    assert_eq!(
        test::call_service(&app, trash).await.status(),
        StatusCode::NO_CONTENT
    );

    let purge = test::TestRequest::delete()
        .uri(&format!("/assets/{}/purge", asset.public_id))
        .cookie(session_cookie)
        .insert_header(("x-csrf-token", session.csrf_token.expose()))
        .to_request();
    assert_eq!(
        test::call_service(&app, purge).await.status(),
        StatusCode::NO_CONTENT
    );

    let asset_count: i64 = sqlx::query_scalar!(r#"SELECT count(*) as "count!" FROM assets"#)
        .fetch_one(&deps.pool)
        .await?;
    let original_count: i64 = sqlx::query_scalar!(r#"SELECT count(*) as "count!" FROM originals"#)
        .fetch_one(&deps.pool)
        .await?;
    let share_count: i64 = sqlx::query_scalar!(r#"SELECT count(*) as "count!" FROM asset_shares"#)
        .fetch_one(&deps.pool)
        .await?;
    let public_id_str = asset.public_id.to_string();
    let audit_count: i64 = sqlx::query_scalar!(
        r#"
        SELECT count(*) as "count!"
        FROM audit_events
        WHERE action = 'asset.purge'
          AND outcome = 'success'
          AND target_id = $1
          AND metadata->>'original_removed' = 'true'
        "#,
        public_id_str,
    )
    .fetch_one(&deps.pool)
    .await?;

    assert_eq!(asset_count, 0);
    assert_eq!(original_count, 0);
    assert_eq!(share_count, 0);
    assert_eq!(audit_count, 1);

    Ok(())
}
