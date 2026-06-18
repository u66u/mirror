//! Semantic asset embedding index.
//!
//! This is a Mirror use-case boundary over Postgres/pgvector, not a generic
//! vector database abstraction. Callers deal in assets, model packs, and
//! validated embeddings.

use pgvector::Vector;
use sqlx::{PgPool, Postgres, Transaction};
use thiserror::Error;
use uuid::Uuid;

use crate::models::{DistanceMetric, ModelPackKind, ValidatedEmbedding};

/// Semantic search hit for an active owner asset.
#[derive(Debug, Clone, PartialEq)]
pub struct SemanticSearchHit {
    /// Public asset ID.
    pub asset_public_id: Uuid,
    /// Ranking score where higher is better for all supported metrics.
    pub score: f64,
}

/// Search execution knobs for semantic pgvector queries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SemanticSearchOptions {
    /// Use the ANN-compatible expression query path.
    pub use_ann: bool,
    /// `hnsw.ef_search` breadth for ANN scans.
    pub ann_ef_search: i32,
}

impl SemanticSearchOptions {
    /// Exact-search defaults. ANN stays opt-in.
    #[must_use]
    pub const fn exact() -> Self {
        Self {
            use_ann: false,
            ann_ef_search: 40,
        }
    }

    /// ANN query path with pgvector's conservative default scan breadth.
    #[must_use]
    pub const fn ann() -> Self {
        Self {
            use_ann: true,
            ann_ef_search: 40,
        }
    }
}

/// Created/desired HNSW index metadata for one semantic model pack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticAnnIndexSpec {
    /// Stable generated index name.
    pub index_name: String,
    /// pgvector operator class selected from the model pack metric.
    pub operator_class: &'static str,
    /// SQL used to create the index.
    pub ddl: String,
}

/// Semantic index failure.
#[derive(Debug, Error)]
pub enum SemanticIndexError {
    /// Model pack does not exist or is not a semantic image/text pack.
    #[error("semantic model pack is invalid")]
    InvalidModelPack,
    /// Embedding length does not match the model pack dimension.
    #[error("semantic embedding dimension mismatch")]
    DimensionMismatch,
    /// Query limit is outside the accepted range.
    #[error("semantic search limit is invalid")]
    InvalidLimit,
    /// Asset is unavailable for indexing.
    #[error("semantic asset is unavailable")]
    AssetUnavailable,
    /// Database failed.
    #[error("semantic index database error: {0}")]
    Database(#[from] sqlx::Error),
}

/// Inserts or replaces an asset embedding for one model pack.
pub async fn upsert_asset_embedding(
    pool: &PgPool,
    asset_id: Uuid,
    model_pack_id: Uuid,
    embedding: &ValidatedEmbedding,
) -> Result<(), SemanticIndexError> {
    let spec = semantic_model_spec(pool, model_pack_id).await?;
    let mut tx = pool.begin().await?;
    upsert_asset_embedding_with_dimension_in_tx(
        &mut tx,
        asset_id,
        model_pack_id,
        embedding,
        spec.embedding_dimension,
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

/// Transaction-compatible embedding upsert.
pub async fn upsert_asset_embedding_with_dimension_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    asset_id: Uuid,
    model_pack_id: Uuid,
    embedding: &ValidatedEmbedding,
    embedding_dimension: i32,
) -> Result<(), SemanticIndexError> {
    check_dimension(embedding, embedding_dimension)?;
    let vector = Vector::from(embedding.values().to_vec());

    let affected = sqlx::query!(
        r#"
        INSERT INTO asset_embeddings (
            asset_id,
            model_pack_id,
            owner_id,
            asset_public_id,
            asset_created_at,
            asset_trashed_at,
            embedding,
            embedding_dimension
        )
        SELECT
            a.id,
            $2,
            a.owner_id,
            a.public_id,
            a.created_at,
            a.trashed_at,
            $3,
            $4
        FROM assets a
        WHERE a.id = $1
        ON CONFLICT (asset_id, model_pack_id)
        DO UPDATE SET
            owner_id = EXCLUDED.owner_id,
            asset_public_id = EXCLUDED.asset_public_id,
            asset_created_at = EXCLUDED.asset_created_at,
            asset_trashed_at = EXCLUDED.asset_trashed_at,
            embedding = EXCLUDED.embedding,
            embedding_dimension = EXCLUDED.embedding_dimension,
            updated_at = now()
        "#,
        asset_id,
        model_pack_id,
        vector as _,
        embedding_dimension
    )
    .execute(&mut **tx)
    .await?
    .rows_affected();

    if affected == 0 {
        return Err(SemanticIndexError::AssetUnavailable);
    }

    Ok(())
}

/// Deletes all embeddings for one asset.
pub async fn delete_asset_embeddings(
    pool: &PgPool,
    asset_id: Uuid,
) -> Result<u64, SemanticIndexError> {
    let deleted = sqlx::query!("DELETE FROM asset_embeddings WHERE asset_id = $1", asset_id)
        .execute(pool)
        .await?
        .rows_affected();
    Ok(deleted)
}

/// Searches active owner assets in one semantic model space.
pub async fn semantic_search(
    pool: &PgPool,
    owner_id: i16,
    model_pack_id: Uuid,
    query_embedding: &ValidatedEmbedding,
    limit: i64,
) -> Result<Vec<SemanticSearchHit>, SemanticIndexError> {
    semantic_search_with_options(
        pool,
        owner_id,
        model_pack_id,
        query_embedding,
        limit,
        SemanticSearchOptions::exact(),
    )
    .await
}

/// Searches active owner assets with exact or opt-in ANN-compatible ordering.
pub async fn semantic_search_with_options(
    pool: &PgPool,
    owner_id: i16,
    model_pack_id: Uuid,
    query_embedding: &ValidatedEmbedding,
    limit: i64,
    options: SemanticSearchOptions,
) -> Result<Vec<SemanticSearchHit>, SemanticIndexError> {
    if !(1..=200).contains(&limit) {
        return Err(SemanticIndexError::InvalidLimit);
    }

    let spec = semantic_model_spec(pool, model_pack_id).await?;
    check_dimension(query_embedding, spec.embedding_dimension)?;

    let vector = Vector::from(query_embedding.values().to_vec());
    let rows = if options.use_ann {
        semantic_search_ann(pool, owner_id, model_pack_id, &vector, limit, spec, options).await?
    } else {
        semantic_search_exact(pool, owner_id, model_pack_id, &vector, limit, spec).await?
    };

    Ok(rows
        .into_iter()
        .map(|(asset_public_id, raw_rank)| SemanticSearchHit {
            asset_public_id,
            score: score_from_pgvector_rank(spec.distance_metric, raw_rank),
        })
        .collect())
}

/// Creates the per-model-pack HNSW expression index needed by the ANN path.
///
/// The index is partial by model pack and active asset state. Owner filtering is
/// still a normal SQL predicate, backed by `asset_embeddings_owner_model_active_idx`.
pub async fn ensure_semantic_ann_index(
    pool: &PgPool,
    model_pack_id: Uuid,
) -> Result<SemanticAnnIndexSpec, SemanticIndexError> {
    let spec = semantic_model_spec(pool, model_pack_id).await?;
    let index = semantic_ann_index_spec(model_pack_id, spec);
    sqlx::query(&index.ddl).execute(pool).await?;
    Ok(index)
}

async fn semantic_search_exact(
    pool: &PgPool,
    owner_id: i16,
    model_pack_id: Uuid,
    vector: &Vector,
    limit: i64,
    spec: SemanticModelSpec,
) -> Result<Vec<(Uuid, f64)>, SemanticIndexError> {
    let rows: Vec<(Uuid, f64)> = match spec.distance_metric {
        DistanceMetric::Cosine => sqlx::query!(
            r#"
                SELECT asset_public_id, (embedding <=> $3) AS "raw_rank!"
                FROM asset_embeddings
                WHERE model_pack_id = $1
                  AND owner_id = $2
                  AND asset_trashed_at IS NULL
                ORDER BY embedding <=> $3 ASC, asset_created_at DESC, asset_public_id ASC
                LIMIT $4
                "#,
            model_pack_id,
            owner_id,
            vector as _,
            limit
        )
        .fetch_all(pool)
        .await?
        .into_iter()
        .map(|r| (r.asset_public_id, r.raw_rank))
        .collect(),
        DistanceMetric::L2 => sqlx::query!(
            r#"
                SELECT asset_public_id, (embedding <-> $3) AS "raw_rank!"
                FROM asset_embeddings
                WHERE model_pack_id = $1
                  AND owner_id = $2
                  AND asset_trashed_at IS NULL
                ORDER BY embedding <-> $3 ASC, asset_created_at DESC, asset_public_id ASC
                LIMIT $4
                "#,
            model_pack_id,
            owner_id,
            vector as _,
            limit
        )
        .fetch_all(pool)
        .await?
        .into_iter()
        .map(|r| (r.asset_public_id, r.raw_rank))
        .collect(),
        DistanceMetric::Dot => {
            // Note: Dot product metric in pgvector is '<#>' but ranks ascending.
            // Pgvector defines <#> as negative inner product to allow ascending sort.
            sqlx::query!(
                r#"
                SELECT asset_public_id, (embedding <#> $3) AS "raw_rank!"
                FROM asset_embeddings
                WHERE model_pack_id = $1
                  AND owner_id = $2
                  AND asset_trashed_at IS NULL
                ORDER BY embedding <#> $3 ASC, asset_created_at DESC, asset_public_id ASC
                LIMIT $4
                "#,
                model_pack_id,
                owner_id,
                vector as _,
                limit
            )
            .fetch_all(pool)
            .await?
            .into_iter()
            .map(|r| (r.asset_public_id, r.raw_rank))
            .collect()
        }
    };

    Ok(rows)
}

async fn semantic_search_ann(
    pool: &PgPool,
    owner_id: i16,
    model_pack_id: Uuid,
    vector: &Vector,
    limit: i64,
    spec: SemanticModelSpec,
    options: SemanticSearchOptions,
) -> Result<Vec<(Uuid, f64)>, SemanticIndexError> {
    let ef_search = options.ann_ef_search.clamp(1, 1_000).to_string();
    let mut tx = pool.begin().await?;
    sqlx::query_scalar::<_, String>("SELECT set_config('hnsw.ef_search', $1, true)")
        .bind(ef_search)
        .fetch_one(&mut *tx)
        .await?;

    let dimension = spec.embedding_dimension;
    let operator = distance_operator(spec.distance_metric);
    let sql = format!(
        r#"
        SELECT
            asset_public_id,
            (embedding::vector({dimension}) {operator} $1::vector({dimension}))::float8 AS raw_rank
        FROM asset_embeddings
        WHERE model_pack_id = $2
          AND owner_id = $3
          AND asset_trashed_at IS NULL
        ORDER BY
            embedding::vector({dimension}) {operator} $1::vector({dimension}) ASC,
            asset_created_at DESC,
            asset_public_id ASC
        LIMIT $4
        "#
    );

    let rows = sqlx::query_as::<_, (Uuid, f64)>(&sql)
        .bind(vector)
        .bind(model_pack_id)
        .bind(owner_id)
        .bind(limit)
        .fetch_all(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(rows)
}

#[derive(Debug, Clone, Copy)]
struct SemanticModelSpec {
    embedding_dimension: i32,
    distance_metric: DistanceMetric,
}

async fn semantic_model_spec(
    pool: &PgPool,
    model_pack_id: Uuid,
) -> Result<SemanticModelSpec, SemanticIndexError> {
    let row = sqlx::query!(
        r#"
        SELECT kind, embedding_dimension, distance_metric
        FROM model_packs
        WHERE id = $1
        "#,
        model_pack_id
    )
    .fetch_optional(pool)
    .await?;

    let Some(row) = row else {
        return Err(SemanticIndexError::InvalidModelPack);
    };

    let kind = row.kind;
    let embedding_dimension = row.embedding_dimension;
    let distance_metric = row.distance_metric;

    if kind != ModelPackKind::SemanticImageText.as_str() {
        return Err(SemanticIndexError::InvalidModelPack);
    }

    if embedding_dimension <= 0 {
        return Err(SemanticIndexError::InvalidModelPack);
    }

    Ok(SemanticModelSpec {
        embedding_dimension,
        distance_metric: DistanceMetric::from_db_str(&distance_metric)
            .ok_or(SemanticIndexError::InvalidModelPack)?,
    })
}

fn check_dimension(
    embedding: &ValidatedEmbedding,
    expected: i32,
) -> Result<(), SemanticIndexError> {
    if usize::try_from(expected).ok() == Some(embedding.values().len()) {
        Ok(())
    } else {
        Err(SemanticIndexError::DimensionMismatch)
    }
}

fn score_from_pgvector_rank(distance_metric: DistanceMetric, raw_rank: f64) -> f64 {
    match distance_metric {
        DistanceMetric::Cosine => 1.0 - raw_rank,
        DistanceMetric::L2 => -raw_rank,
        DistanceMetric::Dot => -raw_rank,
    }
}

fn semantic_ann_index_spec(model_pack_id: Uuid, spec: SemanticModelSpec) -> SemanticAnnIndexSpec {
    let uuid = model_pack_id.simple().to_string();
    let uuid = &uuid[..24];
    let index_name = format!("ae_ann_{}_{}_hnsw_idx", uuid, spec.distance_metric.as_str());
    let operator_class = distance_operator_class(spec.distance_metric);
    let dimension = spec.embedding_dimension;
    let ddl = format!(
        r#"
        CREATE INDEX CONCURRENTLY IF NOT EXISTS {index_name}
        ON asset_embeddings
        USING hnsw ((embedding::vector({dimension})) {operator_class})
        WHERE model_pack_id = '{model_pack_id}'::uuid
          AND asset_trashed_at IS NULL
        "#
    );

    SemanticAnnIndexSpec {
        index_name,
        operator_class,
        ddl,
    }
}

fn distance_operator(distance_metric: DistanceMetric) -> &'static str {
    match distance_metric {
        DistanceMetric::Cosine => "<=>",
        DistanceMetric::L2 => "<->",
        DistanceMetric::Dot => "<#>",
    }
}

fn distance_operator_class(distance_metric: DistanceMetric) -> &'static str {
    match distance_metric {
        DistanceMetric::Cosine => "vector_cosine_ops",
        DistanceMetric::L2 => "vector_l2_ops",
        DistanceMetric::Dot => "vector_ip_ops",
    }
}
