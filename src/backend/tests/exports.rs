use actix_web::{App, cookie::Cookie, http::StatusCode, test, web};
use mirror_backend::{
    assets::trash_asset,
    auth::{SessionCreateInput, SetupState, create_session},
    config::Config,
    http,
    state::AppState,
};
use serde_json::Value;

mod support;
use support::{TestResult, create_promoted_asset, jpeg_bytes, storage_test_deps};

#[tokio::test]
#[ignore = "requires MIRROR_TEST_DATABASE_URL pointing at a dedicated test database"]
async fn original_export_manifest_lists_active_originals_only() -> TestResult {
    let deps = storage_test_deps().await?;
    let active = create_promoted_asset(&deps, "active-export.jpg").await?;
    let trashed = create_promoted_asset(&deps, "trashed-export.jpg").await?;
    trash_asset(&deps.pool, 1, trashed.public_id).await?;
    let session = create_session(
        &deps.pool,
        SessionCreateInput {
            owner_id: 1,
            user_agent: Some("export-manifest-test".to_owned()),
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

    let request = test::TestRequest::get()
        .uri("/exports/originals/manifest")
        .cookie(Cookie::new(
            "mirror_session",
            session.token.expose().to_owned(),
        ))
        .to_request();
    let response = test::call_service(&app, request).await;

    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = test::read_body_json(response).await;
    assert_eq!(body["manifest_version"], "mirror-original-export-v1");
    assert!(!body["generated_at"].is_null());
    assert_eq!(body["items"].as_array().map(Vec::len), Some(1));
    let item = &body["items"][0];
    let active_public_id = active.public_id.to_string();
    assert_eq!(item["asset_id"].as_str(), Some(active_public_id.as_str()));
    assert_eq!(item["original_filename"], "active-export.jpg");
    assert_eq!(item["media_type"], "image/jpeg");
    assert!(
        item["blake3_hash"]
            .as_str()
            .is_some_and(|hash| hash.len() == 64)
    );
    assert!(
        item["storage_key"]
            .as_str()
            .is_some_and(|key| key.starts_with("originals/blake3/"))
    );

    let bytes_request = test::TestRequest::get()
        .uri(&format!("/exports/originals/{}", active.public_id))
        .cookie(Cookie::new(
            "mirror_session",
            session.token.expose().to_owned(),
        ))
        .to_request();
    let bytes_response = test::call_service(&app, bytes_request).await;
    assert_eq!(bytes_response.status(), StatusCode::OK);
    assert_eq!(
        bytes_response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok()),
        Some("image/jpeg")
    );
    let expected_len = jpeg_bytes().len().to_string();
    assert_eq!(
        bytes_response
            .headers()
            .get("content-length")
            .and_then(|value| value.to_str().ok()),
        Some(expected_len.as_str())
    );
    assert_eq!(test::read_body(bytes_response).await, jpeg_bytes());

    let trashed_request = test::TestRequest::get()
        .uri(&format!("/exports/originals/{}", trashed.public_id))
        .cookie(Cookie::new(
            "mirror_session",
            session.token.expose().to_owned(),
        ))
        .to_request();
    assert_eq!(
        test::call_service(&app, trashed_request).await.status(),
        StatusCode::NOT_FOUND
    );

    Ok(())
}

#[tokio::test]
#[ignore = "requires MIRROR_TEST_DATABASE_URL pointing at a dedicated test database"]
async fn original_export_archive_contains_manifest_and_active_original_bytes() -> TestResult {
    let deps = storage_test_deps().await?;
    let active = create_promoted_asset(&deps, "archive-export.jpg").await?;
    let trashed = create_promoted_asset(&deps, "trashed-archive-export.jpg").await?;
    trash_asset(&deps.pool, 1, trashed.public_id).await?;
    let session = create_session(
        &deps.pool,
        SessionCreateInput {
            owner_id: 1,
            user_agent: Some("export-archive-test".to_owned()),
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

    let request = test::TestRequest::get()
        .uri("/exports/originals/archive.tar")
        .cookie(Cookie::new(
            "mirror_session",
            session.token.expose().to_owned(),
        ))
        .to_request();
    let response = test::call_service(&app, request).await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok()),
        Some("application/x-tar")
    );
    let body = test::read_body(response).await.to_vec();
    let entries = parse_tar_entries(&body)?;
    let manifest = entries
        .iter()
        .find(|entry| entry.path == "manifest.json")
        .ok_or_else(|| std::io::Error::other("manifest missing"))?;
    let manifest_json: Value =
        serde_json::from_slice(&manifest.bytes).map_err(std::io::Error::other)?;
    assert_eq!(manifest_json["items"].as_array().map(Vec::len), Some(1));
    let active_id = active.public_id.to_string();
    assert_eq!(
        manifest_json["items"][0]["asset_id"].as_str(),
        Some(active_id.as_str())
    );
    assert!(!String::from_utf8_lossy(&manifest.bytes).contains(&trashed.public_id.to_string()));

    let original_path = format!("originals/{}.jpg", active.public_id);
    let original = entries
        .iter()
        .find(|entry| entry.path == original_path)
        .ok_or_else(|| std::io::Error::other("original missing"))?;
    assert_eq!(original.bytes, jpeg_bytes());

    Ok(())
}

#[tokio::test]
#[ignore = "requires MIRROR_TEST_DATABASE_URL pointing at a dedicated test database"]
async fn original_export_manifest_is_owner_rate_limited() -> TestResult {
    let deps = storage_test_deps().await?;
    create_promoted_asset(&deps, "rate-limited-export.jpg").await?;
    let session = create_session(
        &deps.pool,
        SessionCreateInput {
            owner_id: 1,
            user_agent: Some("export-rate-limit-test".to_owned()),
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

    for _ in 0..30 {
        let request = test::TestRequest::get()
            .uri("/exports/originals/manifest")
            .cookie(Cookie::new(
                "mirror_session",
                session.token.expose().to_owned(),
            ))
            .to_request();
        assert_eq!(
            test::call_service(&app, request).await.status(),
            StatusCode::OK
        );
    }

    let blocked_request = test::TestRequest::get()
        .uri("/exports/originals/manifest")
        .cookie(Cookie::new(
            "mirror_session",
            session.token.expose().to_owned(),
        ))
        .to_request();
    assert_eq!(
        test::call_service(&app, blocked_request).await.status(),
        StatusCode::TOO_MANY_REQUESTS
    );

    let stored_keys: Vec<Vec<u8>> =
        sqlx::query_scalar!("SELECT key_hash FROM rate_limit_buckets WHERE action = $1", "export_original_manifest")
            .fetch_all(&deps.pool)
            .await?;
    assert_eq!(stored_keys.len(), 1);

    Ok(())
}

struct TarEntry {
    path: String,
    bytes: Vec<u8>,
}

fn parse_tar_entries(bytes: &[u8]) -> TestResult<Vec<TarEntry>> {
    let mut offset = 0_usize;
    let mut entries = Vec::new();
    while offset + 512 <= bytes.len() {
        let header = &bytes[offset..offset + 512];
        if header.iter().all(|byte| *byte == 0) {
            break;
        }
        let path_end = header[..100]
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(100);
        let path = std::str::from_utf8(&header[..path_end])
            .map_err(std::io::Error::other)?
            .to_owned();
        let size_end = header[124..136]
            .iter()
            .position(|byte| *byte == 0 || *byte == b' ')
            .unwrap_or(12);
        let size_text =
            std::str::from_utf8(&header[124..124 + size_end]).map_err(std::io::Error::other)?;
        let size = usize::from_str_radix(size_text, 8).map_err(std::io::Error::other)?;
        offset += 512;
        if offset + size > bytes.len() {
            return Err(std::io::Error::other("tar entry exceeds archive").into());
        }
        entries.push(TarEntry {
            path,
            bytes: bytes[offset..offset + size].to_vec(),
        });
        offset += size + tar_entry_padding(size);
    }
    Ok(entries)
}

fn tar_entry_padding(size: usize) -> usize {
    let remainder = size % 512;
    if remainder == 0 { 0 } else { 512 - remainder }
}
