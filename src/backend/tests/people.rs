use mirror_backend::models::{
    ModelPackFileManifest, ModelPackManifest, ModelPackSelfTestManifest, install_model_pack,
};
use pgvector::Vector;
use uuid::Uuid;

mod support;
use support::{TestResult, fresh_owner_pool, valid_image_preprocess, valid_onnx_config};

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
        ],
        self_tests: vec![ModelPackSelfTestManifest {
            name: "face_fixture".to_owned(),
            input_path: "fixtures/face.jpg".to_owned(),
            expected_output_sha256: "d".repeat(64),
        }],
    }
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
