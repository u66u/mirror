use actix_web::{App, cookie::Cookie, http::StatusCode, test, web};
use mirror_backend::{
    auth::{
        DeviceTokenCreateInput, SessionCreateInput, SetupState, create_device_token, create_session,
    },
    config::Config,
    http,
    rate_limit::{self, QuotaInput},
    state::AppState,
    storage::StorageKey,
    uploads::{CreateUploadInput, UPLOAD_PART_SIZE_BYTES, create_upload, put_part},
};
use serde_json::{Value, json};
use time::{Duration, OffsetDateTime};

mod support;
use support::{TestResult, asset_count, job_count, jpeg_bytes, storage_test_deps};

#[tokio::test]
#[ignore = "requires MIRROR_TEST_DATABASE_URL pointing at a dedicated test database"]
async fn upload_part_route_accepts_four_mib_and_rejects_larger_without_artifacts() -> TestResult {
    let deps = storage_test_deps().await?;
    let device = create_device_token(
        &deps.pool,
        DeviceTokenCreateInput {
            owner_id: 1,
            name: "upload boundary test".to_owned(),
            created_by_session_id: None,
            user_agent: Some("upload-http-test".to_owned()),
        },
    )
    .await?;
    let authorization = format!("Bearer {}", device.token.expose());

    let mut exact_bytes = vec![0_u8; UPLOAD_PART_SIZE_BYTES];
    exact_bytes[..3].copy_from_slice(&[0xff, 0xd8, 0xff]);
    let exact_upload = create_upload(
        &deps.pool,
        CreateUploadInput {
            owner_id: 1,
            original_filename: "exact-part.jpg".to_owned(),
            expected_size: i64::try_from(exact_bytes.len())?,
            expected_blake3: blake3::hash(&exact_bytes).to_hex().to_string(),
            media_type: "image/jpeg".to_owned(),
            client_upload_key: None,
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

    let exact_request = test::TestRequest::put()
        .uri(&format!("/uploads/{}/parts/0", exact_upload.upload_id))
        .insert_header(("authorization", authorization.as_str()))
        .set_payload(exact_bytes)
        .to_request();
    let exact_response = test::call_service(&app, exact_request).await;
    assert_eq!(exact_response.status(), StatusCode::NO_CONTENT);

    let exact_size: i64 = sqlx::query_scalar!(
        "SELECT size_bytes FROM upload_parts WHERE upload_id = $1",
        exact_upload.upload_id
    )
    .fetch_one(&deps.pool)
    .await?;
    assert_eq!(exact_size, i64::try_from(UPLOAD_PART_SIZE_BYTES)?);
    let exact_key = StorageKey::staging_upload(exact_upload.upload_id, "part-00000000")?;
    assert!(deps.storage.exists(&exact_key).await?);

    let oversized_bytes = vec![0_u8; UPLOAD_PART_SIZE_BYTES + 1];
    let oversized_upload = create_upload(
        &deps.pool,
        CreateUploadInput {
            owner_id: 1,
            original_filename: "oversized-part.jpg".to_owned(),
            expected_size: i64::try_from(oversized_bytes.len())?,
            expected_blake3: blake3::hash(&oversized_bytes).to_hex().to_string(),
            media_type: "image/jpeg".to_owned(),
            client_upload_key: None,
        },
    )
    .await?;
    let oversized_request = test::TestRequest::put()
        .uri(&format!("/uploads/{}/parts/0", oversized_upload.upload_id))
        .insert_header(("authorization", authorization))
        .set_payload(oversized_bytes)
        .to_request();
    let oversized_response = test::call_service(&app, oversized_request).await;
    assert_eq!(oversized_response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let oversized_body: Value = test::read_body_json(oversized_response).await;
    assert_eq!(
        oversized_body,
        json!({
            "error": "upload_part_too_large",
            "message": "upload part exceeds the 4 MiB limit"
        })
    );

    let oversized_part_count: i64 = sqlx::query_scalar!(
        r#"SELECT count(*) as "count!" FROM upload_parts WHERE upload_id = $1"#,
        oversized_upload.upload_id
    )
    .fetch_one(&deps.pool)
    .await?;
    assert_eq!(oversized_part_count, 0);
    let oversized_key = StorageKey::staging_upload(oversized_upload.upload_id, "part-00000000")?;
    assert!(!deps.storage.exists(&oversized_key).await?);

    Ok(())
}

#[tokio::test]
#[ignore = "requires MIRROR_TEST_DATABASE_URL pointing at a dedicated test database"]
async fn complete_upload_route_promotes_asset_and_enqueues_jobs() -> TestResult {
    let deps = storage_test_deps().await?;
    let session = create_session(
        &deps.pool,
        SessionCreateInput {
            owner_id: 1,
            user_agent: Some("upload-http-test".to_owned()),
            device_name: Some("test browser".to_owned()),
        },
    )
    .await?;
    let bytes = jpeg_bytes();
    let upload = create_upload(
        &deps.pool,
        CreateUploadInput {
            owner_id: 1,
            original_filename: "route.jpg".to_owned(),
            expected_size: i64::try_from(bytes.len())?,
            expected_blake3: blake3::hash(&bytes).to_hex().to_string(),
            media_type: "image/jpeg".to_owned(),
            client_upload_key: None,
        },
    )
    .await?;
    put_part(&deps.pool, &deps.storage, 1, upload.upload_id, 0, bytes).await?;

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
    let req = test::TestRequest::post()
        .uri(&format!("/uploads/{}/complete", upload.upload_id))
        .cookie(Cookie::new(
            "mirror_session",
            session.token.expose().to_owned(),
        ))
        .insert_header(("x-csrf-token", session.csrf_token.expose()))
        .to_request();

    let response = test::call_service(&app, req).await;

    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = test::read_body_json(response).await;
    assert_eq!(body["upload"]["status"], "verified");
    assert!(body["promoted"]["asset_id"].is_string());
    assert_eq!(asset_count(&deps.pool).await?, 1);
    assert_eq!(job_count(&deps.pool).await?, 2);

    Ok(())
}

#[tokio::test]
#[ignore = "requires MIRROR_TEST_DATABASE_URL pointing at a dedicated test database"]
async fn create_upload_route_respects_persisted_rate_limit_bucket() -> TestResult {
    let deps = storage_test_deps().await?;
    let session = create_session(
        &deps.pool,
        SessionCreateInput {
            owner_id: 1,
            user_agent: Some("upload-rate-limit-test".to_owned()),
            device_name: Some("test browser".to_owned()),
        },
    )
    .await?;
    let config = Config::from_env();
    assert!(
        rate_limit::record_quota_attempt(
            &deps.pool,
            &config.rate_limit_secret,
            QuotaInput {
                action: "upload_create",
                key: "1",
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
                db: Some(deps.pool.clone()),
                setup: SetupState::Disabled,
                storage: Some(deps.storage.clone()),
            }))
            .configure(http::configure),
    )
    .await;
    let request = test::TestRequest::post()
        .uri("/uploads")
        .cookie(Cookie::new(
            "mirror_session",
            session.token.expose().to_owned(),
        ))
        .insert_header(("x-csrf-token", session.csrf_token.expose()))
        .set_json(json!({
            "original_filename": "blocked.jpg",
            "expected_size": jpeg_bytes().len(),
            "expected_blake3": blake3::hash(&jpeg_bytes()).to_hex().to_string(),
            "media_type": "image/jpeg",
            "client_upload_key": null,
        }))
        .to_request();

    let response = test::call_service(&app, request).await;

    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    let upload_count: i64 =
        sqlx::query_scalar!(r#"SELECT count(*) as "count!" FROM upload_sessions"#)
            .fetch_one(&deps.pool)
            .await?;
    assert_eq!(upload_count, 0);

    Ok(())
}
