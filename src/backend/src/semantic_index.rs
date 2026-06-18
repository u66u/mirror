//! Semantic asset embedding index.
//!
//! This is a Mirror use-case boundary over Postgres/pgvector, not a generic
//! vector database abstraction. Callers deal in assets, model packs, and
//! validated embeddings.

use pgvector::Vector;
use sqlx::PgPool;
use uuid::Uuid;

use crate::models::{DistanceMetric, ModelPackKind, ValidatedEmbedding};

/// Semantic search hit for an active owner asset.
#[derive(Debug, Clone, PartialEq)]
pub struct SemanticSearchHit {
    /// Public asset ID.
    pub asset_id: Uuid,
    /// Ranking score where higher is better for all supported metrics.
    pub score: f64,
}

/// Semantic index failure.
#[derive(Debug)]
pub enum SemanticIndexError {
    /// Model pack does not exist or is not a semantic image/text pack.
    InvalidModelPack,
    /// Embedding length does not match the model pack dimension.
    DimensionMismatch,
    /// Query limit is outside the accepted range.
    InvalidLimit,
    /// Database failed.
    Database(sqlx::Error),
}

impl std::fmt::Display for SemanticIndexError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::InvalidModelPack => "semantic model pack is invalid",
            Self::DimensionMismatch => "semantic embedding dimension mismatch",
            Self::InvalidLimit => "semantic search limit is invalid",
            Self::Database(_) => "semantic index database error",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for SemanticIndexError {}

impl From<sqlx::Error> for SemanticIndexError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

/// Inserts or replaces an asset embedding for one model pack.
pub async fn upsert_asset_embedding(
    pool: &PgPool,
    asset_id: Uuid,
    model_pack_id: Uuid,
    embedding: &ValidatedEmbedding,
) -> Result<(), SemanticIndexError> {
    let spec = semantic_model_spec(pool, model_pack_id).await?;
    upsert_asset_embedding_with_dimension(
        pool,
        asset_id,
        model_pack_id,
        embedding,
        spec.embedding_dimension,
    )
    .await
}

/// Inserts or replaces an asset embedding when caller already loaded the
/// semantic model metadata in the same workflow.
pub async fn upsert_asset_embedding_with_dimension(
    pool: &PgPool,
    asset_id: Uuid,
    model_pack_id: Uuid,
    embedding: &ValidatedEmbedding,
    embedding_dimension: i32,
) -> Result<(), SemanticIndexError> {
    check_dimension(embedding, embedding_dimension)?;
    let vector = Vector::from(embedding.values().to_vec());

    sqlx::query(
        r#"
        INSERT INTO asset_embeddings (
            asset_id,
            model_pack_id,
            embedding,
            embedding_dimension
        )
        VALUES ($1, $2, $3, $4)
        ON CONFLICT (asset_id, model_pack_id)
        DO UPDATE SET
            embedding = EXCLUDED.embedding,
            embedding_dimension = EXCLUDED.embedding_dimension,
            updated_at = now()
        "#,
    )
    .bind(asset_id)
    .bind(model_pack_id)
    .bind(vector)
    .bind(embedding_dimension)
    .execute(pool)
    .await?;

    Ok(())
}

/// Deletes all embeddings for one asset.
pub async fn delete_asset_embeddings(
    pool: &PgPool,
    asset_id: Uuid,
) -> Result<u64, SemanticIndexError> {
    let deleted = sqlx::query("DELETE FROM asset_embeddings WHERE asset_id = $1")
        .bind(asset_id)
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
    if !(1..=200).contains(&limit) {
        return Err(SemanticIndexError::InvalidLimit);
    }
    let spec = semantic_model_spec(pool, model_pack_id).await?;
    check_dimension(query_embedding, spec.embedding_dimension)?;
    let vector = Vector::from(query_embedding.values().to_vec());
    let order = match spec.distance_metric {
        DistanceMetric::Cosine => "<=>",
        DistanceMetric::L2 => "<->",
        DistanceMetric::Dot => "<#>",
    };
    // pgvector `<#>` returns negative inner product. Keep SQL ordering on the
    // raw pgvector rank ascending, then convert to Mirror score after fetch.
    let sql = format!(
        r#"
        SELECT assets.public_id, asset_embeddings.embedding {order} $3 AS raw_rank
        FROM asset_embeddings
        JOIN assets ON assets.id = asset_embeddings.asset_id
        WHERE asset_embeddings.model_pack_id = $1
          AND assets.owner_id = $2
          AND assets.trashed_at IS NULL
        ORDER BY raw_rank ASC, assets.created_at DESC, assets.public_id ASC
        LIMIT $4
        "#
    );
    let rows = sqlx::query_as::<_, (Uuid, f64)>(&sql)
        .bind(model_pack_id)
        .bind(owner_id)
        .bind(vector)
        .bind(limit)
        .fetch_all(pool)
        .await?;

    Ok(rows
        .into_iter()
        .map(|(asset_id, raw_rank)| SemanticSearchHit {
            asset_id,
            score: score_from_pgvector_rank(spec.distance_metric, raw_rank),
        })
        .collect())
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
    let row = sqlx::query_as::<_, (String, i32, String)>(
        r#"
        SELECT kind, embedding_dimension, distance_metric
        FROM model_packs
        WHERE id = $1
        "#,
    )
    .bind(model_pack_id)
    .fetch_optional(pool)
    .await?;
    let Some((kind, embedding_dimension, distance_metric)) = row else {
        return Err(SemanticIndexError::InvalidModelPack);
    };
    if kind != ModelPackKind::SemanticImageText.as_str() {
        return Err(SemanticIndexError::InvalidModelPack);
    }
    Ok(SemanticModelSpec {
        embedding_dimension,
        distance_metric: parse_semantic_distance_metric(&distance_metric)?,
    })
}

fn parse_semantic_distance_metric(value: &str) -> Result<DistanceMetric, SemanticIndexError> {
    match value {
        "cosine" => Ok(DistanceMetric::Cosine),
        "dot" => Ok(DistanceMetric::Dot),
        "l2" => Ok(DistanceMetric::L2),
        _ => Err(SemanticIndexError::InvalidModelPack),
    }
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
        DistanceMetric::Cosine | DistanceMetric::L2 => -raw_rank,
        DistanceMetric::Dot => -raw_rank,
    }
}
