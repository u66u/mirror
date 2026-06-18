use std::{num::NonZeroUsize, sync::Arc};

use actix_web::{App, cookie::Cookie, http::StatusCode, test as actix_test, web};
use mirror_backend::storage::ObjectStorage;
use mirror_backend::{
    auth::{SessionCreateInput, SetupState, create_session},
    config::Config,
    http,
    ml::{ImageTextEmbedder, SharedImageTextRuntime, sha256_f32_values},
    models::{
        ModelPackError, ModelPackFileManifest, ModelPackManifest, ModelPackSelfTestManifest,
        activate_model_pack, install_model_pack, install_model_pack_files,
        record_model_pack_self_test, record_reindex_asset_result, start_model_reindex,
        validate_embedding_output, validate_model_pack_manifest,
    },
    state::AppState,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tempfile::TempDir;
use uuid::Uuid;

mod support;
use support::{
    FakeImageTextEmbedder, TestResult, fresh_owner_pool, valid_image_preprocess,
    valid_model_pack_manifest, valid_onnx_config,
};

#[test]
fn model_pack_manifest_rejects_ambiguous_or_unsafe_inputs() {
    let mut missing_license = valid_model_pack_manifest();
    missing_license.license.clear();
    assert!(matches!(
        validate_model_pack_manifest(&missing_license),
        Err(ModelPackError::InvalidManifest("license"))
    ));

    let mut duplicate_paths = valid_model_pack_manifest();
    duplicate_paths.files.push(duplicate_paths.files[0].clone());
    assert!(matches!(
        validate_model_pack_manifest(&duplicate_paths),
        Err(ModelPackError::InvalidManifest("file.path.duplicate"))
    ));

    let mut parent_path = valid_model_pack_manifest();
    parent_path.files[0].path = "models/../encoder.onnx".to_owned();
    assert!(matches!(
        validate_model_pack_manifest(&parent_path),
        Err(ModelPackError::InvalidManifest("file.path"))
    ));

    let mut bad_checksum = valid_model_pack_manifest();
    bad_checksum.files[0].sha256 = "not-a-sha".to_owned();
    assert!(matches!(
        validate_model_pack_manifest(&bad_checksum),
        Err(ModelPackError::InvalidManifest("file.sha256"))
    ));

    let mut missing_self_test = valid_model_pack_manifest();
    missing_self_test.self_tests.clear();
    assert!(matches!(
        validate_model_pack_manifest(&missing_self_test),
        Err(ModelPackError::InvalidManifest("self_tests"))
    ));

    let mut missing_runtime_file = valid_model_pack_manifest();
    missing_runtime_file.onnx.image_model_path = "models/missing.onnx".to_owned();
    assert!(matches!(
        validate_model_pack_manifest(&missing_runtime_file),
        Err(ModelPackError::InvalidManifest("onnx.image_model_path"))
    ));

    let mut invalid_preprocess = valid_model_pack_manifest();
    invalid_preprocess.image_preprocess.std[0] = 0.0;
    assert!(matches!(
        validate_model_pack_manifest(&invalid_preprocess),
        Err(ModelPackError::InvalidManifest("image_preprocess.std"))
    ));
}

#[test]
fn embedding_output_validation_rejects_runtime_shape_and_float_footguns() -> TestResult {
    let manifest = valid_model_pack_manifest();
    let valid = validate_embedding_output(&manifest, vec![0.1; 768])?;
    assert_eq!(valid.values().len(), 768);

    assert!(matches!(
        validate_embedding_output(&manifest, vec![0.1; 767]),
        Err(ModelPackError::InvalidEmbedding("dimension"))
    ));

    let mut nan = vec![0.1; 768];
    nan[0] = f32::NAN;
    assert!(matches!(
        validate_embedding_output(&manifest, nan),
        Err(ModelPackError::InvalidEmbedding("finite"))
    ));

    assert!(matches!(
        validate_embedding_output(&manifest, vec![0.0; 768]),
        Err(ModelPackError::InvalidEmbedding("zero_norm"))
    ));

    Ok(())
}

#[tokio::test]
async fn model_pack_file_install_verifies_checksum_size_and_storage_namespace() -> TestResult {
    let source_dir = TempDir::new()?;
    let storage_dir = TempDir::new()?;
    let storage = ObjectStorage::local(storage_dir.path())?;
    let model_id = Uuid::now_v7();
    std::fs::create_dir_all(source_dir.path().join("models"))?;
    std::fs::write(
        source_dir.path().join("models/image_encoder.onnx"),
        b"image",
    )?;
    std::fs::write(source_dir.path().join("models/text_encoder.onnx"), b"text")?;
    std::fs::create_dir_all(source_dir.path().join("tokenizer"))?;
    std::fs::write(
        source_dir.path().join("tokenizer/tokenizer.json"),
        b"tokenizer",
    )?;
    std::fs::create_dir_all(source_dir.path().join("self-tests"))?;
    std::fs::write(source_dir.path().join("self-tests/cat.jpg"), b"cat")?;

    let manifest = file_install_manifest()?;
    let installed =
        install_model_pack_files(&storage, source_dir.path(), model_id, &manifest).await?;

    assert_eq!(installed.len(), 4);
    assert!(installed.iter().all(|file| {
        file.storage_key
            .as_str()
            .starts_with(&format!("model-packs/{model_id}/"))
    }));
    let image = storage.read(&installed[0].storage_key).await?;
    assert_eq!(image, b"image");

    let mut bad = manifest.clone();
    bad.files[0].sha256 = "0".repeat(64);
    assert!(matches!(
        install_model_pack_files(&storage, source_dir.path(), Uuid::now_v7(), &bad).await,
        Err(ModelPackError::FileVerificationFailed)
    ));

    Ok(())
}

#[tokio::test]
#[ignore = "requires MIRROR_TEST_DATABASE_URL pointing at a dedicated test database"]
async fn model_pack_activation_requires_passed_self_test_and_is_one_active_per_kind() -> TestResult
{
    let pool = fresh_owner_pool().await?;
    let first = install_model_pack(&pool, valid_model_pack_manifest()).await?;
    assert_eq!(first.status, "installed");
    assert_eq!(first.self_test_status, "pending");
    assert!(matches!(
        activate_model_pack(&pool, first.model_pack_id).await,
        Err(ModelPackError::SelfTestRequired)
    ));

    let failed = record_model_pack_self_test(
        &pool,
        first.model_pack_id,
        false,
        Some("golden output mismatch"),
    )
    .await?;
    assert_eq!(failed.self_test_status, "failed");
    assert!(matches!(
        activate_model_pack(&pool, first.model_pack_id).await,
        Err(ModelPackError::SelfTestRequired)
    ));

    record_model_pack_self_test(&pool, first.model_pack_id, true, None).await?;
    let active = activate_model_pack(&pool, first.model_pack_id).await?;
    assert_eq!(active.status, "active");

    let mut second_manifest = valid_model_pack_manifest();
    second_manifest.model_revision = "2026-06-18.2".to_owned();
    let second = install_model_pack(&pool, second_manifest).await?;
    record_model_pack_self_test(&pool, second.model_pack_id, true, None).await?;
    activate_model_pack(&pool, second.model_pack_id).await?;

    let statuses: Vec<(String, String)> = sqlx::query!(
        r#"
        SELECT model_revision, status
        FROM model_packs
        WHERE kind = 'semantic_image_text'
        ORDER BY model_revision
        "#
    )
    .map(|r| (r.model_revision, r.status))
    .fetch_all(&pool)
    .await?;
    assert_eq!(
        statuses,
        vec![
            ("2026-06-18.1".to_owned(), "installed".to_owned()),
            ("2026-06-18.2".to_owned(), "active".to_owned())
        ]
    );

    Ok(())
}

#[tokio::test]
#[ignore = "requires MIRROR_TEST_DATABASE_URL pointing at a dedicated test database"]
async fn model_pack_admin_routes_install_self_test_activate_and_reindex() -> TestResult {
    let pool = fresh_owner_pool().await?;
    let session = create_session(
        &pool,
        SessionCreateInput {
            owner_id: 1,
            user_agent: Some("model-pack-route-test".to_owned()),
            device_name: Some("test browser".to_owned()),
        },
    )
    .await?;
    let app = actix_test::init_service(
        App::new()
            .app_data(web::Data::new(AppState {
                config: Config::from_env(),
                db: Some(pool.clone()),
                setup: SetupState::Disabled,
                storage: None,
            }))
            .configure(http::configure),
    )
    .await;

    let install = actix_test::TestRequest::post()
        .uri("/model-packs")
        .cookie(Cookie::new(
            "mirror_session",
            session.token.expose().to_owned(),
        ))
        .insert_header(("x-csrf-token", session.csrf_token.expose()))
        .set_json(valid_model_pack_manifest())
        .to_request();
    let install_response = actix_test::call_service(&app, install).await;
    assert_eq!(install_response.status(), StatusCode::CREATED);
    let installed: Value = actix_test::read_body_json(install_response).await;
    let model_pack_id = installed["model_pack_id"]
        .as_str()
        .ok_or_else(|| std::io::Error::other("model_pack_id missing"))?;

    let early_activate = actix_test::TestRequest::post()
        .uri(&format!("/model-packs/{model_pack_id}/activate"))
        .cookie(Cookie::new(
            "mirror_session",
            session.token.expose().to_owned(),
        ))
        .insert_header(("x-csrf-token", session.csrf_token.expose()))
        .to_request();
    let early_activate_response = actix_test::call_service(&app, early_activate).await;
    assert_eq!(early_activate_response.status(), StatusCode::CONFLICT);

    let self_test = actix_test::TestRequest::post()
        .uri(&format!("/model-packs/{model_pack_id}/self-test"))
        .cookie(Cookie::new(
            "mirror_session",
            session.token.expose().to_owned(),
        ))
        .insert_header(("x-csrf-token", session.csrf_token.expose()))
        .set_json(json!({ "passed": true }))
        .to_request();
    assert_eq!(
        actix_test::call_service(&app, self_test).await.status(),
        StatusCode::OK
    );

    let activate = actix_test::TestRequest::post()
        .uri(&format!("/model-packs/{model_pack_id}/activate"))
        .cookie(Cookie::new(
            "mirror_session",
            session.token.expose().to_owned(),
        ))
        .insert_header(("x-csrf-token", session.csrf_token.expose()))
        .to_request();
    let activate_response = actix_test::call_service(&app, activate).await;
    assert_eq!(activate_response.status(), StatusCode::OK);

    let reindex = actix_test::TestRequest::post()
        .uri(&format!("/model-packs/{model_pack_id}/reindex"))
        .cookie(Cookie::new(
            "mirror_session",
            session.token.expose().to_owned(),
        ))
        .insert_header(("x-csrf-token", session.csrf_token.expose()))
        .to_request();
    let reindex_response = actix_test::call_service(&app, reindex).await;
    assert_eq!(reindex_response.status(), StatusCode::ACCEPTED);
    let reindex_body: Value = actix_test::read_body_json(reindex_response).await;

    let list = actix_test::TestRequest::get()
        .uri("/model-packs")
        .cookie(Cookie::new(
            "mirror_session",
            session.token.expose().to_owned(),
        ))
        .to_request();
    let list_response = actix_test::call_service(&app, list).await;
    assert_eq!(list_response.status(), StatusCode::OK);
    let packs: Value = actix_test::read_body_json(list_response).await;
    assert_eq!(packs.as_array().map(Vec::len), Some(1));
    assert_eq!(packs[0]["model_pack_id"].as_str(), Some(model_pack_id));
    assert_eq!(packs[0]["status"], "active");

    let runs = actix_test::TestRequest::get()
        .uri(&format!("/model-packs/{model_pack_id}/reindex-runs"))
        .cookie(Cookie::new(
            "mirror_session",
            session.token.expose().to_owned(),
        ))
        .to_request();
    let runs_response = actix_test::call_service(&app, runs).await;
    assert_eq!(runs_response.status(), StatusCode::OK);
    let runs_body: Value = actix_test::read_body_json(runs_response).await;
    assert_eq!(runs_body.as_array().map(Vec::len), Some(1));
    assert_eq!(
        runs_body[0]["reindex_run_id"],
        reindex_body["reindex_run_id"]
    );

    Ok(())
}

#[tokio::test]
#[ignore = "requires MIRROR_TEST_DATABASE_URL pointing at a dedicated test database"]
async fn model_pack_self_test_run_route_records_runtime_result() -> TestResult {
    let pool = fresh_owner_pool().await?;
    let storage_dir = TempDir::new()?;
    let storage = ObjectStorage::local(storage_dir.path())?;
    let session = create_session(
        &pool,
        SessionCreateInput {
            owner_id: 1,
            user_agent: Some("model-pack-self-test-route-test".to_owned()),
            device_name: Some("test browser".to_owned()),
        },
    )
    .await?;
    let embedder: Arc<dyn ImageTextEmbedder + Send + Sync> = Arc::new(FakeImageTextEmbedder);
    let runtime = SharedImageTextRuntime::new(embedder, NonZeroUsize::MIN);
    let app = actix_test::init_service(
        App::new()
            .app_data(web::Data::new(runtime))
            .app_data(web::Data::new(AppState {
                config: Config::from_env(),
                db: Some(pool.clone()),
                setup: SetupState::Disabled,
                storage: Some(storage.clone()),
            }))
            .configure(http::configure),
    )
    .await;

    let manifest = file_install_manifest_with_expected(&sha256_f32_values(&vec![1.0; 768]))?;
    let install = actix_test::TestRequest::post()
        .uri("/model-packs")
        .cookie(Cookie::new(
            "mirror_session",
            session.token.expose().to_owned(),
        ))
        .insert_header(("x-csrf-token", session.csrf_token.expose()))
        .set_json(&manifest)
        .to_request();
    let install_response = actix_test::call_service(&app, install).await;
    assert_eq!(install_response.status(), StatusCode::CREATED);
    let installed: Value = actix_test::read_body_json(install_response).await;
    let model_pack_id = Uuid::parse_str(
        installed["model_pack_id"]
            .as_str()
            .ok_or_else(|| std::io::Error::other("model_pack_id missing"))?,
    )
    .map_err(std::io::Error::other)?;

    let source_dir = write_model_pack_source_files()?;
    install_model_pack_files(&storage, source_dir.path(), model_pack_id, &manifest).await?;

    let run = actix_test::TestRequest::post()
        .uri(&format!("/model-packs/{model_pack_id}/self-test/run"))
        .cookie(Cookie::new(
            "mirror_session",
            session.token.expose().to_owned(),
        ))
        .insert_header(("x-csrf-token", session.csrf_token.expose()))
        .to_request();
    let response = actix_test::call_service(&app, run).await;

    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = actix_test::read_body_json(response).await;
    assert_eq!(body["self_test_status"], "passed");
    Ok(())
}

#[tokio::test]
#[ignore = "requires MIRROR_TEST_DATABASE_URL pointing at a dedicated test database"]
async fn model_reindex_queues_embedding_jobs_for_active_assets_only() -> TestResult {
    let pool = fresh_owner_pool().await?;
    let pack = install_model_pack(&pool, valid_model_pack_manifest()).await?;
    assert!(matches!(
        start_model_reindex(&pool, pack.model_pack_id).await,
        Err(ModelPackError::SelfTestRequired)
    ));
    record_model_pack_self_test(&pool, pack.model_pack_id, true, None).await?;

    let active_asset_id = insert_model_pack_asset(&pool, false).await?;
    let trashed_asset_id = insert_model_pack_asset(&pool, true).await?;
    let run = start_model_reindex(&pool, pack.model_pack_id).await?;

    assert_eq!(run.status, "queued");
    assert_eq!(run.total_assets, 1);
    assert_eq!(run.queued_assets, 1);
    let jobs: Vec<(String, serde_json::Value)> =
        sqlx::query!("SELECT kind, payload FROM jobs WHERE kind = 'embed_asset'")
            .map(|r| (r.kind, r.payload))
            .fetch_all(&pool)
            .await?;
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].0, "embed_asset");
    assert_eq!(
        jobs[0].1["asset_id"].as_str(),
        Some(active_asset_id.to_string().as_str())
    );
    assert_ne!(
        jobs[0].1["asset_id"].as_str(),
        Some(trashed_asset_id.to_string().as_str())
    );
    assert_eq!(
        jobs[0].1["model_pack_id"].as_str(),
        Some(pack.model_pack_id.to_string().as_str())
    );
    assert_eq!(
        jobs[0].1["reindex_run_id"].as_str(),
        Some(run.reindex_run_id.to_string().as_str())
    );
    let rows: i64 = sqlx::query_scalar!(
        r#"SELECT count(*) as "count!" FROM model_reindex_assets WHERE reindex_run_id = $1"#,
        run.reindex_run_id
    )
    .fetch_one(&pool)
    .await?;
    assert_eq!(rows, 1);

    let done =
        record_reindex_asset_result(&pool, run.reindex_run_id, active_asset_id, true, None).await?;
    assert_eq!(done.status, "succeeded");
    assert_eq!(done.processed_assets, 1);
    assert_eq!(done.failed_assets, 0);
    let duplicate =
        record_reindex_asset_result(&pool, run.reindex_run_id, active_asset_id, true, None).await?;
    assert_eq!(duplicate.processed_assets, 1);
    assert_eq!(duplicate.failed_assets, 0);

    Ok(())
}

#[tokio::test]
#[ignore = "requires MIRROR_TEST_DATABASE_URL pointing at a dedicated test database"]
async fn model_reindex_records_terminal_failures_without_double_counting() -> TestResult {
    let pool = fresh_owner_pool().await?;
    let pack = install_model_pack(&pool, valid_model_pack_manifest()).await?;
    record_model_pack_self_test(&pool, pack.model_pack_id, true, None).await?;
    let ok_asset_id = insert_model_pack_asset(&pool, false).await?;
    let failed_asset_id = insert_model_pack_asset(&pool, false).await?;
    let run = start_model_reindex(&pool, pack.model_pack_id).await?;
    assert_eq!(run.total_assets, 2);

    let running =
        record_reindex_asset_result(&pool, run.reindex_run_id, ok_asset_id, true, None).await?;
    assert_eq!(running.status, "running");
    assert_eq!(running.processed_assets, 1);
    assert_eq!(running.failed_assets, 0);

    let failed = record_reindex_asset_result(
        &pool,
        run.reindex_run_id,
        failed_asset_id,
        false,
        Some("embedding runtime failed"),
    )
    .await?;
    assert_eq!(failed.status, "failed");
    assert_eq!(failed.processed_assets, 1);
    assert_eq!(failed.failed_assets, 1);
    let duplicate = record_reindex_asset_result(
        &pool,
        run.reindex_run_id,
        failed_asset_id,
        false,
        Some("embedding runtime failed"),
    )
    .await?;
    assert_eq!(duplicate.processed_assets, 1);
    assert_eq!(duplicate.failed_assets, 1);

    let stored_error: Option<String> = sqlx::query_scalar!(
        r#"
        SELECT error_message
        FROM model_reindex_assets
        WHERE reindex_run_id = $1 AND asset_id = $2
        "#,
        run.reindex_run_id,
        failed_asset_id
    )
    .fetch_one(&pool)
    .await?;
    assert_eq!(stored_error.as_deref(), Some("embedding runtime failed"));

    Ok(())
}

fn file_install_manifest() -> TestResult<ModelPackManifest> {
    file_install_manifest_with_expected(&"d".repeat(64))
}

fn file_install_manifest_with_expected(
    expected_output_sha256: &str,
) -> TestResult<ModelPackManifest> {
    Ok(ModelPackManifest {
        kind: "semantic_image_text".to_owned(),
        runtime: "onnx".to_owned(),
        model_key: "siglip2-base-patch16-224".to_owned(),
        model_revision: "2026-06-18.files".to_owned(),
        license: "Apache-2.0".to_owned(),
        embedding_dimension: 768,
        distance_metric: "cosine".to_owned(),
        onnx: valid_onnx_config(),
        image_preprocess: valid_image_preprocess(),
        files: vec![
            file_manifest("models/image_encoder.onnx", b"image")?,
            file_manifest("models/text_encoder.onnx", b"text")?,
            file_manifest("tokenizer/tokenizer.json", b"tokenizer")?,
            file_manifest("self-tests/cat.jpg", b"cat")?,
        ],
        self_tests: vec![ModelPackSelfTestManifest {
            name: "text_image_fixture_similarity".to_owned(),
            input_path: "self-tests/cat.jpg".to_owned(),
            expected_output_sha256: expected_output_sha256.to_owned(),
        }],
    })
}

fn write_model_pack_source_files() -> TestResult<TempDir> {
    let source_dir = TempDir::new()?;
    std::fs::create_dir_all(source_dir.path().join("models"))?;
    std::fs::write(
        source_dir.path().join("models/image_encoder.onnx"),
        b"image",
    )?;
    std::fs::write(source_dir.path().join("models/text_encoder.onnx"), b"text")?;
    std::fs::create_dir_all(source_dir.path().join("tokenizer"))?;
    std::fs::write(
        source_dir.path().join("tokenizer/tokenizer.json"),
        b"tokenizer",
    )?;
    std::fs::create_dir_all(source_dir.path().join("self-tests"))?;
    std::fs::write(source_dir.path().join("self-tests/cat.jpg"), b"cat")?;
    Ok(source_dir)
}

fn file_manifest(path: &str, bytes: &[u8]) -> TestResult<ModelPackFileManifest> {
    let digest = Sha256::digest(bytes);
    Ok(ModelPackFileManifest {
        path: path.to_owned(),
        sha256: format!("{digest:x}"),
        size_bytes: i64::try_from(bytes.len())?,
    })
}

async fn insert_model_pack_asset(pool: &sqlx::PgPool, trashed: bool) -> TestResult<Uuid> {
    let bytes = Uuid::now_v7().as_bytes().to_vec();
    let hash = blake3::hash(&bytes).to_hex().to_string();
    let original_id = Uuid::now_v7();
    let asset_id = Uuid::now_v7();
    sqlx::query!(
        r#"
        INSERT INTO originals (id, blake3_hash, storage_key, size_bytes, media_type)
        VALUES ($1, $2, $3, $4, 'image/jpeg')
        "#,
        original_id,
        hash,
        format!("originals/blake3/{hash}"),
        i64::try_from(bytes.len())?
    )
    .execute(pool)
    .await?;
    sqlx::query!(
        r#"
        INSERT INTO assets (id, public_id, owner_id, original_id, trashed_at)
        VALUES ($1, $2, 1, $3, CASE WHEN $4 THEN now() ELSE NULL END)
        "#,
        asset_id,
        Uuid::now_v7(),
        original_id,
        trashed
    )
    .execute(pool)
    .await?;
    Ok(asset_id)
}
