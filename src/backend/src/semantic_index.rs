//! Semantic asset embedding index.
//!
//! This is a Mirror use-case boundary over Postgres/pgvector, not a generic
//! vector database abstraction. Callers deal in assets, model packs, and
//! validated embeddings.

use pgvector::Vector;
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;
use thiserror::Error;

use crate::models::{DistanceMetric, ModelPackKind, ValidatedEmbedding};

/// Semantic search hit for an active owner asset.
#[derive(Debug, Clone, PartialEq)]
pub struct SemanticSearchHit {
    /// Public asset ID.
    pub asset_public_id: Uuid,
    /// Ranking score where higher is better for all supported metrics.
    pub score: f64,
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

    let affected = sqlx::query(
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
    )
    .bind(asset_id)
    .bind(model_pack_id)
    .bind(vector)
    .bind(embedding_dimension)
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

    let sql = format!(
        r#"
        SELECT asset_public_id, embedding {order} $3 AS raw_rank
        FROM asset_embeddings
        WHERE model_pack_id = $1
          AND owner_id = $2
          AND asset_trashed_at IS NULL
        ORDER BY raw_rank ASC, asset_created_at DESC, asset_public_id ASC
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
        .map(|(asset_public_id, raw_rank)| SemanticSearchHit {
            asset_public_id,
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
