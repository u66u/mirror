use actix_web::{App, cookie::Cookie, http::StatusCode, test, web};
use mirror_backend::{
    auth::{SessionCreateInput, SetupState, create_session},
    config::Config,
    http,
    state::AppState,
    uploads::{CreateUploadInput, create_upload, put_part},
};
use serde_json::Value;

mod support;
use support::{TestResult, asset_count, job_count, jpeg_bytes, storage_test_deps};

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
