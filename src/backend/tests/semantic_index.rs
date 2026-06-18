use mirror_backend::{
    models::{
        ModelPackFileManifest, ModelPackManifest, ModelPackSelfTestManifest, install_model_pack,
        record_model_pack_self_test, validate_embedding_output,
    },
    semantic_index::{
        SemanticIndexError, delete_asset_embeddings, semantic_search, upsert_asset_embedding,
    },
};
use uuid::Uuid;

mod support;
use support::{TestResult, fresh_owner_pool};

#[tokio::test]
#[ignore = "requires MIRROR_TEST_DATABASE_URL pointing at a dedicated test database with pgvector"]
async fn semantic_search_ranks_active_owner_assets_in_one_model_space() -> TestResult {
    let pool = fresh_owner_pool().await?;
    let manifest = semantic_manifest("cosine");
    let pack = install_model_pack(&pool, manifest.clone()).await?;
    record_model_pack_self_test(&pool, pack.model_pack_id, true, None).await?;
    let near = insert_semantic_asset(&pool, 1, false).await?;
    let far = insert_semantic_asset(&pool, 1, false).await?;
    let trashed = insert_semantic_asset(&pool, 1, true).await?;

    upsert_asset_embedding(
        &pool,
        near.internal_id,
        pack.model_pack_id,
        &validate_embedding_output(&manifest, vec![1.0, 0.0, 0.0])?,
    )
    .await?;
    upsert_asset_embedding(
        &pool,
        far.internal_id,
        pack.model_pack_id,
        &validate_embedding_output(&manifest, vec![0.0, 1.0, 0.0])?,
    )
    .await?;
    upsert_asset_embedding(
        &pool,
        trashed.internal_id,
        pack.model_pack_id,
        &validate_embedding_output(&manifest, vec![1.0, 0.0, 0.0])?,
    )
    .await?;
    let hits = semantic_search(
        &pool,
        1,
        pack.model_pack_id,
        &validate_embedding_output(&manifest, vec![1.0, 0.0, 0.0])?,
        10,
    )
    .await?;

    assert_eq!(
        hits.iter().map(|hit| hit.asset_id).collect::<Vec<_>>(),
        vec![near.public_id, far.public_id]
    );
    assert!(hits[0].score >= hits[1].score);

    Ok(())
}

#[tokio::test]
#[ignore = "requires MIRROR_TEST_DATABASE_URL pointing at a dedicated test database with pgvector"]
async fn semantic_search_dot_metric_returns_positive_similarity_score() -> TestResult {
    let pool = fresh_owner_pool().await?;
    let manifest = semantic_manifest("dot");
    let pack = install_model_pack(&pool, manifest.clone()).await?;
    record_model_pack_self_test(&pool, pack.model_pack_id, true, None).await?;
    let near = insert_semantic_asset(&pool, 1, false).await?;
    let far = insert_semantic_asset(&pool, 1, false).await?;

    upsert_asset_embedding(
        &pool,
        near.internal_id,
        pack.model_pack_id,
        &validate_embedding_output(&manifest, vec![2.0, 0.0, 0.0])?,
    )
    .await?;
    upsert_asset_embedding(
        &pool,
        far.internal_id,
        pack.model_pack_id,
        &validate_embedding_output(&manifest, vec![0.5, 0.0, 0.0])?,
    )
    .await?;

    let hits = semantic_search(
        &pool,
        1,
        pack.model_pack_id,
        &validate_embedding_output(&manifest, vec![1.0, 0.0, 0.0])?,
        10,
    )
    .await?;

    assert_eq!(
        hits.iter().map(|hit| hit.asset_id).collect::<Vec<_>>(),
        vec![near.public_id, far.public_id]
    );
    assert_eq!(hits[0].score, 2.0);
    assert_eq!(hits[1].score, 0.5);

    Ok(())
}

#[tokio::test]
#[ignore = "requires MIRROR_TEST_DATABASE_URL pointing at a dedicated test database with pgvector"]
async fn semantic_index_rejects_wrong_dimensions_and_deletes_asset_embeddings() -> TestResult {
    let pool = fresh_owner_pool().await?;
    let manifest = semantic_manifest("cosine");
    let pack = install_model_pack(&pool, manifest.clone()).await?;
    record_model_pack_self_test(&pool, pack.model_pack_id, true, None).await?;
    let asset = insert_semantic_asset(&pool, 1, false).await?;
    let wrong_manifest = semantic_manifest_with_dimension(2, "cosine");

    assert!(matches!(
        upsert_asset_embedding(
            &pool,
            asset.internal_id,
            pack.model_pack_id,
            &validate_embedding_output(&wrong_manifest, vec![1.0, 0.0])?,
        )
        .await,
        Err(SemanticIndexError::DimensionMismatch)
    ));

    upsert_asset_embedding(
        &pool,
        asset.internal_id,
        pack.model_pack_id,
        &validate_embedding_output(&manifest, vec![1.0, 0.0, 0.0])?,
    )
    .await?;
    assert_eq!(delete_asset_embeddings(&pool, asset.internal_id).await?, 1);
    let hits = semantic_search(
        &pool,
        1,
        pack.model_pack_id,
        &validate_embedding_output(&manifest, vec![1.0, 0.0, 0.0])?,
        10,
    )
    .await?;
    assert!(hits.is_empty());

    Ok(())
}

#[derive(Debug)]
struct SemanticAsset {
    internal_id: Uuid,
    public_id: Uuid,
}

fn semantic_manifest(distance_metric: &str) -> ModelPackManifest {
    semantic_manifest_with_dimension(3, distance_metric)
}

fn semantic_manifest_with_dimension(
    embedding_dimension: i32,
    distance_metric: &str,
) -> ModelPackManifest {
    ModelPackManifest {
        kind: "semantic_image_text".to_owned(),
        runtime: "onnx".to_owned(),
        model_key: "test-semantic".to_owned(),
        model_revision: format!("dim-{embedding_dimension}-{distance_metric}"),
        license: "Apache-2.0".to_owned(),
        embedding_dimension,
        distance_metric: distance_metric.to_owned(),
        files: vec![ModelPackFileManifest {
            path: "models/image_encoder.onnx".to_owned(),
            sha256: "a".repeat(64),
            size_bytes: 10,
        }],
        self_tests: vec![ModelPackSelfTestManifest {
            name: "embedding_fixture".to_owned(),
            input_path: "fixtures/photo.jpg".to_owned(),
            expected_output_sha256: "b".repeat(64),
        }],
    }
}

async fn insert_semantic_asset(
    pool: &sqlx::PgPool,
    owner_id: i16,
    trashed: bool,
) -> TestResult<SemanticAsset> {
    let bytes = Uuid::now_v7().as_bytes().to_vec();
    let hash = blake3::hash(&bytes).to_hex().to_string();
    let original_id = Uuid::now_v7();
    let internal_id = Uuid::now_v7();
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
        INSERT INTO assets (id, public_id, owner_id, original_id, trashed_at)
        VALUES ($1, $2, $3, $4, CASE WHEN $5 THEN now() ELSE NULL END)
        "#,
    )
    .bind(internal_id)
    .bind(public_id)
    .bind(owner_id)
    .bind(original_id)
    .bind(trashed)
    .execute(pool)
    .await?;

    Ok(SemanticAsset {
        internal_id,
        public_id,
    })
}
