//! Model-pack metadata and activation invariants.
//!
//! This module tracks model supply-chain state only. Runtime-specific inference
//! code belongs in the ML worker and must consume validated task-level model
//! packs instead of leaking ONNX/session handles into backend feature code.

use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::{PgPool, types::Json};
use uuid::Uuid;

use crate::jobs::{self, JobKind, JobSpec};
use crate::storage::{ObjectStorage, StorageKey, StorageKeyError};

/// Supported model task kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelPackKind {
    /// Shared image/text embedding space for semantic search.
    SemanticImageText,
    /// Face detection/alignment/identity embedding pipeline.
    FaceIdentity,
}

impl ModelPackKind {
    /// Stable database representation.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SemanticImageText => "semantic_image_text",
            Self::FaceIdentity => "face_identity",
        }
    }
}

/// Supported model runtimes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelRuntime {
    /// ONNX Runtime through the ML worker.
    Onnx,
}

impl ModelRuntime {
    /// Stable database representation.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Onnx => "onnx",
        }
    }
}

/// Vector distance metric emitted by the model pack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DistanceMetric {
    /// Cosine distance.
    Cosine,
    /// Dot-product similarity.
    Dot,
    /// Euclidean distance.
    L2,
}

impl DistanceMetric {
    /// Stable database representation.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cosine => "cosine",
            Self::Dot => "dot",
            Self::L2 => "l2",
        }
    }
}

/// Model-pack manifest accepted by Mirror.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelPackManifest {
    /// Task kind, for example `semantic_image_text`.
    pub kind: String,
    /// Runtime, initially `onnx`.
    pub runtime: String,
    /// Stable model family/key.
    pub model_key: String,
    /// Pinned upstream/model-pack revision.
    pub model_revision: String,
    /// License identifier or short license policy label.
    pub license: String,
    /// Embedding vector length.
    pub embedding_dimension: i32,
    /// Vector distance metric.
    pub distance_metric: String,
    /// Files included in the model pack.
    pub files: Vec<ModelPackFileManifest>,
    /// Golden self-tests that must pass before activation.
    pub self_tests: Vec<ModelPackSelfTestManifest>,
}

/// One model-pack file selected by path and checksum.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelPackFileManifest {
    /// Relative path inside the model pack.
    pub path: String,
    /// Hex SHA-256 digest.
    pub sha256: String,
    /// Expected file size in bytes.
    pub size_bytes: i64,
}

/// One golden self-test declared by a model pack.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelPackSelfTestManifest {
    /// Stable self-test name.
    pub name: String,
    /// Relative input fixture path inside the model pack.
    pub input_path: String,
    /// Hex SHA-256 digest of the expected task output fixture.
    pub expected_output_sha256: String,
}

/// Installed model-pack row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledModelPack {
    /// Model-pack row ID.
    pub model_pack_id: Uuid,
    /// Current install/activation status.
    pub status: String,
    /// Current self-test status.
    pub self_test_status: String,
}

/// Reindex run created for a model pack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelReindexRun {
    /// Reindex run ID.
    pub reindex_run_id: Uuid,
    /// Model pack being indexed.
    pub model_pack_id: Uuid,
    /// Current run status.
    pub status: String,
    /// Number of active assets selected when the run was created.
    pub total_assets: i32,
    /// Number of embedding jobs enqueued for this run.
    pub queued_assets: i32,
    /// Number of assets that completed successfully.
    pub processed_assets: i32,
    /// Number of assets that failed terminally.
    pub failed_assets: i32,
}

/// Installed model-pack file copy result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledModelPackFile {
    /// Manifest-relative source path.
    pub path: String,
    /// Durable storage key.
    pub storage_key: StorageKey,
    /// Verified file size.
    pub size_bytes: i64,
}

/// Embedding output accepted from an ML runtime.
#[derive(Debug, Clone, PartialEq)]
pub struct ValidatedEmbedding {
    /// Dense embedding vector.
    values: Vec<f32>,
}

impl ValidatedEmbedding {
    /// Returns the vector values.
    #[must_use]
    pub fn values(&self) -> &[f32] {
        &self.values
    }
}

/// Model-pack operation failure.
#[derive(Debug)]
pub enum ModelPackError {
    /// Manifest violates Mirror model-pack invariants.
    InvalidManifest(&'static str),
    /// Model pack does not exist.
    NotFound,
    /// Runtime output violates model-pack embedding invariants.
    InvalidEmbedding(&'static str),
    /// Self-test must pass before activation.
    SelfTestRequired,
    /// Local model-pack file path is unsafe.
    InvalidFilePath,
    /// Local model-pack file failed size or checksum verification.
    FileVerificationFailed,
    /// Local model-pack file I/O failed.
    Io(std::io::Error),
    /// Storage operation failed.
    Storage(crate::storage::StorageError),
    /// Generated storage key was invalid.
    StorageKey(StorageKeyError),
    /// Job queue operation failed.
    Job(jobs::JobError),
    /// Database failed.
    Database(sqlx::Error),
}

impl std::fmt::Display for ModelPackError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::InvalidManifest(_) => "invalid model-pack manifest",
            Self::NotFound => "model pack not found",
            Self::InvalidEmbedding(_) => "invalid model embedding output",
            Self::SelfTestRequired => "model pack self-test has not passed",
            Self::InvalidFilePath => "model-pack file path is invalid",
            Self::FileVerificationFailed => "model-pack file verification failed",
            Self::Io(_) => "model-pack file io error",
            Self::Storage(_) => "model-pack storage error",
            Self::StorageKey(_) => "model-pack storage key error",
            Self::Job(_) => "model-pack job queue error",
            Self::Database(_) => "model-pack database error",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for ModelPackError {}

impl From<sqlx::Error> for ModelPackError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

impl From<jobs::JobError> for ModelPackError {
    fn from(error: jobs::JobError) -> Self {
        Self::Job(error)
    }
}

impl From<std::io::Error> for ModelPackError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<crate::storage::StorageError> for ModelPackError {
    fn from(error: crate::storage::StorageError) -> Self {
        Self::Storage(error)
    }
}

impl From<StorageKeyError> for ModelPackError {
    fn from(error: StorageKeyError) -> Self {
        Self::StorageKey(error)
    }
}

/// Validates and records a model pack.
pub async fn install_model_pack(
    pool: &PgPool,
    manifest: ModelPackManifest,
) -> Result<InstalledModelPack, ModelPackError> {
    let validated = validate_model_pack_manifest(&manifest)?;
    let manifest_json =
        serde_json::to_value(&manifest).map_err(|_| ModelPackError::InvalidManifest("json"))?;
    let id = Uuid::now_v7();
    let mut tx = pool.begin().await?;

    let row = sqlx::query_as::<_, (Uuid, String, String)>(
        r#"
        INSERT INTO model_packs (
            id,
            kind,
            runtime,
            model_key,
            model_revision,
            license,
            embedding_dimension,
            distance_metric,
            manifest
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
        RETURNING id, status, self_test_status
        "#,
    )
    .bind(id)
    .bind(validated.kind.as_str())
    .bind(validated.runtime.as_str())
    .bind(&manifest.model_key)
    .bind(&manifest.model_revision)
    .bind(&manifest.license)
    .bind(manifest.embedding_dimension)
    .bind(validated.distance_metric.as_str())
    .bind(Json(manifest_json))
    .fetch_one(&mut *tx)
    .await?;

    for file in manifest.files {
        sqlx::query(
            r#"
            INSERT INTO model_pack_files (model_pack_id, path, sha256, size_bytes)
            VALUES ($1, $2, $3, $4)
            "#,
        )
        .bind(id)
        .bind(file.path)
        .bind(file.sha256)
        .bind(file.size_bytes)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    Ok(InstalledModelPack {
        model_pack_id: row.0,
        status: row.1,
        self_test_status: row.2,
    })
}

/// Verifies and copies all files for an installed model pack into durable
/// object storage.
///
/// The source directory is operator-controlled local input. Manifest paths are
/// still revalidated before joining so path traversal cannot escape it.
pub async fn install_model_pack_files(
    storage: &ObjectStorage,
    source_dir: &Path,
    model_pack_id: Uuid,
    manifest: &ModelPackManifest,
) -> Result<Vec<InstalledModelPackFile>, ModelPackError> {
    validate_model_pack_manifest(manifest)?;
    let mut installed = Vec::with_capacity(manifest.files.len());
    for file in &manifest.files {
        let source_path = model_pack_source_path(source_dir, &file.path)?;
        let bytes = tokio::fs::read(&source_path).await?;
        verify_model_pack_file(&bytes, file)?;
        let storage_key = StorageKey::model_pack_file(model_pack_id, &file.path)?;
        storage.write(&storage_key, bytes).await?;
        installed.push(InstalledModelPackFile {
            path: file.path.clone(),
            storage_key,
            size_bytes: file.size_bytes,
        });
    }
    Ok(installed)
}

/// Validates an embedding emitted by a task-level ML runtime.
pub fn validate_embedding_output(
    manifest: &ModelPackManifest,
    values: Vec<f32>,
) -> Result<ValidatedEmbedding, ModelPackError> {
    let validated = validate_model_pack_manifest(manifest)?;
    let expected = usize::try_from(manifest.embedding_dimension)
        .map_err(|_| ModelPackError::InvalidEmbedding("dimension"))?;
    if values.len() != expected {
        return Err(ModelPackError::InvalidEmbedding("dimension"));
    }
    if values.iter().any(|value| !value.is_finite()) {
        return Err(ModelPackError::InvalidEmbedding("finite"));
    }
    if validated.distance_metric == DistanceMetric::Cosine {
        let norm_squared = values.iter().map(|value| value * value).sum::<f32>();
        if norm_squared <= f32::EPSILON {
            return Err(ModelPackError::InvalidEmbedding("zero_norm"));
        }
    }

    Ok(ValidatedEmbedding { values })
}

/// Records self-test status for an installed model pack.
pub async fn record_model_pack_self_test(
    pool: &PgPool,
    model_pack_id: Uuid,
    passed: bool,
    error_message: Option<&str>,
) -> Result<InstalledModelPack, ModelPackError> {
    let row = sqlx::query_as::<_, (Uuid, String, String)>(
        r#"
        UPDATE model_packs
        SET
            self_test_status = CASE WHEN $2 THEN 'passed' ELSE 'failed' END,
            self_test_error = CASE WHEN $2 THEN NULL ELSE $3 END,
            updated_at = now()
        WHERE id = $1
        RETURNING id, status, self_test_status
        "#,
    )
    .bind(model_pack_id)
    .bind(passed)
    .bind(error_message)
    .fetch_optional(pool)
    .await?;

    let Some(row) = row else {
        return Err(ModelPackError::NotFound);
    };
    Ok(InstalledModelPack {
        model_pack_id: row.0,
        status: row.1,
        self_test_status: row.2,
    })
}

/// Activates one self-tested model pack and deactivates the previous pack for
/// the same task kind.
pub async fn activate_model_pack(
    pool: &PgPool,
    model_pack_id: Uuid,
) -> Result<InstalledModelPack, ModelPackError> {
    let mut tx = pool.begin().await?;
    let row = sqlx::query_as::<_, (String, String)>(
        r#"
        SELECT kind, self_test_status
        FROM model_packs
        WHERE id = $1
        FOR UPDATE
        "#,
    )
    .bind(model_pack_id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some((kind, self_test_status)) = row else {
        return Err(ModelPackError::NotFound);
    };
    if self_test_status != "passed" {
        return Err(ModelPackError::SelfTestRequired);
    }

    sqlx::query(
        r#"
        UPDATE model_packs
        SET status = 'installed', activated_at = NULL, updated_at = now()
        WHERE kind = $1 AND status = 'active'
        "#,
    )
    .bind(&kind)
    .execute(&mut *tx)
    .await?;

    let row = sqlx::query_as::<_, (Uuid, String, String)>(
        r#"
        UPDATE model_packs
        SET status = 'active', activated_at = now(), updated_at = now()
        WHERE id = $1
        RETURNING id, status, self_test_status
        "#,
    )
    .bind(model_pack_id)
    .fetch_one(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(InstalledModelPack {
        model_pack_id: row.0,
        status: row.1,
        self_test_status: row.2,
    })
}

/// Creates a model reindex run and enqueues one embedding job per active asset.
pub async fn start_model_reindex(
    pool: &PgPool,
    model_pack_id: Uuid,
) -> Result<ModelReindexRun, ModelPackError> {
    let mut tx = pool.begin().await?;
    let row = sqlx::query_as::<_, (String, String)>(
        r#"
        SELECT kind, self_test_status
        FROM model_packs
        WHERE id = $1
        FOR UPDATE
        "#,
    )
    .bind(model_pack_id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some((kind, self_test_status)) = row else {
        return Err(ModelPackError::NotFound);
    };
    if self_test_status != "passed" {
        return Err(ModelPackError::SelfTestRequired);
    }

    let asset_ids: Vec<Uuid> = sqlx::query_scalar(
        r#"
        SELECT id
        FROM assets
        WHERE trashed_at IS NULL
        ORDER BY created_at ASC, id ASC
        "#,
    )
    .fetch_all(&mut *tx)
    .await?;
    let total_assets =
        i32::try_from(asset_ids.len()).map_err(|_| ModelPackError::InvalidManifest("assets"))?;
    let status = if asset_ids.is_empty() {
        "succeeded"
    } else {
        "queued"
    };
    let run_id = Uuid::now_v7();

    let row = sqlx::query_as::<_, (Uuid, Uuid, String, i32, i32, i32, i32)>(
        r#"
        INSERT INTO model_reindex_runs (
            id,
            model_pack_id,
            kind,
            status,
            total_assets,
            queued_assets,
            completed_at
        )
        VALUES ($1, $2, $3, $4, $5, $5, CASE WHEN $4 = 'succeeded' THEN now() ELSE NULL END)
        RETURNING id, model_pack_id, status, total_assets, queued_assets, processed_assets, failed_assets
        "#,
    )
    .bind(run_id)
    .bind(model_pack_id)
    .bind(&kind)
    .bind(status)
    .bind(total_assets)
    .fetch_one(&mut *tx)
    .await?;

    for asset_id in asset_ids {
        sqlx::query(
            r#"
            INSERT INTO model_reindex_assets (reindex_run_id, asset_id)
            VALUES ($1, $2)
            "#,
        )
        .bind(run_id)
        .bind(asset_id)
        .execute(&mut *tx)
        .await?;
        jobs::enqueue_in_tx(
            &mut tx,
            JobSpec::immediate(
                JobKind::EmbedAsset,
                json!({
                    "asset_id": asset_id,
                    "model_pack_id": model_pack_id,
                    "reindex_run_id": run_id,
                }),
                format!("model-reindex:{run_id}:{asset_id}"),
            ),
        )
        .await?;
    }

    tx.commit().await?;
    Ok(ModelReindexRun {
        reindex_run_id: row.0,
        model_pack_id: row.1,
        status: row.2,
        total_assets: row.3,
        queued_assets: row.4,
        processed_assets: row.5,
        failed_assets: row.6,
    })
}

/// Records one terminal asset result for a reindex run.
///
/// Retries should call this only after a final success or dead-letter. The
/// per-asset row makes repeated terminal reports idempotent.
pub async fn record_reindex_asset_result(
    pool: &PgPool,
    reindex_run_id: Uuid,
    asset_id: Uuid,
    succeeded: bool,
    error_message: Option<&str>,
) -> Result<ModelReindexRun, ModelPackError> {
    let mut tx = pool.begin().await?;
    let changed = sqlx::query_scalar::<_, bool>(
        r#"
        UPDATE model_reindex_assets
        SET
            status = CASE WHEN $3 THEN 'done' ELSE 'failed' END,
            error_message = CASE WHEN $3 THEN NULL ELSE $4 END,
            updated_at = now()
        WHERE reindex_run_id = $1
          AND asset_id = $2
          AND status = 'queued'
        RETURNING true
        "#,
    )
    .bind(reindex_run_id)
    .bind(asset_id)
    .bind(succeeded)
    .bind(error_message)
    .fetch_optional(&mut *tx)
    .await?
    .unwrap_or(false);

    if changed {
        sqlx::query(
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
                completed_at = CASE
                    WHEN processed_assets + failed_assets + 1 >= total_assets
                    THEN now()
                    ELSE completed_at
                END,
                updated_at = now()
            WHERE id = $1
            "#,
        )
        .bind(reindex_run_id)
        .bind(succeeded)
        .execute(&mut *tx)
        .await?;
    }

    let row = sqlx::query_as::<_, (Uuid, Uuid, String, i32, i32, i32, i32)>(
        r#"
        SELECT id, model_pack_id, status, total_assets, queued_assets, processed_assets, failed_assets
        FROM model_reindex_runs
        WHERE id = $1
        "#,
    )
    .bind(reindex_run_id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(row) = row else {
        return Err(ModelPackError::NotFound);
    };
    tx.commit().await?;
    Ok(ModelReindexRun {
        reindex_run_id: row.0,
        model_pack_id: row.1,
        status: row.2,
        total_assets: row.3,
        queued_assets: row.4,
        processed_assets: row.5,
        failed_assets: row.6,
    })
}

/// Validated manifest enum values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ValidatedModelPackManifest {
    /// Validated task kind.
    pub kind: ModelPackKind,
    /// Validated runtime.
    pub runtime: ModelRuntime,
    /// Validated distance metric.
    pub distance_metric: DistanceMetric,
}

/// Validates a model-pack manifest without touching storage or Postgres.
pub fn validate_model_pack_manifest(
    manifest: &ModelPackManifest,
) -> Result<ValidatedModelPackManifest, ModelPackError> {
    let kind = parse_kind(&manifest.kind)?;
    let runtime = parse_runtime(&manifest.runtime)?;
    let distance_metric = parse_distance_metric(&manifest.distance_metric)?;
    require_text(&manifest.model_key, 120, "model_key")?;
    require_text(&manifest.model_revision, 200, "model_revision")?;
    require_text(&manifest.license, 200, "license")?;
    if !(1..=32_768).contains(&manifest.embedding_dimension) {
        return Err(ModelPackError::InvalidManifest("embedding_dimension"));
    }
    if manifest.files.is_empty() {
        return Err(ModelPackError::InvalidManifest("files"));
    }
    if manifest.self_tests.is_empty() {
        return Err(ModelPackError::InvalidManifest("self_tests"));
    }

    let mut paths = HashSet::new();
    for file in &manifest.files {
        validate_pack_path(&file.path, "file.path")?;
        validate_sha256(&file.sha256, "file.sha256")?;
        if file.size_bytes <= 0 {
            return Err(ModelPackError::InvalidManifest("file.size_bytes"));
        }
        if !paths.insert(file.path.as_str()) {
            return Err(ModelPackError::InvalidManifest("file.path.duplicate"));
        }
    }

    for self_test in &manifest.self_tests {
        require_text(&self_test.name, 120, "self_test.name")?;
        validate_pack_path(&self_test.input_path, "self_test.input_path")?;
        validate_sha256(
            &self_test.expected_output_sha256,
            "self_test.expected_output_sha256",
        )?;
    }

    Ok(ValidatedModelPackManifest {
        kind,
        runtime,
        distance_metric,
    })
}

fn parse_kind(value: &str) -> Result<ModelPackKind, ModelPackError> {
    match value {
        "semantic_image_text" => Ok(ModelPackKind::SemanticImageText),
        "face_identity" => Ok(ModelPackKind::FaceIdentity),
        _ => Err(ModelPackError::InvalidManifest("kind")),
    }
}

fn parse_runtime(value: &str) -> Result<ModelRuntime, ModelPackError> {
    match value {
        "onnx" => Ok(ModelRuntime::Onnx),
        _ => Err(ModelPackError::InvalidManifest("runtime")),
    }
}

fn parse_distance_metric(value: &str) -> Result<DistanceMetric, ModelPackError> {
    match value {
        "cosine" => Ok(DistanceMetric::Cosine),
        "dot" => Ok(DistanceMetric::Dot),
        "l2" => Ok(DistanceMetric::L2),
        _ => Err(ModelPackError::InvalidManifest("distance_metric")),
    }
}

fn require_text(value: &str, max_len: usize, field: &'static str) -> Result<(), ModelPackError> {
    if value.trim().is_empty() || value.len() > max_len {
        Err(ModelPackError::InvalidManifest(field))
    } else {
        Ok(())
    }
}

fn validate_pack_path(value: &str, field: &'static str) -> Result<(), ModelPackError> {
    if value.is_empty()
        || value.len() > 300
        || value.starts_with('/')
        || value.contains('\\')
        || value
            .split('/')
            .any(|segment| segment.is_empty() || matches!(segment, "." | ".."))
    {
        Err(ModelPackError::InvalidManifest(field))
    } else {
        Ok(())
    }
}

fn validate_sha256(value: &str, field: &'static str) -> Result<(), ModelPackError> {
    let valid = value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit());
    if valid {
        Ok(())
    } else {
        Err(ModelPackError::InvalidManifest(field))
    }
}

fn model_pack_source_path(
    source_dir: &Path,
    relative_path: &str,
) -> Result<PathBuf, ModelPackError> {
    validate_pack_path(relative_path, "file.path")?;
    let mut path = source_dir.to_path_buf();
    for segment in relative_path.split('/') {
        path.push(segment);
    }
    Ok(path)
}

fn verify_model_pack_file(
    bytes: &[u8],
    file: &ModelPackFileManifest,
) -> Result<(), ModelPackError> {
    if i64::try_from(bytes.len()).ok() != Some(file.size_bytes) {
        return Err(ModelPackError::FileVerificationFailed);
    }
    let digest = Sha256::digest(bytes);
    let actual = format!("{digest:x}");
    if actual.eq_ignore_ascii_case(&file.sha256) {
        Ok(())
    } else {
        Err(ModelPackError::FileVerificationFailed)
    }
}
