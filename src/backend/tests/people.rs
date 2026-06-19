use std::sync::Arc;

use actix_web::{App, cookie::Cookie, http::StatusCode, test as actix_test, web};
use mirror_backend::{
    auth::{SessionCreateInput, SetupState, create_session},
    config::Config,
    face::{self, FaceBox, FaceRuntime, FaceRuntimeRequest, IndexedFace, SharedFaceRuntime},
    http,
    jobs::{self, JobKind},
    models::{
        ModelPackFileManifest, ModelPackManifest, ModelPackSelfTestManifest, activate_model_pack,
        install_model_pack, record_model_pack_self_test,
    },
    state::AppState,
    storage::StorageKey,
};
use pgvector::Vector;
use serde_json::Value;
use time::OffsetDateTime;
use uuid::Uuid;

mod support;
use support::{
    TestResult, create_promoted_asset, fresh_owner_pool, storage_test_deps, valid_image_preprocess,
    valid_onnx_config,
};
use support::{valid_face_detection_config, valid_face_embedding_config};

#[tokio::test]
#[ignore = "requires MIRROR_TEST_DATABASE_URL pointing at a dedicated test database with pgvector"]
async fn face_tables_preserve_owner_boundaries_and_asset_cascades() -> TestResult {
    let pool = fresh_owner_pool().await?;
    let model_pack = install_model_pack(&pool, face_embedding_manifest()).await?;
    let asset = insert_asset(&pool).await?;
    let person_id = Uuid::now_v7();
    let face_id = Uuid::now_v7();

    sqlx::query(
        r#"
        INSERT INTO people (id, owner_id, display_name, review_status)
        VALUES ($1, 1, NULL, 'unreviewed')
        "#,
    )
    .bind(person_id)
    .execute(&pool)
    .await?;
    sqlx::query(
        r#"
        INSERT INTO face_occurrences (
            id, asset_id, owner_id, detection_model_pack_id,
            bbox_left, bbox_top, bbox_width, bbox_height, quality
        )
        VALUES ($1, $2, 1, NULL, 0.10, 0.20, 0.30, 0.40, 0.90)
        "#,
    )
    .bind(face_id)
    .bind(asset)
    .execute(&pool)
    .await?;
    sqlx::query(
        r#"
        INSERT INTO person_faces (person_id, face_occurrence_id, owner_id)
        VALUES ($1, $2, 1)
        "#,
    )
    .bind(person_id)
    .bind(face_id)
    .execute(&pool)
    .await?;
    sqlx::query(
        r#"
        INSERT INTO face_embeddings (
            face_occurrence_id, owner_id, model_pack_id, embedding, embedding_dimension
        )
        VALUES ($1, 1, $2, $3, 3)
        "#,
    )
    .bind(face_id)
    .bind(model_pack.model_pack_id)
    .bind(Vector::from(vec![0.1_f32, 0.2, 0.3]))
    .execute(&pool)
    .await?;

    let owner_mismatch = sqlx::query(
        r#"
        INSERT INTO face_occurrences (
            id, asset_id, owner_id, bbox_left, bbox_top, bbox_width, bbox_height
        )
        VALUES ($1, $2, 2, 0.10, 0.20, 0.30, 0.40)
        "#,
    )
    .bind(Uuid::now_v7())
    .bind(asset)
    .execute(&pool)
    .await;
    assert!(owner_mismatch.is_err());

    let asset_embedding_count: i64 = sqlx::query_scalar("SELECT count(*) FROM asset_embeddings")
        .fetch_one(&pool)
        .await?;
    assert_eq!(asset_embedding_count, 0);

    sqlx::query("DELETE FROM assets WHERE id = $1")
        .bind(asset)
        .execute(&pool)
        .await?;
    let face_count: i64 = sqlx::query_scalar("SELECT count(*) FROM face_occurrences")
        .fetch_one(&pool)
        .await?;
    let face_embedding_count: i64 = sqlx::query_scalar("SELECT count(*) FROM face_embeddings")
        .fetch_one(&pool)
        .await?;
    let person_face_count: i64 = sqlx::query_scalar("SELECT count(*) FROM person_faces")
        .fetch_one(&pool)
        .await?;

    assert_eq!(face_count, 0);
    assert_eq!(face_embedding_count, 0);
    assert_eq!(person_face_count, 0);
    Ok(())
}

#[tokio::test]
#[ignore = "requires MIRROR_TEST_DATABASE_URL pointing at a dedicated test database with pgvector"]
async fn face_index_job_persists_embeddings_and_people_assignment() -> TestResult {
    let deps = storage_test_deps().await?;
    let pack = install_model_pack(&deps.pool, face_identity_manifest()).await?;
    record_model_pack_self_test(&deps.pool, pack.model_pack_id, true, None).await?;
    activate_model_pack(&deps.pool, pack.model_pack_id).await?;
    let asset = create_promoted_asset(&deps, "face-job.jpg").await?;
    face::enqueue_face_index(&deps.pool, asset.internal_id).await?;
    let job = jobs::lease_next_for_kinds(
        &deps.pool,
        "face-worker",
        OffsetDateTime::UNIX_EPOCH,
        &[JobKind::IndexFaces],
    )
    .await?
    .ok_or_else(|| std::io::Error::other("index_faces job was not leased"))?;
    let runtime = fake_people_face_runtime(vec![1.0, 0.0, 0.0]);

    face::run_face_index_job(&deps.pool, &deps.storage, &runtime, &job).await?;

    let face_count: i64 = sqlx::query_scalar("SELECT count(*) FROM face_occurrences")
        .fetch_one(&deps.pool)
        .await?;
    let embedding_count: i64 = sqlx::query_scalar("SELECT count(*) FROM face_embeddings")
        .fetch_one(&deps.pool)
        .await?;
    let people_count: i64 = sqlx::query_scalar("SELECT count(*) FROM people")
        .fetch_one(&deps.pool)
        .await?;
    let assigned_count: i64 = sqlx::query_scalar("SELECT count(*) FROM person_faces")
        .fetch_one(&deps.pool)
        .await?;
    let detection_pack_id: Option<Uuid> =
        sqlx::query_scalar("SELECT detection_model_pack_id FROM face_occurrences LIMIT 1")
            .fetch_one(&deps.pool)
            .await?;

    assert_eq!(face_count, 1);
    assert_eq!(embedding_count, 1);
    assert_eq!(people_count, 1);
    assert_eq!(assigned_count, 1);
    assert_eq!(detection_pack_id, Some(pack.model_pack_id));

    let people = mirror_backend::people::list_people(&deps.pool, 1).await?;
    assert_eq!(people.len(), 1);
    assert_eq!(people[0].face_count, 1);
    let person_id = people[0].person_id;
    mirror_backend::people::rename_person(&deps.pool, 1, person_id, " Ada ").await?;
    let people = mirror_backend::people::list_people(&deps.pool, 1).await?;
    assert_eq!(people[0].display_name.as_deref(), Some("Ada"));
    assert_eq!(people[0].review_status, "reviewed");
    mirror_backend::people::hide_person(&deps.pool, 1, person_id).await?;
    let people = mirror_backend::people::list_people(&deps.pool, 1).await?;
    assert!(people.is_empty());
    Ok(())
}

#[tokio::test]
#[ignore = "requires MIRROR_TEST_DATABASE_URL pointing at a dedicated test database with pgvector"]
async fn people_album_routes_list_assigned_and_unassigned_faces() -> TestResult {
    let deps = storage_test_deps().await?;
    let assigned_asset = create_promoted_asset(&deps, "assigned-face.jpg").await?;
    let unassigned_asset = create_promoted_asset(&deps, "unassigned-face.jpg").await?;
    let person_id = Uuid::now_v7();
    sqlx::query(
        r#"
        INSERT INTO people (id, owner_id, display_name, review_status)
        VALUES ($1, 1, 'Ada', 'reviewed')
        "#,
    )
    .bind(person_id)
    .execute(&deps.pool)
    .await?;
    let assigned_face =
        insert_face_occurrence(&deps.pool, assigned_asset.internal_id, "assigned", 0.10).await?;
    let chip_key = StorageKey::face_chip(assigned_face, "webp", "test-face-chip-v1")?;
    deps.storage.write(&chip_key, b"chip-webp".to_vec()).await?;
    sqlx::query(
        r#"
        UPDATE face_occurrences
        SET chip_storage_key = $1, chip_width = 112, chip_height = 112, chip_format = 'webp'
        WHERE id = $2
        "#,
    )
    .bind(chip_key.as_str())
    .bind(assigned_face)
    .execute(&deps.pool)
    .await?;
    let unassigned_face =
        insert_face_occurrence(&deps.pool, unassigned_asset.internal_id, "unassigned", 0.55)
            .await?;
    sqlx::query(
        r#"
        INSERT INTO person_faces (person_id, face_occurrence_id, owner_id)
        VALUES ($1, $2, 1)
        "#,
    )
    .bind(person_id)
    .bind(assigned_face)
    .execute(&deps.pool)
    .await?;

    let assigned =
        mirror_backend::people::list_person_faces(&deps.pool, 1, person_id, Some(10)).await?;
    assert_eq!(assigned.len(), 1);
    assert_eq!(assigned[0].face_id, assigned_face);
    assert_eq!(assigned[0].asset_id, assigned_asset.public_id);
    assert_eq!(assigned[0].review_state, "assigned");
    assert_eq!(assigned[0].bbox.left, 0.10);
    assert!(assigned[0].chip_available);

    let unassigned = mirror_backend::people::list_unassigned_faces(&deps.pool, 1, Some(10)).await?;
    assert_eq!(unassigned.len(), 1);
    assert_eq!(unassigned[0].face_id, unassigned_face);
    assert_eq!(unassigned[0].asset_id, unassigned_asset.public_id);
    assert_eq!(unassigned[0].review_state, "unassigned");

    let session = create_session(
        &deps.pool,
        SessionCreateInput {
            owner_id: 1,
            user_agent: Some("people-album-route-test".to_owned()),
            device_name: Some("test browser".to_owned()),
        },
    )
    .await?;
    let app = actix_test::init_service(
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

    let assigned_request = actix_test::TestRequest::get()
        .uri(&format!("/people/{person_id}/faces?limit=5"))
        .cookie(Cookie::new(
            "mirror_session",
            session.token.expose().to_owned(),
        ))
        .to_request();
    let assigned_response = actix_test::call_service(&app, assigned_request).await;
    assert_eq!(assigned_response.status(), StatusCode::OK);
    let assigned_body: Value = actix_test::read_body_json(assigned_response).await;
    assert_eq!(assigned_body.as_array().map(Vec::len), Some(1));
    assert_eq!(
        assigned_body[0]["face_id"].as_str(),
        Some(assigned_face.to_string().as_str())
    );
    assert_eq!(
        assigned_body[0]["asset_id"].as_str(),
        Some(assigned_asset.public_id.to_string().as_str())
    );
    assert_eq!(assigned_body[0]["chip_available"].as_bool(), Some(true));

    let chip_request = actix_test::TestRequest::get()
        .uri(&format!("/people/faces/{assigned_face}/chip"))
        .cookie(Cookie::new(
            "mirror_session",
            session.token.expose().to_owned(),
        ))
        .to_request();
    let chip_response = actix_test::call_service(&app, chip_request).await;
    assert_eq!(chip_response.status(), StatusCode::OK);
    assert_eq!(
        chip_response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok()),
        Some("image/webp")
    );
    let chip_body = actix_test::read_body(chip_response).await;
    assert_eq!(&chip_body[..], b"chip-webp");

    let unassigned_request = actix_test::TestRequest::get()
        .uri("/people/faces/unassigned?limit=5")
        .cookie(Cookie::new(
            "mirror_session",
            session.token.expose().to_owned(),
        ))
        .to_request();
    let unassigned_response = actix_test::call_service(&app, unassigned_request).await;
    assert_eq!(unassigned_response.status(), StatusCode::OK);
    let unassigned_body: Value = actix_test::read_body_json(unassigned_response).await;
    assert_eq!(unassigned_body.as_array().map(Vec::len), Some(1));
    assert_eq!(
        unassigned_body[0]["face_id"].as_str(),
        Some(unassigned_face.to_string().as_str())
    );
    assert_eq!(
        unassigned_body[0]["asset_id"].as_str(),
        Some(unassigned_asset.public_id.to_string().as_str())
    );
    Ok(())
}

fn face_embedding_manifest() -> ModelPackManifest {
    ModelPackManifest {
        kind: "face_embedding".to_owned(),
        runtime: "onnx".to_owned(),
        model_key: "test-face-embedding".to_owned(),
        model_revision: format!("face-{}", Uuid::now_v7()),
        license: "Apache-2.0".to_owned(),
        embedding_dimension: 3,
        distance_metric: "cosine".to_owned(),
        onnx: valid_onnx_config(),
        image_preprocess: valid_image_preprocess(),
        face_detection: Some(valid_face_detection_config()),
        face_embedding: Some(valid_face_embedding_config()),
        files: vec![
            ModelPackFileManifest {
                path: "models/image_encoder.onnx".to_owned(),
                sha256: "a".repeat(64),
                size_bytes: 10,
            },
            ModelPackFileManifest {
                path: "models/text_encoder.onnx".to_owned(),
                sha256: "b".repeat(64),
                size_bytes: 10,
            },
            ModelPackFileManifest {
                path: "tokenizer/tokenizer.json".to_owned(),
                sha256: "c".repeat(64),
                size_bytes: 10,
            },
            ModelPackFileManifest {
                path: "models/face_detector.onnx".to_owned(),
                sha256: "e".repeat(64),
                size_bytes: 10,
            },
            ModelPackFileManifest {
                path: "models/face_embedding.onnx".to_owned(),
                sha256: "f".repeat(64),
                size_bytes: 10,
            },
        ],
        self_tests: vec![ModelPackSelfTestManifest {
            name: "face_fixture".to_owned(),
            input_path: "fixtures/face.jpg".to_owned(),
            expected_output_sha256: "d".repeat(64),
        }],
    }
}

fn face_identity_manifest() -> ModelPackManifest {
    let mut manifest = face_embedding_manifest();
    manifest.kind = "face_identity".to_owned();
    manifest.model_key = "test-face-identity".to_owned();
    manifest.model_revision = format!("face-identity-{}", Uuid::now_v7());
    manifest
}

fn fake_people_face_runtime(embedding: Vec<f32>) -> SharedFaceRuntime {
    Arc::new(FakeFaceRuntime { embedding })
}

struct FakeFaceRuntime {
    embedding: Vec<f32>,
}

impl FaceRuntime for FakeFaceRuntime {
    fn detect_and_embed(
        &self,
        _request: FaceRuntimeRequest<'_>,
    ) -> Result<Vec<IndexedFace>, mirror_backend::face::FaceIndexError> {
        Ok(vec![IndexedFace {
            bbox: FaceBox {
                left: 0.1,
                top: 0.2,
                width: 0.3,
                height: 0.4,
            },
            quality: Some(0.95),
            embedding: self.embedding.clone(),
            chip: None,
        }])
    }
}

async fn insert_face_occurrence(
    pool: &sqlx::PgPool,
    asset_id: Uuid,
    review_state: &str,
    left: f32,
) -> TestResult<Uuid> {
    let face_id = Uuid::now_v7();
    sqlx::query(
        r#"
        INSERT INTO face_occurrences (
            id, asset_id, owner_id, detection_model_pack_id,
            bbox_left, bbox_top, bbox_width, bbox_height, quality, review_state
        )
        VALUES ($1, $2, 1, NULL, $3, 0.20, 0.30, 0.40, 0.90, $4)
        "#,
    )
    .bind(face_id)
    .bind(asset_id)
    .bind(left)
    .bind(review_state)
    .execute(pool)
    .await?;
    Ok(face_id)
}

async fn insert_asset(pool: &sqlx::PgPool) -> TestResult<Uuid> {
    let bytes = Uuid::now_v7().as_bytes().to_vec();
    let hash = blake3::hash(&bytes).to_hex().to_string();
    let original_id = Uuid::now_v7();
    let asset_id = Uuid::now_v7();
    let public_id = Uuid::now_v7();
    sqlx::query(
        r#"
        INSERT INTO originals (id, blake3_hash, storage_key, size_bytes, media_type)
        VALUES ($1, $2, $3, $4, 'image/jpeg')
        "#,
    )
    .bind(original_id)
    .bind(&hash)
    .bind(format!("originals/blake3/{hash}"))
    .bind(i64::try_from(bytes.len())?)
    .execute(pool)
    .await?;
    sqlx::query(
        r#"
        INSERT INTO assets (id, public_id, owner_id, original_id)
        VALUES ($1, $2, 1, $3)
        "#,
    )
    .bind(asset_id)
    .bind(public_id)
    .bind(original_id)
    .execute(pool)
    .await?;
    Ok(asset_id)
}
