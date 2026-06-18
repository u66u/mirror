//! ML job handling.
//!
//! Runtime-specific code belongs behind `ImageTextEmbedder`. This module owns
//! Mirror side effects: load original, validate output, store embedding, update
//! reindex progress.

use std::{num::NonZeroUsize, sync::Arc};
use thiserror::Error;

use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Postgres, Transaction, types::Json};
use tokio::sync::Semaphore;
use uuid::Uuid;

use crate::{
    jobs::{JobKind, LeasedJob},
    models::{
        ModelPackError, ModelPackKind, ModelPackManifest, ValidatedEmbedding,
        record_reindex_asset_result, validate_embedding_output, validate_model_pack_manifest,
    },
    semantic_index::{self, SemanticIndexError},
    storage::{ObjectStorage, StorageError, StorageKey, StorageKeyError},
};

const MAX_TEXT_QUERY_CHARS: usize = 512;
const MAX_TEXT_QUERY_BYTES: usize = MAX_TEXT_QUERY_CHARS * 4;

const MAX_REINDEX_ERROR_MESSAGE_CHARS: usize = 1_000;

/// Shared ML runtime wrapper.
///
/// `ImageTextEmbedder` implementations are synchronous because many ML runtimes
/// expose blocking APIs. This wrapper moves those calls onto Tokio's blocking
/// pool and bounds concurrent inference.
#[derive(Clone)]
pub struct MlRuntime<E: ?Sized> {
    embedder: Arc<E>,
    permits: Arc<Semaphore>,
    max_image_bytes: usize,
}

/// Runtime handle suitable for Actix app data.
pub type SharedImageTextRuntime = MlRuntime<dyn ImageTextEmbedder + Send + Sync>;

impl<E: ?Sized> MlRuntime<E> {
    /// Creates a runtime wrapper with bounded concurrent embedding calls.
    pub fn new(embedder: Arc<E>, max_concurrent_embeddings: NonZeroUsize) -> Self {
        Self::with_max_image_bytes(embedder, max_concurrent_embeddings, 25 * 1024 * 1024)
    }

    /// Creates a runtime wrapper with a caller-selected encoded image byte cap.
    pub fn with_max_image_bytes(
        embedder: Arc<E>,
        max_concurrent_embeddings: NonZeroUsize,
        max_image_bytes: usize,
    ) -> Self {
        Self {
            embedder,
            permits: Arc::new(Semaphore::new(max_concurrent_embeddings.get())),
            max_image_bytes: max_image_bytes.max(1),
        }
    }
}

impl<E> MlRuntime<E>
where
    E: ImageTextEmbedder + Send + Sync + 'static + ?Sized,
{
    async fn embed_image(
        &self,
        bytes: Vec<u8>,
        media_type: String,
        manifest: ModelPackManifest,
    ) -> Result<Vec<f32>, MlError> {
        let permit = self
            .permits
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| MlError::RuntimeUnavailable)?;

        let embedder = self.embedder.clone();

        tokio::task::spawn_blocking(move || {
            let _permit = permit;

            embedder.embed_image(EmbedImageRequest {
                bytes: &bytes,
                media_type: &media_type,
                manifest: &manifest,
            })
        })
        .await
        .map_err(|_| MlError::RuntimeUnavailable)?
    }

    async fn embed_text(
        &self,
        text: String,
        manifest: ModelPackManifest,
    ) -> Result<Vec<f32>, MlError> {
        let permit = self
            .permits
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| MlError::RuntimeUnavailable)?;

        let embedder = self.embedder.clone();

        tokio::task::spawn_blocking(move || {
            let _permit = permit;

            embedder.embed_text(EmbedTextRequest {
                text: &text,
                manifest: &manifest,
            })
        })
        .await
        .map_err(|_| MlError::RuntimeUnavailable)?
    }
}

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
#[derive(Debug, Error)]
pub enum MlError {
    /// Job kind belongs to another worker path.
    #[error("unsupported ml job kind")]
    UnsupportedJobKind,
    /// Job payload is missing required UUID fields.
    #[error("invalid ml job payload")]
    InvalidJobPayload,
    /// Text query is empty or too large.
    #[error("invalid ml text query")]
    InvalidTextQuery,
    /// Asset, reindex row, or model pack no longer exists.
    #[error("ml asset, reindex row, or model pack not found")]
    NotFound,
    /// V1 worker only embeds explicitly supported still-image formats.
    #[error("unsupported ml media type")]
    UnsupportedMediaType,
    /// Worker started without a configured embedding runtime, or runtime task panicked.
    #[error("ml runtime unavailable")]
    RuntimeUnavailable,
    /// Encoded original exceeds the embedding runtime boundary.
    #[error("ml image exceeds embedding byte limit")]
    ImageTooLarge,
    /// Original storage key is invalid.
    #[error("invalid ml storage key")]
    InvalidStorageKey(#[from] StorageKeyError),
    /// Object storage failed.
    #[error("ml storage error")]
    Storage(#[from] StorageError),
    /// Model-pack state or runtime output is invalid.
    #[error("ml model error")]
    Model(#[from] ModelPackError),
    /// Semantic index update failed.
    #[error("ml semantic index error")]
    SemanticIndex(#[from] SemanticIndexError),
    /// Database failed.
    #[error("ml database error")]
    Database(#[from] sqlx::Error),
}

impl MlError {
    /// Returns whether the queue should retry this error.
    ///
    /// Permanent asset-level errors are converted into reindex failures by
    /// `run_ml_job`, so the queue should normally only see retryable errors.
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::RuntimeUnavailable
                | Self::Storage(_)
                | Self::Database(_)
                | Self::SemanticIndex(SemanticIndexError::Database(_))
                | Self::Model(ModelPackError::Database(_))
                | Self::Model(ModelPackError::Storage(_))
                | Self::Model(ModelPackError::Io(_))
        )
    }
}

/// Runs one ML job side effect.
///
/// Permanent asset-level failures are recorded as terminal reindex failures and
/// then returned as `Ok(())`, so the queue can mark the job complete instead of
/// retrying unsupported/corrupt inputs forever.
///
/// Transient failures are returned as `Err` so the queue can retry. Once the job
/// dead-letters, the worker loop should call `record_embed_asset_dead_letter_payload`.
pub async fn run_ml_job<E>(
    pool: &PgPool,
    storage: &ObjectStorage,
    runtime: &MlRuntime<E>,
    job: &LeasedJob,
) -> Result<(), MlError>
where
    E: ImageTextEmbedder + Send + Sync + 'static + ?Sized,
{
    if job.kind != JobKind::EmbedAsset {
        return Err(MlError::UnsupportedJobKind);
    }

    let payload = EmbedAssetPayload::from_json(&job.payload)?;

    match run_embed_asset_job(pool, storage, runtime, &payload).await {
        Ok(()) => Ok(()),
        Err(error) if is_terminal_reindex_failure(&error) => {
            let message = reindex_error_message(&error);
            record_reindex_asset_result(
                pool,
                payload.reindex_run_id,
                payload.asset_id,
                false,
                Some(&message),
            )
            .await?;
            Ok(())
        }
        Err(error) => Err(error),
    }
}

/// Records a dead-lettered embed job as a failed reindex asset.
///
/// Call this from the worker loop after `jobs::fail_with_outcome` returns a
/// dead outcome for an `EmbedAsset` job.
pub async fn record_embed_asset_dead_letter_payload(
    pool: &PgPool,
    payload: &Value,
    message: &str,
) -> Result<(), MlError> {
    let payload = EmbedAssetPayload::from_json(payload)?;
    let message = truncate_chars(message, MAX_REINDEX_ERROR_MESSAGE_CHARS);

    record_reindex_asset_result(
        pool,
        payload.reindex_run_id,
        payload.asset_id,
        false,
        Some(&message),
    )
    .await?;

    Ok(())
}

/// Convenience wrapper for dead-lettering a leased embed job.
pub async fn record_embed_asset_dead_letter(
    pool: &PgPool,
    job: &LeasedJob,
    message: &str,
) -> Result<bool, MlError> {
    if job.kind != JobKind::EmbedAsset {
        return Ok(false);
    }

    record_embed_asset_dead_letter_payload(pool, &job.payload, message).await?;
    Ok(true)
}

/// Embeds an owner text query and searches active assets through pgvector.
pub async fn semantic_text_search<E>(
    pool: &PgPool,
    runtime: &MlRuntime<E>,
    owner_id: i16,
    query: &str,
    limit: i64,
) -> Result<Vec<semantic_index::SemanticSearchHit>, MlError>
where
    E: ImageTextEmbedder + Send + Sync + 'static + ?Sized,
{
    let query = valid_text_query(query)?.to_owned();
    let pack = active_semantic_model_pack(pool).await?;

    let values = runtime.embed_text(query, pack.manifest.clone()).await?;
    let embedding = validate_embedding_output(&pack.manifest, values)?;

    semantic_index::semantic_search(pool, owner_id, pack.model_pack_id, &embedding, limit)
        .await
        .map_err(MlError::SemanticIndex)
}

async fn run_embed_asset_job<E>(
    pool: &PgPool,
    storage: &ObjectStorage,
    runtime: &MlRuntime<E>,
    payload: &EmbedAssetPayload,
) -> Result<(), MlError>
where
    E: ImageTextEmbedder + Send + Sync + 'static + ?Sized,
{
    let Some(asset) = load_embed_asset(pool, payload).await? else {
        return Ok(());
    };

    if !is_supported_still_image_media_type(&asset.media_type) {
        return Err(MlError::UnsupportedMediaType);
    }

    let storage_key = asset.storage_key()?;
    let bytes = storage
        .read_bounded(&storage_key, runtime.max_image_bytes)
        .await
        .map_err(|error| match error {
            StorageError::ObjectTooLarge => MlError::ImageTooLarge,
            error => MlError::Storage(error),
        })?;

    let values = runtime
        .embed_image(bytes, asset.media_type.clone(), asset.manifest.clone())
        .await?;

    let embedding = validate_embedding_output(&asset.manifest, values)?;

    commit_embed_asset_success(pool, payload, &embedding, asset.embedding_dimension).await?;

    Ok(())
}

/// Runs a model pack's golden image self-tests with the configured runtime.
pub async fn run_model_pack_self_tests<E>(
    pool: &PgPool,
    storage: &ObjectStorage,
    runtime: &MlRuntime<E>,
    model_pack_id: Uuid,
) -> Result<crate::models::InstalledModelPack, MlError>
where
    E: ImageTextEmbedder + Send + Sync + 'static + ?Sized,
{
    let manifest = load_model_pack_manifest(pool, model_pack_id).await?;
    validate_semantic_model_pack_row(
        &manifest,
        manifest.embedding_dimension,
        &manifest.distance_metric,
    )?;

    for self_test in &manifest.self_tests {
        let result =
            run_one_model_pack_self_test(storage, runtime, model_pack_id, &manifest, self_test)
                .await;
        match result {
            Ok(true) => {}
            Ok(false) => {
                return record_self_test_failure(pool, model_pack_id, "self-test output mismatch")
                    .await;
            }
            Err(error) if self_test_runtime_failure(&error) => {
                return record_self_test_failure(pool, model_pack_id, &error.to_string()).await;
            }
            Err(error) => return Err(error),
        }
    }

    crate::models::record_model_pack_self_test(pool, model_pack_id, true, None)
        .await
        .map_err(MlError::Model)
}

async fn run_one_model_pack_self_test<E>(
    storage: &ObjectStorage,
    runtime: &MlRuntime<E>,
    model_pack_id: Uuid,
    manifest: &ModelPackManifest,
    self_test: &crate::models::ModelPackSelfTestManifest,
) -> Result<bool, MlError>
where
    E: ImageTextEmbedder + Send + Sync + 'static + ?Sized,
{
    let storage_key = StorageKey::model_pack_file(model_pack_id, &self_test.input_path)?;
    let bytes = storage
        .read_bounded(&storage_key, runtime.max_image_bytes)
        .await
        .map_err(|error| match error {
            StorageError::ObjectTooLarge => MlError::ImageTooLarge,
            error => MlError::Storage(error),
        })?;
    let media_type = self_test_media_type(&self_test.input_path)?;
    let values = runtime
        .embed_image(bytes, media_type.to_owned(), manifest.clone())
        .await?;
    let embedding = validate_embedding_output(manifest, values)?;
    Ok(sha256_f32_values(embedding.values()) == self_test.expected_output_sha256)
}

async fn commit_embed_asset_success(
    pool: &PgPool,
    payload: &EmbedAssetPayload,
    embedding: &ValidatedEmbedding,
    embedding_dimension: i32,
) -> Result<(), MlError> {
    let mut tx = pool.begin().await?;

    semantic_index::upsert_asset_embedding_with_dimension_in_tx(
        &mut tx,
        payload.asset_id,
        payload.model_pack_id,
        embedding,
        embedding_dimension,
    )
    .await
    .map_err(MlError::SemanticIndex)?;

    record_reindex_asset_result_in_tx(
        &mut tx,
        payload.reindex_run_id,
        payload.asset_id,
        true,
        None,
    )
    .await?;

    tx.commit().await?;
    Ok(())
}

async fn record_reindex_asset_result_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    reindex_run_id: Uuid,
    asset_id: Uuid,
    succeeded: bool,
    error_message: Option<&str>,
) -> Result<(), MlError> {
    let changed = sqlx::query_scalar!(
        r#"
        UPDATE model_reindex_assets
        SET
            status = CASE WHEN $3 THEN 'done' ELSE 'failed' END,
            error_message = CASE WHEN $3 THEN NULL ELSE $4 END,
            updated_at = now()
        WHERE reindex_run_id = $1
        AND asset_id = $2
        AND status = 'queued'
        RETURNING true AS "changed!"
        "#,
        reindex_run_id,
        asset_id,
        succeeded,
        error_message
    )
    .fetch_optional(&mut **tx)
    .await?
    .unwrap_or(false);

    let exists = sqlx::query_scalar!(
        r#"
        SELECT true AS "exists!"
        FROM model_reindex_assets
        WHERE reindex_run_id = $1
          AND asset_id = $2
        LIMIT 1
        "#,
        reindex_run_id,
        asset_id
    )
    .fetch_optional(&mut **tx)
    .await?
    .unwrap_or(false);

    if !exists {
        return Err(MlError::NotFound);
    }

    if changed {
        sqlx::query!(
            r#"
            UPDATE model_reindex_runs
            SET
                processed_assets = processed_assets + CASE WHEN $2 THEN 1 ELSE 0 END,
                failed_assets = failed_assets + CASE WHEN $2 THEN 0 ELSE 1 END,
                status = CASE
                    WHEN processed_assets + failed_assets + 1 >= total_assets
                         AND failed_assets + CASE WHEN $2 THEN 0 ELSE 1 END = 0
                    THEN 'succeeded'
                    WHEN processed_assets + failed_assets + 1 >= total_assets
                    THEN 'failed'
                    WHEN status = 'queued'
                    THEN 'running'
                    ELSE status
                END,
                updated_at = now()
            WHERE id = $1
            "#,
            reindex_run_id,
            succeeded
        )
        .execute(&mut **tx)
        .await?;
    }

    Ok(())
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
    embedding_dimension: i32,
}

impl EmbedAsset {
    fn storage_key(&self) -> Result<StorageKey, MlError> {
        StorageKey::new(&self.storage_key).map_err(MlError::InvalidStorageKey)
    }
}

async fn load_embed_asset(
    pool: &PgPool,
    payload: &EmbedAssetPayload,
) -> Result<Option<EmbedAsset>, MlError> {
    let row = sqlx::query!(
        r#"
        SELECT
            o.storage_key,
            o.media_type,
            mp.embedding_dimension,
            mp.distance_metric,
            mp.manifest as "manifest: Json<ModelPackManifest>",
            ra.status
        FROM model_reindex_runs rr
        JOIN model_reindex_assets ra
          ON ra.reindex_run_id = rr.id
         AND ra.asset_id = $1
        JOIN model_packs mp
          ON mp.id = rr.model_pack_id
         AND mp.id = $2
        JOIN assets a
          ON a.id = ra.asset_id
        JOIN originals o
          ON o.id = a.original_id
        WHERE rr.id = $3
          AND a.trashed_at IS NULL
          AND mp.kind = 'semantic_image_text'
          AND mp.self_test_status = 'passed'
        "#,
        payload.asset_id,
        payload.model_pack_id,
        payload.reindex_run_id
    )
    .fetch_optional(pool)
    .await?;

    let Some(row) = row else {
        return Err(MlError::NotFound);
    };

    if row.status != "queued" {
        return Ok(None);
    }

    let manifest = row.manifest.0;
    validate_semantic_model_pack_row(&manifest, row.embedding_dimension, &row.distance_metric)?;

    Ok(Some(EmbedAsset {
        storage_key: row.storage_key,
        media_type: row.media_type,
        manifest,
        embedding_dimension: row.embedding_dimension,
    }))
}

async fn active_semantic_model_pack(pool: &PgPool) -> Result<ActiveSemanticModelPack, MlError> {
    let row = sqlx::query!(
        r#"
        SELECT id, embedding_dimension, distance_metric, manifest as "manifest: Json<ModelPackManifest>"
        FROM model_packs
        WHERE kind = 'semantic_image_text'
          AND status = 'active'
          AND self_test_status = 'passed'
        ORDER BY activated_at DESC NULLS LAST, updated_at DESC, id ASC
        LIMIT 1
        "#,
    )
    .fetch_optional(pool)
    .await?;

    let Some(row) = row else {
        return Err(MlError::NotFound);
    };

    let manifest = row.manifest.0;
    validate_semantic_model_pack_row(&manifest, row.embedding_dimension, &row.distance_metric)?;

    Ok(ActiveSemanticModelPack {
        model_pack_id: row.id,
        manifest,
    })
}

async fn load_model_pack_manifest(
    pool: &PgPool,
    model_pack_id: Uuid,
) -> Result<ModelPackManifest, MlError> {
    sqlx::query_scalar!(
        r#"
        SELECT manifest as "manifest: Json<ModelPackManifest>"
        FROM model_packs
        WHERE id = $1
        "#,
        model_pack_id
    )
    .fetch_optional(pool)
    .await?
    .map(|manifest| manifest.0)
    .ok_or(MlError::NotFound)
}

fn validate_semantic_model_pack_row(
    manifest: &ModelPackManifest,
    embedding_dimension: i32,
    distance_metric: &str,
) -> Result<(), MlError> {
    let validated = validate_model_pack_manifest(manifest)?;

    if validated.kind != ModelPackKind::SemanticImageText {
        return Err(ModelPackError::InvalidManifest("kind").into());
    }

    if manifest.embedding_dimension != embedding_dimension {
        return Err(ModelPackError::InvalidManifest("embedding_dimension").into());
    }

    if manifest.distance_metric != distance_metric {
        return Err(ModelPackError::InvalidManifest("distance_metric").into());
    }

    Ok(())
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

    if trimmed.is_empty()
        || trimmed.len() > MAX_TEXT_QUERY_BYTES
        || trimmed.chars().count() > MAX_TEXT_QUERY_CHARS
    {
        Err(MlError::InvalidTextQuery)
    } else {
        Ok(trimmed)
    }
}

fn is_supported_still_image_media_type(media_type: &str) -> bool {
    // Keep this strict. GIF and WebP can be animated; enable them only after
    // the runtime explicitly rejects animated variants or defines first-frame
    // embedding semantics.
    matches!(media_type, "image/jpeg" | "image/png")
}

fn self_test_media_type(path: &str) -> Result<&'static str, MlError> {
    if path.ends_with(".jpg") || path.ends_with(".jpeg") {
        Ok("image/jpeg")
    } else if path.ends_with(".png") {
        Ok("image/png")
    } else {
        Err(ModelPackError::InvalidManifest("self_test.input_path").into())
    }
}

fn self_test_runtime_failure(error: &MlError) -> bool {
    matches!(
        error,
        MlError::RuntimeUnavailable
            | MlError::ImageTooLarge
            | MlError::Model(ModelPackError::InvalidEmbedding(_))
    )
}

async fn record_self_test_failure(
    pool: &PgPool,
    model_pack_id: Uuid,
    message: &str,
) -> Result<crate::models::InstalledModelPack, MlError> {
    let message = truncate_chars(message, MAX_REINDEX_ERROR_MESSAGE_CHARS);
    crate::models::record_model_pack_self_test(pool, model_pack_id, false, Some(&message))
        .await
        .map_err(MlError::Model)
}

/// Returns the stable SHA-256 digest of little-endian f32 task output values.
pub fn sha256_f32_values(values: &[f32]) -> String {
    let mut hasher = Sha256::new();
    for value in values {
        hasher.update(value.to_le_bytes());
    }
    format!("{:x}", hasher.finalize())
}

fn is_terminal_reindex_failure(error: &MlError) -> bool {
    match error {
        MlError::NotFound
        | MlError::UnsupportedMediaType
        | MlError::ImageTooLarge
        | MlError::InvalidStorageKey(_) => true,

        MlError::Model(
            ModelPackError::InvalidManifest(_)
            | ModelPackError::NotFound
            | ModelPackError::InvalidEmbedding(_)
            | ModelPackError::SelfTestRequired
            | ModelPackError::InvalidFilePath
            | ModelPackError::FileVerificationFailed
            | ModelPackError::StorageKey(_),
        ) => true,

        MlError::Model(ModelPackError::Storage(StorageError::ObjectTooLarge)) => true,

        MlError::SemanticIndex(
            SemanticIndexError::InvalidModelPack
            | SemanticIndexError::DimensionMismatch
            | SemanticIndexError::InvalidLimit
            | SemanticIndexError::AssetUnavailable,
        ) => true,

        MlError::UnsupportedJobKind
        | MlError::InvalidJobPayload
        | MlError::InvalidTextQuery
        | MlError::RuntimeUnavailable
        | MlError::Storage(_)
        | MlError::Model(_)
        | MlError::SemanticIndex(SemanticIndexError::Database(_))
        | MlError::Database(_) => false,
    }
}

fn reindex_error_message(error: &MlError) -> String {
    truncate_chars(&error.to_string(), MAX_REINDEX_ERROR_MESSAGE_CHARS)
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}
