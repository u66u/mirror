//! ML job handling.
//!
//! Runtime-specific code belongs behind `ImageTextEmbedder`. This module owns
//! Mirror side effects: load original, validate output, store embedding, update
//! reindex progress.

use serde_json::Value;
use sqlx::types::Json;
use uuid::Uuid;

use crate::{
    jobs::{JobKind, LeasedJob},
    models::{
        ModelPackError, ModelPackManifest, record_reindex_asset_result, validate_embedding_output,
    },
    semantic_index::{self, SemanticIndexError},
    storage::{ObjectStorage, StorageError, StorageKey, StorageKeyError},
};

const MAX_TEXT_QUERY_CHARS: usize = 512;

/// Image embedding request passed to the configured runtime.
pub struct EmbedImageRequest<'a> {
    /// Original bytes for supported image media.
    pub bytes: &'a [u8],
    /// Original media type.
    pub media_type: &'a str,
    /// Model-pack manifest from the database.
    pub manifest: &'a ModelPackManifest,
}

/// Text embedding request passed to the configured runtime.
pub struct EmbedTextRequest<'a> {
    /// Owner query after boundary validation.
    pub text: &'a str,
    /// Model-pack manifest from the database.
    pub manifest: &'a ModelPackManifest,
}

/// Task-level image/text embedding boundary.
pub trait ImageTextEmbedder {
    /// Embeds one image into the model pack's shared image/text vector space.
    fn embed_image(&self, request: EmbedImageRequest<'_>) -> Result<Vec<f32>, MlError>;

    /// Embeds one text query into the model pack's shared image/text vector space.
    fn embed_text(&self, request: EmbedTextRequest<'_>) -> Result<Vec<f32>, MlError>;
}

/// ML job failure.
#[derive(Debug)]
pub enum MlError {
    /// Job kind belongs to another worker path.
    UnsupportedJobKind,
    /// Job payload is missing required UUID fields.
    InvalidJobPayload,
    /// Text query is empty or too large.
    InvalidTextQuery,
    /// Asset or model pack no longer exists.
    NotFound,
    /// V1 worker only embeds still images.
    UnsupportedMediaType,
    /// Worker started without a configured embedding runtime.
    RuntimeUnavailable,
    /// Original storage key is invalid.
    InvalidStorageKey(StorageKeyError),
    /// Object storage failed.
    Storage(StorageError),
    /// Model-pack state or runtime output is invalid.
    Model(ModelPackError),
    /// Semantic index update failed.
    SemanticIndex(SemanticIndexError),
    /// Database failed.
    Database(sqlx::Error),
}

impl std::fmt::Display for MlError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::UnsupportedJobKind => "unsupported ml job kind",
            Self::InvalidJobPayload => "invalid ml job payload",
            Self::InvalidTextQuery => "invalid ml text query",
            Self::NotFound => "ml asset or model pack not found",
            Self::UnsupportedMediaType => "unsupported ml media type",
            Self::RuntimeUnavailable => "ml runtime unavailable",
            Self::InvalidStorageKey(_) => "invalid ml storage key",
            Self::Storage(_) => "ml storage error",
            Self::Model(_) => "ml model error",
            Self::SemanticIndex(_) => "ml semantic index error",
            Self::Database(_) => "ml database error",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for MlError {}

impl From<ModelPackError> for MlError {
    fn from(error: ModelPackError) -> Self {
        Self::Model(error)
    }
}

impl From<SemanticIndexError> for MlError {
    fn from(error: SemanticIndexError) -> Self {
        Self::SemanticIndex(error)
    }
}

/// Runs one ML job side effect.
pub async fn run_ml_job<E>(
    pool: &sqlx::PgPool,
    storage: &ObjectStorage,
    embedder: &E,
    job: &LeasedJob,
) -> Result<(), MlError>
where
    E: ImageTextEmbedder,
{
    if job.kind != JobKind::EmbedAsset {
        return Err(MlError::UnsupportedJobKind);
    }

    let payload = EmbedAssetPayload::from_json(&job.payload)?;
    let asset = load_embed_asset(pool, payload.asset_id, payload.model_pack_id).await?;
    if !matches!(
        asset.media_type.as_str(),
        "image/jpeg" | "image/png" | "image/gif" | "image/webp"
    ) {
        return Err(MlError::UnsupportedMediaType);
    }

    let bytes = storage
        .read(&asset.storage_key()?)
        .await
        .map_err(MlError::Storage)?;
    let values = embedder.embed_image(EmbedImageRequest {
        bytes: &bytes,
        media_type: &asset.media_type,
        manifest: &asset.manifest,
    })?;
    let embedding = validate_embedding_output(&asset.manifest, values)?;
    semantic_index::upsert_asset_embedding_with_dimension(
        pool,
        payload.asset_id,
        payload.model_pack_id,
        &embedding,
        asset.manifest.embedding_dimension,
    )
    .await?;
    record_reindex_asset_result(pool, payload.reindex_run_id, payload.asset_id, true, None).await?;
    Ok(())
}

/// Embeds an owner text query and searches active assets through pgvector.
pub async fn semantic_text_search<E>(
    pool: &sqlx::PgPool,
    embedder: &E,
    owner_id: i16,
    query: &str,
    limit: i64,
) -> Result<Vec<semantic_index::SemanticSearchHit>, MlError>
where
    E: ImageTextEmbedder,
{
    let query = valid_text_query(query)?;
    let pack = active_semantic_model_pack(pool).await?;
    let values = embedder.embed_text(EmbedTextRequest {
        text: query,
        manifest: &pack.manifest,
    })?;
    let embedding = validate_embedding_output(&pack.manifest, values)?;
    semantic_index::semantic_search(pool, owner_id, pack.model_pack_id, &embedding, limit)
        .await
        .map_err(MlError::SemanticIndex)
}

struct EmbedAssetPayload {
    asset_id: Uuid,
    model_pack_id: Uuid,
    reindex_run_id: Uuid,
}

struct ActiveSemanticModelPack {
    model_pack_id: Uuid,
    manifest: ModelPackManifest,
}

impl EmbedAssetPayload {
    fn from_json(value: &Value) -> Result<Self, MlError> {
        Ok(Self {
            asset_id: uuid_field(value, "asset_id")?,
            model_pack_id: uuid_field(value, "model_pack_id")?,
            reindex_run_id: uuid_field(value, "reindex_run_id")?,
        })
    }
}

struct EmbedAsset {
    storage_key: String,
    media_type: String,
    manifest: ModelPackManifest,
}

impl EmbedAsset {
    fn storage_key(&self) -> Result<StorageKey, MlError> {
        StorageKey::new(&self.storage_key).map_err(MlError::InvalidStorageKey)
    }
}

async fn load_embed_asset(
    pool: &sqlx::PgPool,
    asset_id: Uuid,
    model_pack_id: Uuid,
) -> Result<EmbedAsset, MlError> {
    let row = sqlx::query_as::<_, (String, String, Json<ModelPackManifest>)>(
        r#"
        SELECT o.storage_key, o.media_type, mp.manifest
        FROM assets a
        JOIN originals o ON o.id = a.original_id
        JOIN model_packs mp ON mp.id = $2
        WHERE a.id = $1
          AND a.trashed_at IS NULL
          AND mp.kind = 'semantic_image_text'
        "#,
    )
    .bind(asset_id)
    .bind(model_pack_id)
    .fetch_optional(pool)
    .await
    .map_err(MlError::Database)?;

    row.map(|(storage_key, media_type, manifest)| EmbedAsset {
        storage_key,
        media_type,
        manifest: manifest.0,
    })
    .ok_or(MlError::NotFound)
}

async fn active_semantic_model_pack(
    pool: &sqlx::PgPool,
) -> Result<ActiveSemanticModelPack, MlError> {
    let row = sqlx::query_as::<_, (Uuid, Json<ModelPackManifest>)>(
        r#"
        SELECT id, manifest
        FROM model_packs
        WHERE kind = 'semantic_image_text'
          AND status = 'active'
          AND self_test_status = 'passed'
        "#,
    )
    .fetch_optional(pool)
    .await
    .map_err(MlError::Database)?;

    row.map(|(model_pack_id, manifest)| ActiveSemanticModelPack {
        model_pack_id,
        manifest: manifest.0,
    })
    .ok_or(MlError::NotFound)
}

fn uuid_field(value: &Value, field: &str) -> Result<Uuid, MlError> {
    value
        .get(field)
        .and_then(Value::as_str)
        .and_then(|raw| Uuid::parse_str(raw).ok())
        .ok_or(MlError::InvalidJobPayload)
}

fn valid_text_query(query: &str) -> Result<&str, MlError> {
    let trimmed = query.trim();
    if trimmed.is_empty() || trimmed.chars().count() > MAX_TEXT_QUERY_CHARS {
        Err(MlError::InvalidTextQuery)
    } else {
        Ok(trimmed)
    }
}
