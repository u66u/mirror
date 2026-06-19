//! Model-pack metadata and activation invariants.
//!
//! This module tracks model supply-chain state only. Runtime-specific inference
//! code belongs in the ML worker and must consume validated task-level model
//! packs instead of leaking ONNX/session handles into backend feature code.

use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};
use thiserror::Error;

use schemars::{JsonSchema, schema_for};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::jobs::{self, JobKind, JobSpec};
use crate::paths::validate_relative_str;
use crate::storage::{ObjectStorage, StorageKey, StorageKeyError};

/// Manifest filename expected at the root of a local model-pack directory.
pub const MODEL_PACK_MANIFEST_FILENAME: &str = "manifest.json";

/// Built-in JSON-only model-pack presets.
pub const MODEL_PACK_PRESETS: &[&str] = &[
    "opencv_yunet_detection_2023mar",
    "opencv_sface_embedding_2021dec",
    "insightface_buffalo_l_scrfd_arcface",
];

/// Supported model task kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, sqlx::Type)]
#[sqlx(type_name = "text", rename_all = "snake_case")]
pub enum ModelPackKind {
    /// Shared image/text embedding space for semantic search.
    SemanticImageText,
    /// Combined face detection/alignment/identity embedding pipeline.
    FaceIdentity,
    /// Face detector model pack.
    FaceDetection,
    /// Face identity embedding model pack.
    FaceEmbedding,
}

impl ModelPackKind {
    /// Stable database representation.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SemanticImageText => "semantic_image_text",
            Self::FaceIdentity => "face_identity",
            Self::FaceDetection => "face_detection",
            Self::FaceEmbedding => "face_embedding",
        }
    }
}

/// Supported model runtimes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, sqlx::Type)]
#[sqlx(type_name = "text", rename_all = "snake_case")]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, sqlx::Type)]
#[sqlx(type_name = "text", rename_all = "snake_case")]
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

    /// Parses a database/manifest distance metric value.
    pub fn from_db_str(value: &str) -> Option<Self> {
        match value {
            "cosine" => Some(Self::Cosine),
            "dot" => Some(Self::Dot),
            "l2" => Some(Self::L2),
            _ => None,
        }
    }
}

/// Model-pack manifest accepted by Mirror.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
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
    /// Runtime-specific ONNX config.
    pub onnx: OnnxModelPackConfig,
    /// Image preprocessing contract for image inputs.
    pub image_preprocess: ImagePreprocessConfig,
    /// Face detection contract for face model packs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub face_detection: Option<FaceDetectionModelConfig>,
    /// Face embedding contract for face model packs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub face_embedding: Option<FaceEmbeddingModelConfig>,
    /// Files included in the model pack.
    pub files: Vec<ModelPackFileManifest>,
    /// Golden self-tests that must pass before activation.
    pub self_tests: Vec<ModelPackSelfTestManifest>,
}

/// ONNX session and tensor names required by the runtime.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct OnnxModelPackConfig {
    /// ONNX model used for image embeddings.
    pub image_model_path: String,
    /// ONNX model used for text embeddings.
    pub text_model_path: String,
    /// Tokenizer JSON used for text inputs.
    pub tokenizer_path: String,
    /// Image tensor input name.
    pub image_input_name: String,
    /// Image embedding output name.
    pub image_output_name: String,
    /// Token IDs tensor input name.
    pub text_input_ids_name: String,
    /// Attention mask tensor input name.
    pub text_attention_mask_name: String,
    /// Text embedding output name.
    pub text_output_name: String,
}

/// Image preprocessing contract required before ONNX image inference.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct ImagePreprocessConfig {
    /// Resize/crop target width.
    pub width: u32,
    /// Resize/crop target height.
    pub height: u32,
    /// Channel order expected by the model.
    pub color_order: String,
    /// Tensor layout expected by the model.
    pub tensor_layout: String,
    /// Per-channel input mean.
    pub mean: [f32; 3],
    /// Per-channel input standard deviation.
    pub std: [f32; 3],
}

/// ONNX face detector output contract.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct FaceDetectionModelConfig {
    /// Built-in detector adapter kind, for example `decoded_boxes_v1`,
    /// `scrfd`, or `yunet_opencv_compat`.
    #[serde(default = "default_face_detector_adapter")]
    pub adapter: String,
    /// ONNX model used for face detection.
    pub model_path: String,
    /// Image tensor input name.
    pub input_name: String,
    /// Face box tensor output name.
    pub boxes_output_name: String,
    /// Face score tensor output name.
    pub scores_output_name: String,
    /// Optional landmarks tensor output name.
    pub landmarks_output_name: Option<String>,
    /// Optional adapter-specific output names. SCRFD/YuNet adapters use this
    /// for multi-head outputs when model files do not use conventional names.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub output_names: Vec<String>,
    /// Box coordinate space: `normalized` or `pixel`.
    pub box_coordinate_space: String,
    /// Box format: `xywh` or `xyxy`.
    pub box_format: String,
    /// Minimum score accepted into the face pipeline.
    pub score_threshold: f32,
    /// Minimum normalized width/height accepted into the face pipeline.
    #[serde(default)]
    pub min_face_size_ratio: f32,
    /// IoU threshold used by NMS.
    pub nms_threshold: f32,
    /// Maximum faces stored per asset.
    pub max_faces: i32,
}

/// ONNX face embedding input/output contract.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct FaceEmbeddingModelConfig {
    /// Built-in embedding adapter kind, for example `raw_embedding_v1`,
    /// `arcface`, or `sface_opencv_compat`.
    #[serde(default = "default_face_embedding_adapter")]
    pub adapter: String,
    /// ONNX model used for face identity embeddings.
    pub model_path: String,
    /// Face crop tensor input name.
    pub input_name: String,
    /// Face embedding tensor output name.
    pub output_name: String,
    /// Face crop target width.
    pub width: u32,
    /// Face crop target height.
    pub height: u32,
    /// Channel order expected by the embedding model.
    pub color_order: String,
    /// Tensor layout expected by the embedding model.
    pub tensor_layout: String,
    /// Face crop preparation: `bbox_crop` or `five_point`.
    #[serde(default = "default_face_alignment")]
    pub alignment: String,
    /// Per-channel input mean.
    pub mean: [f32; 3],
    /// Per-channel input standard deviation.
    pub std: [f32; 3],
    /// Cosine similarity threshold for auto-assigning to an existing person.
    pub match_threshold: f32,
    /// Whether the runtime L2-normalizes output before storage/matching.
    #[serde(default)]
    pub l2_normalize_output: bool,
}

fn default_face_detector_adapter() -> String {
    "decoded_boxes_v1".to_owned()
}

fn default_face_embedding_adapter() -> String {
    "raw_embedding_v1".to_owned()
}

fn default_face_alignment() -> String {
    "five_point".to_owned()
}

/// One model-pack file selected by path and checksum.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct ModelPackFileManifest {
    /// Relative path inside the model pack.
    pub path: String,
    /// Hex SHA-256 digest.
    pub sha256: String,
    /// Expected file size in bytes.
    pub size_bytes: i64,
}

/// One golden self-test declared by a model pack.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct ModelPackSelfTestManifest {
    /// Stable self-test name.
    pub name: String,
    /// Relative input fixture path inside the model pack.
    pub input_path: String,
    /// Hex SHA-256 digest of the expected task output fixture.
    pub expected_output_sha256: String,
}

/// Installed model-pack row.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct InstalledModelPack {
    /// Model-pack row ID.
    pub model_pack_id: Uuid,
    /// Current install/activation status.
    pub status: String,
    /// Current self-test status.
    pub self_test_status: String,
}

/// Reindex run created for a model pack.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
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

/// Owner-visible model-pack catalog row.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ModelPackSummary {
    /// Model-pack row ID.
    pub model_pack_id: Uuid,
    /// Task kind.
    pub kind: String,
    /// Runtime key.
    pub runtime: String,
    /// Stable model family/key.
    pub model_key: String,
    /// Pinned model revision.
    pub model_revision: String,
    /// Current install/activation status.
    pub status: String,
    /// Current self-test status.
    pub self_test_status: String,
    /// Embedding vector length.
    pub embedding_dimension: i32,
    /// Distance metric.
    pub distance_metric: String,
    /// Last update time.
    pub updated_at: OffsetDateTime,
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

/// Result of validating a local model-pack directory before install.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ModelPackDirectoryReport {
    /// Path to the manifest file read from the directory.
    pub manifest_path: PathBuf,
    /// Manifest task kind.
    pub kind: String,
    /// Manifest runtime key.
    pub runtime: String,
    /// Stable model family/key.
    pub model_key: String,
    /// Pinned model revision.
    pub model_revision: String,
    /// Number of files verified from the manifest.
    pub file_count: usize,
    /// Total declared bytes for verified files.
    pub total_size_bytes: i64,
}

/// Embedding output accepted from an ML runtime.
#[derive(Debug, Clone, PartialEq)]
pub struct ValidatedEmbedding {
    /// Dense embedding vector.
    values: Vec<f32>,
}

type ModelPackSummaryRow = (
    Uuid,
    String,
    String,
    String,
    String,
    String,
    String,
    i32,
    String,
    OffsetDateTime,
);

impl From<ModelPackSummaryRow> for ModelPackSummary {
    fn from(row: ModelPackSummaryRow) -> Self {
        Self {
            model_pack_id: row.0,
            kind: row.1,
            runtime: row.2,
            model_key: row.3,
            model_revision: row.4,
            status: row.5,
            self_test_status: row.6,
            embedding_dimension: row.7,
            distance_metric: row.8,
            updated_at: row.9,
        }
    }
}

impl ValidatedEmbedding {
    /// Returns the vector values.
    #[must_use]
    pub fn values(&self) -> &[f32] {
        &self.values
    }
}

/// Model-pack operation failure.
#[derive(Debug, Error)]
pub enum ModelPackError {
    /// Manifest violates Mirror model-pack invariants.
    #[error("invalid model-pack manifest")]
    InvalidManifest(&'static str),
    /// Model pack does not exist.
    #[error("model pack not found")]
    NotFound,
    /// Runtime output violates model-pack embedding invariants.
    #[error("invalid model embedding output")]
    InvalidEmbedding(&'static str),
    /// Self-test must pass before activation.
    #[error("model pack self-test has not passed")]
    SelfTestRequired,
    /// Local model-pack file path is unsafe.
    #[error("model-pack file path is invalid")]
    InvalidFilePath,
    /// Local model-pack file failed size or checksum verification.
    #[error("model-pack file verification failed")]
    FileVerificationFailed,
    /// Local model-pack file I/O failed.
    #[error("model-pack file io error")]
    Io(#[from] std::io::Error),
    /// Model-pack JSON failed to parse or serialize.
    #[error("model-pack json error")]
    Json(#[from] serde_json::Error),
    /// Storage operation failed.
    #[error("model-pack storage error")]
    Storage(#[from] crate::storage::StorageError),
    /// Generated storage key was invalid.
    #[error("model-pack storage key error")]
    StorageKey(#[from] StorageKeyError),
    /// Job queue operation failed.
    #[error("model-pack job queue error")]
    Job(#[from] jobs::JobError),
    /// Database failed.
    #[error("model-pack database error")]
    Database(#[from] sqlx::Error),
}

/// Generates the JSON Schema for model-pack manifests from Rust DTO types.
pub fn model_pack_manifest_schema_json() -> Result<serde_json::Value, ModelPackError> {
    serde_json::to_value(schema_for!(ModelPackManifest)).map_err(ModelPackError::Json)
}

/// Returns one built-in model-pack preset manifest.
///
/// Presets are JSON authoring templates. Operators must replace file sizes and
/// SHA-256 values with the exact model/self-test files they install.
pub fn model_pack_preset_manifest(name: &str) -> Result<ModelPackManifest, ModelPackError> {
    match name {
        "opencv_yunet_detection_2023mar" => Ok(opencv_yunet_detection_preset()),
        "opencv_sface_embedding_2021dec" => Ok(opencv_sface_embedding_preset()),
        "insightface_buffalo_l_scrfd_arcface" => Ok(insightface_scrfd_arcface_preset()),
        _ => Err(ModelPackError::NotFound),
    }
}

/// Returns one built-in model-pack preset as pretty JSON.
pub fn model_pack_preset_manifest_json(name: &str) -> Result<String, ModelPackError> {
    let manifest = model_pack_preset_manifest(name)?;
    serde_json::to_string_pretty(&manifest).map_err(ModelPackError::Json)
}

/// Returns a built-in preset with `files[].sha256` and `files[].size_bytes`
/// filled from an existing local model-pack directory.
pub fn materialize_model_pack_preset_from_directory(
    name: &str,
    source_dir: &Path,
) -> Result<ModelPackManifest, ModelPackError> {
    let mut manifest = model_pack_preset_manifest(name)?;
    for file in &mut manifest.files {
        let source_path = model_pack_source_path(source_dir, &file.path)?;
        let bytes = std::fs::read(&source_path)?;
        file.size_bytes = i64::try_from(bytes.len())
            .map_err(|_| ModelPackError::InvalidManifest("file.size_bytes"))?;
        let digest = Sha256::digest(&bytes);
        file.sha256 = format!("{digest:x}");
    }
    validate_model_pack_manifest(&manifest)?;
    Ok(manifest)
}

/// Returns a materialized built-in preset as pretty JSON.
pub fn materialize_model_pack_preset_json(
    name: &str,
    source_dir: &Path,
) -> Result<String, ModelPackError> {
    let manifest = materialize_model_pack_preset_from_directory(name, source_dir)?;
    serde_json::to_string_pretty(&manifest).map_err(ModelPackError::Json)
}

fn opencv_yunet_detection_preset() -> ModelPackManifest {
    let model_path = "models/face_detection_yunet_2023mar.onnx".to_owned();
    ModelPackManifest {
        kind: "face_detection".to_owned(),
        runtime: "onnx".to_owned(),
        model_key: "opencv-yunet".to_owned(),
        model_revision: "2023mar".to_owned(),
        license: "Apache-2.0".to_owned(),
        embedding_dimension: 1,
        distance_metric: "cosine".to_owned(),
        onnx: preset_onnx_config(&model_path),
        image_preprocess: ImagePreprocessConfig {
            width: 320,
            height: 320,
            color_order: "rgb".to_owned(),
            tensor_layout: "nchw".to_owned(),
            mean: [0.0, 0.0, 0.0],
            std: [1.0 / 255.0, 1.0 / 255.0, 1.0 / 255.0],
        },
        face_detection: Some(FaceDetectionModelConfig {
            adapter: "yunet_opencv_compat".to_owned(),
            model_path: model_path.clone(),
            input_name: "input".to_owned(),
            boxes_output_name: "unused_boxes".to_owned(),
            scores_output_name: "unused_scores".to_owned(),
            landmarks_output_name: None,
            output_names: Vec::new(),
            box_coordinate_space: "pixel".to_owned(),
            box_format: "xywh".to_owned(),
            score_threshold: 0.3,
            min_face_size_ratio: 0.15,
            nms_threshold: 0.3,
            max_faces: 8,
        }),
        face_embedding: None,
        files: preset_files(&[&model_path, "self-tests/face.jpg"]),
        self_tests: preset_self_tests(),
    }
}

fn opencv_sface_embedding_preset() -> ModelPackManifest {
    let model_path = "models/face_recognition_sface_2021dec.onnx".to_owned();
    ModelPackManifest {
        kind: "face_embedding".to_owned(),
        runtime: "onnx".to_owned(),
        model_key: "opencv-sface".to_owned(),
        model_revision: "2021dec".to_owned(),
        license: "Apache-2.0".to_owned(),
        embedding_dimension: 128,
        distance_metric: "cosine".to_owned(),
        onnx: preset_onnx_config(&model_path),
        image_preprocess: ImagePreprocessConfig {
            width: 112,
            height: 112,
            color_order: "rgb".to_owned(),
            tensor_layout: "nchw".to_owned(),
            mean: [0.0, 0.0, 0.0],
            std: [1.0 / 255.0, 1.0 / 255.0, 1.0 / 255.0],
        },
        face_detection: None,
        face_embedding: Some(FaceEmbeddingModelConfig {
            adapter: "sface_opencv_compat".to_owned(),
            model_path: model_path.clone(),
            input_name: "data".to_owned(),
            output_name: "fc1".to_owned(),
            width: 112,
            height: 112,
            color_order: "rgb".to_owned(),
            tensor_layout: "nchw".to_owned(),
            alignment: "five_point".to_owned(),
            mean: [0.0, 0.0, 0.0],
            std: [1.0 / 255.0, 1.0 / 255.0, 1.0 / 255.0],
            match_threshold: 0.363,
            l2_normalize_output: true,
        }),
        files: preset_files(&[&model_path, "self-tests/aligned-face.jpg"]),
        self_tests: preset_self_tests(),
    }
}

fn insightface_scrfd_arcface_preset() -> ModelPackManifest {
    let detector_path = "models/det_10g.onnx".to_owned();
    let embedder_path = "models/w600k_r50.onnx".to_owned();
    ModelPackManifest {
        kind: "face_identity".to_owned(),
        runtime: "onnx".to_owned(),
        model_key: "insightface-buffalo-l".to_owned(),
        model_revision: "scrfd10g-w600k-r50".to_owned(),
        license: "model-license-required".to_owned(),
        embedding_dimension: 512,
        distance_metric: "cosine".to_owned(),
        onnx: preset_onnx_config(&detector_path),
        image_preprocess: ImagePreprocessConfig {
            width: 640,
            height: 640,
            color_order: "rgb".to_owned(),
            tensor_layout: "nchw".to_owned(),
            mean: [0.5, 0.5, 0.5],
            std: [128.0 / 255.0, 128.0 / 255.0, 128.0 / 255.0],
        },
        face_detection: Some(FaceDetectionModelConfig {
            adapter: "scrfd".to_owned(),
            model_path: detector_path.clone(),
            input_name: "input.1".to_owned(),
            boxes_output_name: "unused_boxes".to_owned(),
            scores_output_name: "unused_scores".to_owned(),
            landmarks_output_name: None,
            output_names: vec![
                "448".to_owned(),
                "471".to_owned(),
                "494".to_owned(),
                "451".to_owned(),
                "474".to_owned(),
                "497".to_owned(),
                "454".to_owned(),
                "477".to_owned(),
                "500".to_owned(),
            ],
            box_coordinate_space: "pixel".to_owned(),
            box_format: "xyxy".to_owned(),
            score_threshold: 0.5,
            min_face_size_ratio: 0.15,
            nms_threshold: 0.4,
            max_faces: 16,
        }),
        face_embedding: Some(FaceEmbeddingModelConfig {
            adapter: "arcface".to_owned(),
            model_path: embedder_path.clone(),
            input_name: "input.1".to_owned(),
            output_name: "683".to_owned(),
            width: 112,
            height: 112,
            color_order: "rgb".to_owned(),
            tensor_layout: "nchw".to_owned(),
            alignment: "five_point".to_owned(),
            mean: [0.5, 0.5, 0.5],
            std: [0.5, 0.5, 0.5],
            match_threshold: 0.55,
            l2_normalize_output: true,
        }),
        files: preset_files(&[&detector_path, &embedder_path, "self-tests/face.jpg"]),
        self_tests: preset_self_tests(),
    }
}

fn preset_onnx_config(model_path: &str) -> OnnxModelPackConfig {
    OnnxModelPackConfig {
        image_model_path: model_path.to_owned(),
        text_model_path: model_path.to_owned(),
        tokenizer_path: "self-tests/face.jpg".to_owned(),
        image_input_name: "unused_image".to_owned(),
        image_output_name: "unused_image_output".to_owned(),
        text_input_ids_name: "unused_input_ids".to_owned(),
        text_attention_mask_name: "unused_attention_mask".to_owned(),
        text_output_name: "unused_text_output".to_owned(),
    }
}

fn preset_files(paths: &[&str]) -> Vec<ModelPackFileManifest> {
    paths
        .iter()
        .enumerate()
        .map(|(index, path)| ModelPackFileManifest {
            path: (*path).to_owned(),
            sha256: format!("{index:064x}"),
            size_bytes: 1,
        })
        .collect()
}

fn preset_self_tests() -> Vec<ModelPackSelfTestManifest> {
    vec![ModelPackSelfTestManifest {
        name: "face_fixture".to_owned(),
        input_path: "self-tests/face.jpg".to_owned(),
        expected_output_sha256: "0".repeat(64),
    }]
}

/// Validates a local model-pack directory without touching storage or Postgres.
///
/// The directory must contain `manifest.json`. Every manifest file entry is
/// joined through the same path validator used by install, then checked for
/// exact byte length and SHA-256 digest.
pub fn validate_model_pack_directory(
    source_dir: &Path,
) -> Result<ModelPackDirectoryReport, ModelPackError> {
    let manifest_path = source_dir.join(MODEL_PACK_MANIFEST_FILENAME);
    let manifest_bytes = std::fs::read(&manifest_path)?;
    let manifest: ModelPackManifest = serde_json::from_slice(&manifest_bytes)?;
    validate_model_pack_manifest(&manifest)?;

    let mut total_size_bytes = 0_i64;
    for file in &manifest.files {
        let source_path = model_pack_source_path(source_dir, &file.path)?;
        let bytes = std::fs::read(&source_path)?;
        verify_model_pack_file(&bytes, file)?;
        total_size_bytes = total_size_bytes
            .checked_add(file.size_bytes)
            .ok_or(ModelPackError::InvalidManifest("files.size_bytes"))?;
    }

    Ok(ModelPackDirectoryReport {
        manifest_path,
        kind: manifest.kind,
        runtime: manifest.runtime,
        model_key: manifest.model_key,
        model_revision: manifest.model_revision,
        file_count: manifest.files.len(),
        total_size_bytes,
    })
}

/// Formats validation errors for operators running the local model-pack checker.
#[must_use]
pub fn model_pack_operator_error(error: &ModelPackError) -> String {
    match error {
        ModelPackError::InvalidManifest(field) => {
            format!("invalid model-pack manifest field: {field}")
        }
        ModelPackError::InvalidFilePath => "invalid model-pack file path".to_owned(),
        ModelPackError::FileVerificationFailed => {
            "model-pack file size or SHA-256 checksum mismatch".to_owned()
        }
        ModelPackError::Io(source) => {
            format!("model-pack file is missing or unreadable: {source}")
        }
        ModelPackError::Json(source) => format!("invalid model-pack JSON: {source}"),
        other => other.to_string(),
    }
}

/// Validates and records a model pack.
pub async fn install_model_pack(
    pool: &PgPool,
    manifest: ModelPackManifest,
) -> Result<InstalledModelPack, ModelPackError> {
    let validated = validate_model_pack_manifest(&manifest)?;
    let id = Uuid::now_v7();
    let mut tx = pool.begin().await?;

    let row = sqlx::query!(
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
        id,
        validated.kind as ModelPackKind,
        validated.runtime as ModelRuntime,
        manifest.model_key,
        manifest.model_revision,
        manifest.license,
        manifest.embedding_dimension,
        std::option::Option::<DistanceMetric>::from(validated.distance_metric) as _,
        sqlx::types::Json(&manifest) as _
    )
    .fetch_one(&mut *tx)
    .await?;

    for file in manifest.files {
        sqlx::query!(
            r#"
            INSERT INTO model_pack_files (model_pack_id, path, sha256, size_bytes)
            VALUES ($1, $2, $3, $4)
            "#,
            id,
            file.path,
            file.sha256,
            file.size_bytes
        )
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    Ok(InstalledModelPack {
        model_pack_id: row.id,
        status: row.status,
        self_test_status: row.self_test_status,
    })
}

/// Lists installed model packs for owner/admin surfaces.
pub async fn list_model_packs(pool: &PgPool) -> Result<Vec<ModelPackSummary>, ModelPackError> {
    sqlx::query_as::<_, ModelPackSummaryRow>(
        r#"
        SELECT
            id,
            kind,
            runtime,
            model_key,
            model_revision,
            status,
            self_test_status,
            embedding_dimension,
            distance_metric,
            updated_at
        FROM model_packs
        ORDER BY kind ASC, status = 'active' DESC, updated_at DESC, id ASC
        "#,
    )
    .fetch_all(pool)
    .await
    .map(|rows| rows.into_iter().map(ModelPackSummary::from).collect())
    .map_err(ModelPackError::Database)
}

/// Loads one installed model pack's task kind.
pub async fn model_pack_kind(
    pool: &PgPool,
    model_pack_id: Uuid,
) -> Result<ModelPackKind, ModelPackError> {
    let kind = sqlx::query_scalar!(
        r#"
        SELECT kind
        FROM model_packs
        WHERE id = $1
        "#,
        model_pack_id
    )
    .fetch_optional(pool)
    .await?
    .ok_or(ModelPackError::NotFound)?;
    parse_kind(&kind)
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
    let row = sqlx::query!(
        r#"
        UPDATE model_packs
        SET
            self_test_status = CASE WHEN $2 THEN 'passed' ELSE 'failed' END,
            self_test_error = CASE WHEN $2 THEN NULL ELSE $3 END,
            updated_at = now()
        WHERE id = $1
        RETURNING id, status, self_test_status
        "#,
        model_pack_id,
        passed,
        error_message
    )
    .fetch_optional(pool)
    .await?;

    let Some(row) = row else {
        return Err(ModelPackError::NotFound);
    };
    Ok(InstalledModelPack {
        model_pack_id: row.id,
        status: row.status,
        self_test_status: row.self_test_status,
    })
}

/// Activates one self-tested model pack and deactivates the previous pack for
/// the same task kind.
pub async fn activate_model_pack(
    pool: &PgPool,
    model_pack_id: Uuid,
) -> Result<InstalledModelPack, ModelPackError> {
    let mut tx = pool.begin().await?;
    let row = sqlx::query!(
        r#"
        SELECT kind, self_test_status
        FROM model_packs
        WHERE id = $1
        FOR UPDATE
        "#,
        model_pack_id
    )
    .fetch_optional(&mut *tx)
    .await?;
    let Some(row) = row else {
        return Err(ModelPackError::NotFound);
    };
    if row.self_test_status != "passed" {
        return Err(ModelPackError::SelfTestRequired);
    }
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext('mirror_model_pack_kind'), hashtext($1))")
        .bind(&row.kind)
        .execute(&mut *tx)
        .await?;
    sqlx::query!(
        r#"
        UPDATE model_packs
        SET status = 'installed', activated_at = NULL, updated_at = now()
        WHERE kind = $1 AND status = 'active'
        "#,
        row.kind
    )
    .execute(&mut *tx)
    .await?;

    let updated_row = sqlx::query!(
        r#"
        UPDATE model_packs
        SET status = 'active', activated_at = now(), updated_at = now()
        WHERE id = $1
        RETURNING id, status, self_test_status
        "#,
        model_pack_id
    )
    .fetch_one(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(InstalledModelPack {
        model_pack_id: updated_row.id,
        status: updated_row.status,
        self_test_status: updated_row.self_test_status,
    })
}

/// Creates a model reindex run and enqueues one embedding job per active asset.
pub async fn start_model_reindex(
    pool: &PgPool,
    model_pack_id: Uuid,
) -> Result<ModelReindexRun, ModelPackError> {
    let mut tx = pool.begin().await?;
    let row = sqlx::query!(
        r#"
        SELECT kind, self_test_status
        FROM model_packs
        WHERE id = $1
        FOR UPDATE
        "#,
        model_pack_id
    )
    .fetch_optional(&mut *tx)
    .await?;
    let Some(row) = row else {
        return Err(ModelPackError::NotFound);
    };
    if row.self_test_status != "passed" {
        return Err(ModelPackError::SelfTestRequired);
    }
    let model_kind = row.kind;

    let asset_ids: Vec<Uuid> = sqlx::query_scalar!(
        r#"
        SELECT id
        FROM assets
        WHERE trashed_at IS NULL
        ORDER BY created_at ASC, id ASC
        "#
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

    let row = sqlx::query!(
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
        run_id,
        model_pack_id,
        model_kind,
        status,
        total_assets
    )
    .fetch_one(&mut *tx)
    .await?;

    let face_pair = match model_kind.as_str() {
        "semantic_image_text" => None,
        "face_identity" => Some((model_pack_id, model_pack_id)),
        "face_detection" => Some((
            model_pack_id,
            active_face_counterpart(&mut tx, &["face_embedding", "face_identity"]).await?,
        )),
        "face_embedding" => Some((
            active_face_counterpart(&mut tx, &["face_detection", "face_identity"]).await?,
            model_pack_id,
        )),
        _ => return Err(ModelPackError::InvalidManifest("kind")),
    };

    for asset_id in asset_ids {
        sqlx::query!(
            r#"
            INSERT INTO model_reindex_assets (reindex_run_id, asset_id)
            VALUES ($1, $2)
            "#,
            run_id,
            asset_id
        )
        .execute(&mut *tx)
        .await?;
        let (kind, payload) =
            if let Some((detection_model_pack_id, embedding_model_pack_id)) = face_pair {
                (
                    JobKind::IndexFaces,
                    json!({
                        "asset_id": asset_id,
                        "detection_model_pack_id": detection_model_pack_id,
                        "embedding_model_pack_id": embedding_model_pack_id,
                        "reindex_run_id": run_id,
                    }),
                )
            } else {
                (
                    JobKind::EmbedAsset,
                    json!({
                        "asset_id": asset_id,
                        "model_pack_id": model_pack_id,
                        "reindex_run_id": run_id,
                    }),
                )
            };
        jobs::enqueue_in_tx(
            &mut tx,
            JobSpec::immediate(kind, payload, format!("model-reindex:{run_id}:{asset_id}")),
        )
        .await?;
    }

    tx.commit().await?;
    Ok(ModelReindexRun {
        reindex_run_id: row.id,
        model_pack_id: row.model_pack_id,
        status: row.status,
        total_assets: row.total_assets,
        queued_assets: row.queued_assets,
        processed_assets: row.processed_assets,
        failed_assets: row.failed_assets,
    })
}

async fn active_face_counterpart(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    kinds: &[&str],
) -> Result<Uuid, ModelPackError> {
    let row = sqlx::query(
        r#"
        SELECT id
        FROM model_packs
        WHERE kind = ANY($1)
          AND status = 'active'
          AND self_test_status = 'passed'
        ORDER BY activated_at DESC NULLS LAST, updated_at DESC, id ASC
        LIMIT 1
        "#,
    )
    .bind(kinds)
    .fetch_optional(&mut **tx)
    .await?;
    row.map(|row| row.get("id")).ok_or(ModelPackError::NotFound)
}

/// Lists recent reindex runs for one model pack.
pub async fn list_model_reindex_runs(
    pool: &PgPool,
    model_pack_id: Uuid,
) -> Result<Vec<ModelReindexRun>, ModelPackError> {
    let exists = sqlx::query_scalar!(
        r#"SELECT true as "b!" FROM model_packs WHERE id = $1"#,
        model_pack_id
    )
    .fetch_optional(pool)
    .await?
    .unwrap_or(false);
    if !exists {
        return Err(ModelPackError::NotFound);
    }

    sqlx::query_as!(
        ModelReindexRun,
        r#"
        SELECT
            id as reindex_run_id,
            model_pack_id,
            status,
            total_assets,
            queued_assets,
            processed_assets,
            failed_assets
        FROM model_reindex_runs
        WHERE model_pack_id = $1
        ORDER BY created_at DESC, id DESC
        LIMIT 20
        "#,
        model_pack_id
    )
    .fetch_all(pool)
    .await
    .map_err(ModelPackError::Database)
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
        RETURNING true as "b!"
        "#,
        reindex_run_id,
        asset_id,
        succeeded,
        error_message
    )
    .fetch_optional(&mut *tx)
    .await?
    .unwrap_or(false);

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
                completed_at = CASE
                    WHEN processed_assets + failed_assets + 1 >= total_assets
                    THEN now()
                    ELSE completed_at
                END,
                updated_at = now()
            WHERE id = $1
            "#,
            reindex_run_id,
            succeeded
        )
        .execute(&mut *tx)
        .await?;
    }

    let row = sqlx::query!(
        r#"
        SELECT id, model_pack_id, status, total_assets, queued_assets, processed_assets, failed_assets
        FROM model_reindex_runs
        WHERE id = $1
        "#,
        reindex_run_id
    )
    .fetch_optional(&mut *tx)
    .await?;
    let Some(row) = row else {
        return Err(ModelPackError::NotFound);
    };
    tx.commit().await?;
    Ok(ModelReindexRun {
        reindex_run_id: row.id,
        model_pack_id: row.model_pack_id,
        status: row.status,
        total_assets: row.total_assets,
        queued_assets: row.queued_assets,
        processed_assets: row.processed_assets,
        failed_assets: row.failed_assets,
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
    validate_onnx_config(&manifest.onnx)?;
    validate_image_preprocess(&manifest.image_preprocess)?;
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

    match kind {
        ModelPackKind::SemanticImageText => {
            require_manifest_file(
                &paths,
                &manifest.onnx.image_model_path,
                "onnx.image_model_path",
            )?;
            require_manifest_file(
                &paths,
                &manifest.onnx.text_model_path,
                "onnx.text_model_path",
            )?;
            require_manifest_file(&paths, &manifest.onnx.tokenizer_path, "onnx.tokenizer_path")?;
        }
        ModelPackKind::FaceDetection => {
            let config = manifest
                .face_detection
                .as_ref()
                .ok_or(ModelPackError::InvalidManifest("face_detection"))?;
            validate_face_detection_config(config)?;
            require_manifest_file(&paths, &config.model_path, "face_detection.model_path")?;
        }
        ModelPackKind::FaceEmbedding => {
            let config = manifest
                .face_embedding
                .as_ref()
                .ok_or(ModelPackError::InvalidManifest("face_embedding"))?;
            validate_face_embedding_config(config)?;
            require_manifest_file(&paths, &config.model_path, "face_embedding.model_path")?;
        }
        ModelPackKind::FaceIdentity => {
            let detection = manifest
                .face_detection
                .as_ref()
                .ok_or(ModelPackError::InvalidManifest("face_detection"))?;
            let embedding = manifest
                .face_embedding
                .as_ref()
                .ok_or(ModelPackError::InvalidManifest("face_embedding"))?;
            validate_face_detection_config(detection)?;
            validate_face_embedding_config(embedding)?;
            require_manifest_file(&paths, &detection.model_path, "face_detection.model_path")?;
            require_manifest_file(&paths, &embedding.model_path, "face_embedding.model_path")?;
        }
    }

    Ok(ValidatedModelPackManifest {
        kind,
        runtime,
        distance_metric,
    })
}

fn validate_onnx_config(config: &OnnxModelPackConfig) -> Result<(), ModelPackError> {
    validate_pack_path(&config.image_model_path, "onnx.image_model_path")?;
    validate_pack_path(&config.text_model_path, "onnx.text_model_path")?;
    validate_pack_path(&config.tokenizer_path, "onnx.tokenizer_path")?;
    require_text(&config.image_input_name, 120, "onnx.image_input_name")?;
    require_text(&config.image_output_name, 120, "onnx.image_output_name")?;
    require_text(&config.text_input_ids_name, 120, "onnx.text_input_ids_name")?;
    require_text(
        &config.text_attention_mask_name,
        120,
        "onnx.text_attention_mask_name",
    )?;
    require_text(&config.text_output_name, 120, "onnx.text_output_name")?;
    Ok(())
}

fn validate_face_detection_config(config: &FaceDetectionModelConfig) -> Result<(), ModelPackError> {
    match config.adapter.as_str() {
        "decoded_boxes_v1" | "scrfd" | "yunet_opencv_compat" => {}
        _ => return Err(ModelPackError::InvalidManifest("face_detection.adapter")),
    }
    validate_pack_path(&config.model_path, "face_detection.model_path")?;
    require_text(&config.input_name, 120, "face_detection.input_name")?;
    require_text(
        &config.boxes_output_name,
        120,
        "face_detection.boxes_output_name",
    )?;
    require_text(
        &config.scores_output_name,
        120,
        "face_detection.scores_output_name",
    )?;
    if let Some(name) = &config.landmarks_output_name {
        require_text(name, 120, "face_detection.landmarks_output_name")?;
    }
    for name in &config.output_names {
        require_text(name, 120, "face_detection.output_names")?;
    }
    match config.box_coordinate_space.as_str() {
        "normalized" | "pixel" => {}
        _ => {
            return Err(ModelPackError::InvalidManifest(
                "face_detection.box_coordinate_space",
            ));
        }
    }
    match config.box_format.as_str() {
        "xywh" | "xyxy" => {}
        _ => return Err(ModelPackError::InvalidManifest("face_detection.box_format")),
    }
    if !(0.0..=1.0).contains(&config.score_threshold)
        || !(0.0..=1.0).contains(&config.min_face_size_ratio)
        || !(0.0..=1.0).contains(&config.nms_threshold)
        || !(1..=1_000).contains(&config.max_faces)
    {
        return Err(ModelPackError::InvalidManifest("face_detection.thresholds"));
    }
    Ok(())
}

fn validate_face_embedding_config(config: &FaceEmbeddingModelConfig) -> Result<(), ModelPackError> {
    match config.adapter.as_str() {
        "raw_embedding_v1" | "arcface" | "sface_opencv_compat" => {}
        _ => return Err(ModelPackError::InvalidManifest("face_embedding.adapter")),
    }
    validate_pack_path(&config.model_path, "face_embedding.model_path")?;
    require_text(&config.input_name, 120, "face_embedding.input_name")?;
    require_text(&config.output_name, 120, "face_embedding.output_name")?;
    if config.width == 0 || config.width > 4096 {
        return Err(ModelPackError::InvalidManifest("face_embedding.width"));
    }
    if config.height == 0 || config.height > 4096 {
        return Err(ModelPackError::InvalidManifest("face_embedding.height"));
    }
    match config.color_order.as_str() {
        "rgb" | "bgr" => {}
        _ => {
            return Err(ModelPackError::InvalidManifest(
                "face_embedding.color_order",
            ));
        }
    }
    match config.tensor_layout.as_str() {
        "nchw" | "nhwc" => {}
        _ => {
            return Err(ModelPackError::InvalidManifest(
                "face_embedding.tensor_layout",
            ));
        }
    }
    match config.alignment.as_str() {
        "bbox_crop" | "five_point" => {}
        _ => return Err(ModelPackError::InvalidManifest("face_embedding.alignment")),
    }
    if config
        .mean
        .iter()
        .any(|value| !value.is_finite() || value.abs() > 1_000.0)
    {
        return Err(ModelPackError::InvalidManifest("face_embedding.mean"));
    }
    if config
        .std
        .iter()
        .any(|value| !value.is_finite() || *value <= 0.0 || *value > 1_000.0)
    {
        return Err(ModelPackError::InvalidManifest("face_embedding.std"));
    }
    if !(0.0..=1.0).contains(&config.match_threshold) {
        return Err(ModelPackError::InvalidManifest(
            "face_embedding.match_threshold",
        ));
    }
    Ok(())
}

fn validate_image_preprocess(config: &ImagePreprocessConfig) -> Result<(), ModelPackError> {
    if config.width == 0 || config.width > 4096 {
        return Err(ModelPackError::InvalidManifest("image_preprocess.width"));
    }
    if config.height == 0 || config.height > 4096 {
        return Err(ModelPackError::InvalidManifest("image_preprocess.height"));
    }
    match config.color_order.as_str() {
        "rgb" | "bgr" => {}
        _ => {
            return Err(ModelPackError::InvalidManifest(
                "image_preprocess.color_order",
            ));
        }
    }
    match config.tensor_layout.as_str() {
        "nchw" | "nhwc" => {}
        _ => {
            return Err(ModelPackError::InvalidManifest(
                "image_preprocess.tensor_layout",
            ));
        }
    }
    if !config.mean.into_iter().all(f32::is_finite) {
        return Err(ModelPackError::InvalidManifest("image_preprocess.mean"));
    }
    if !config
        .std
        .into_iter()
        .all(|value| value.is_finite() && value > 0.0)
    {
        return Err(ModelPackError::InvalidManifest("image_preprocess.std"));
    }
    Ok(())
}

fn require_manifest_file(
    paths: &HashSet<&str>,
    path: &str,
    field: &'static str,
) -> Result<(), ModelPackError> {
    if paths.contains(path) {
        Ok(())
    } else {
        Err(ModelPackError::InvalidManifest(field))
    }
}

fn parse_kind(value: &str) -> Result<ModelPackKind, ModelPackError> {
    match value {
        "semantic_image_text" => Ok(ModelPackKind::SemanticImageText),
        "face_identity" => Ok(ModelPackKind::FaceIdentity),
        "face_detection" => Ok(ModelPackKind::FaceDetection),
        "face_embedding" => Ok(ModelPackKind::FaceEmbedding),
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
    DistanceMetric::from_db_str(value).ok_or(ModelPackError::InvalidManifest("distance_metric"))
}

fn require_text(value: &str, max_len: usize, field: &'static str) -> Result<(), ModelPackError> {
    if value.trim().is_empty() || value.len() > max_len {
        Err(ModelPackError::InvalidManifest(field))
    } else {
        Ok(())
    }
}

fn validate_pack_path(value: &str, field: &'static str) -> Result<(), ModelPackError> {
    validate_relative_str(value, 300).map_err(|_| ModelPackError::InvalidManifest(field))
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
