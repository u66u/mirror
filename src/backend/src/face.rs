//! Face indexing pipeline.
//!
//! Runtime code detects faces and embeds aligned crops. This module owns Mirror
//! side effects: load originals, choose active face model packs, persist
//! face rows/embeddings, and keep owner-local people assignments.

use std::{
    collections::HashMap,
    io::Cursor,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use image::{DynamicImage, GenericImageView, ImageFormat, Rgb, RgbImage, imageops::FilterType};
use ort::{
    session::{Session, SessionOutputs},
    value::Tensor,
};
use pgvector::Vector;
use serde_json::Value;
use sqlx::{PgPool, Row, types::Json};
use thiserror::Error;
use uuid::Uuid;

use crate::{
    config::MlDevicePreference,
    jobs::{self, JobError, JobKind, JobSpec, LeasedJob},
    media::{MediaToolError, decode_still_image, normalize_still_image_for_image_crate},
    ml::MlError,
    models::{
        self, FaceDetectionModelConfig, FaceEmbeddingModelConfig, ImagePreprocessConfig,
        ModelPackError, ModelPackKind, ModelPackManifest, validate_embedding_output,
    },
    onnx_embedder::{
        extract_output, model_pack_file_path, normalize_channel, open_session, ordered_channels,
    },
    storage::{ObjectStorage, StorageError, StorageKey, StorageKeyError},
};

const MAX_FACE_IMAGE_BYTES: usize = 25 * 1024 * 1024;
const FACE_CHIP_GENERATOR_VERSION: &str = "face-chip-v1-webp-1";

/// Shared face runtime handle for worker wiring.
pub type SharedFaceRuntime = Arc<dyn FaceRuntime + Send + Sync>;

/// Face indexing runtime request.
pub struct FaceRuntimeRequest<'a> {
    /// Original encoded bytes.
    pub bytes: &'a [u8],
    /// Original media type.
    pub media_type: &'a str,
    /// Installed detection model-pack ID.
    pub detection_model_pack_id: Uuid,
    /// Detection model-pack manifest.
    pub detection_manifest: &'a ModelPackManifest,
    /// Installed embedding model-pack ID.
    pub embedding_model_pack_id: Uuid,
    /// Embedding model-pack manifest.
    pub embedding_manifest: &'a ModelPackManifest,
}

/// Face model-pack runtime self-test request.
pub struct FaceSelfTestRequest<'a> {
    /// Self-test encoded image bytes.
    pub bytes: &'a [u8],
    /// Self-test image media type.
    pub media_type: &'a str,
    /// Installed model-pack ID.
    pub model_pack_id: Uuid,
    /// Installed model-pack manifest.
    pub manifest: &'a ModelPackManifest,
}

/// One detected and embedded face ready for DB storage.
#[derive(Debug, Clone, PartialEq)]
pub struct IndexedFace {
    /// Normalized bounding box.
    pub bbox: FaceBox,
    /// Detector confidence.
    pub quality: Option<f32>,
    /// Face identity embedding.
    pub embedding: Vec<f32>,
    /// Generated aligned/cropped face chip.
    pub chip: Option<FaceChipImage>,
}

/// Generated face chip image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FaceChipImage {
    /// Encoded image bytes.
    pub bytes: Vec<u8>,
    /// Pixel width.
    pub width: u32,
    /// Pixel height.
    pub height: u32,
    /// Stable storage format.
    pub format: &'static str,
}

/// Normalized face rectangle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FaceBox {
    /// Left coordinate in `0.0..=1.0`.
    pub left: f32,
    /// Top coordinate in `0.0..=1.0`.
    pub top: f32,
    /// Width in `0.0..=1.0`.
    pub width: f32,
    /// Height in `0.0..=1.0`.
    pub height: f32,
}

impl FaceBox {
    fn is_valid(self) -> bool {
        self.left.is_finite()
            && self.top.is_finite()
            && self.width.is_finite()
            && self.height.is_finite()
            && self.left >= 0.0
            && self.top >= 0.0
            && self.width > 0.0
            && self.height > 0.0
            && self.left + self.width <= 1.0001
            && self.top + self.height <= 1.0001
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct FaceLandmarks {
    points: [[f32; 2]; 5],
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct DetectedFace {
    bbox: FaceBox,
    quality: f32,
    landmarks: Option<FaceLandmarks>,
}

/// Blocking face runtime boundary.
pub trait FaceRuntime {
    /// Detects and embeds faces for one asset original.
    fn detect_and_embed(
        &self,
        request: FaceRuntimeRequest<'_>,
    ) -> Result<Vec<IndexedFace>, FaceIndexError>;

    /// Runs one face model-pack self-test and returns deterministic f32 output.
    fn self_test_output(
        &self,
        _request: FaceSelfTestRequest<'_>,
    ) -> Result<Vec<f32>, FaceIndexError> {
        Err(FaceIndexError::RuntimeUnavailable)
    }
}

/// ONNX-backed face detection/embedding runtime.
pub struct OnnxFaceRuntime {
    storage_root: PathBuf,
    device: MlDevicePreference,
    heif_convert_path: PathBuf,
    detection_sessions: Mutex<HashMap<Uuid, Arc<Mutex<Session>>>>,
    embedding_sessions: Mutex<HashMap<Uuid, Arc<Mutex<Session>>>>,
}

impl OnnxFaceRuntime {
    /// Creates a lazy ONNX face runtime.
    #[must_use]
    pub fn new(storage_root: PathBuf, device: MlDevicePreference) -> Self {
        Self::with_heif_converter(storage_root, device, "heif-convert")
    }

    /// Creates a lazy ONNX face runtime with an explicit HEIC/HEIF converter.
    #[must_use]
    pub fn with_heif_converter(
        storage_root: PathBuf,
        device: MlDevicePreference,
        heif_convert_path: impl Into<PathBuf>,
    ) -> Self {
        Self {
            storage_root,
            device,
            heif_convert_path: heif_convert_path.into(),
            detection_sessions: Mutex::new(HashMap::new()),
            embedding_sessions: Mutex::new(HashMap::new()),
        }
    }

    fn self_test_detector(
        &self,
        request: FaceSelfTestRequest<'_>,
    ) -> Result<Vec<f32>, FaceIndexError> {
        let config = request
            .manifest
            .face_detection
            .as_ref()
            .ok_or(ModelPackError::InvalidManifest("face_detection"))?;
        let detector = detector_adapter(config, request.manifest)?;
        let image = decode_face_image(request.bytes, request.media_type, &self.heif_convert_path)?;
        let (inputs, ctx) = detector.preprocess(&image)?;
        let session = self.detection_session(request.model_pack_id, request.manifest)?;
        let outputs = {
            let mut session = session
                .lock()
                .map_err(|_| FaceIndexError::RuntimeUnavailable)?;
            run_onnx(&mut session, inputs, &detector.output_names())?
        };
        let faces = detector.postprocess(&outputs, &ctx)?;
        Ok(flatten_detected_faces_for_self_test(&faces))
    }

    fn self_test_embedder(
        &self,
        request: FaceSelfTestRequest<'_>,
    ) -> Result<Vec<f32>, FaceIndexError> {
        let config = request
            .manifest
            .face_embedding
            .as_ref()
            .ok_or(ModelPackError::InvalidManifest("face_embedding"))?;
        let embedder = embedder_adapter(config)?;
        let image = decode_face_image(request.bytes, request.media_type, &self.heif_convert_path)?;
        let spec = embedder.chip_spec();
        let chip = FaceChip {
            image: image
                .resize_exact(spec.width, spec.height, FilterType::Triangle)
                .to_rgb8(),
            source_bbox: FaceBox {
                left: 0.0,
                top: 0.0,
                width: 1.0,
                height: 1.0,
            },
        };
        let inputs = embedder.preprocess_chip(&chip)?;
        let session = self.embedding_session(request.model_pack_id, request.manifest)?;
        let outputs = {
            let mut session = session
                .lock()
                .map_err(|_| FaceIndexError::RuntimeUnavailable)?;
            run_onnx(&mut session, inputs, &embedder.output_names())?
        };
        embedder.postprocess(&outputs)
    }

    fn detection_session(
        &self,
        model_pack_id: Uuid,
        manifest: &ModelPackManifest,
    ) -> Result<Arc<Mutex<Session>>, FaceIndexError> {
        let config = manifest
            .face_detection
            .as_ref()
            .ok_or(ModelPackError::InvalidManifest("face_detection"))?;
        let mut sessions = self
            .detection_sessions
            .lock()
            .map_err(|_| FaceIndexError::RuntimeUnavailable)?;
        if let Some(session) = sessions.get(&model_pack_id) {
            return Ok(Arc::clone(session));
        }
        let path = model_pack_file_path(&self.storage_root, model_pack_id, &config.model_path)?;
        let session = Arc::new(Mutex::new(open_session(&path, self.device)?));
        sessions.insert(model_pack_id, Arc::clone(&session));
        Ok(session)
    }

    fn embedding_session(
        &self,
        model_pack_id: Uuid,
        manifest: &ModelPackManifest,
    ) -> Result<Arc<Mutex<Session>>, FaceIndexError> {
        let config = manifest
            .face_embedding
            .as_ref()
            .ok_or(ModelPackError::InvalidManifest("face_embedding"))?;
        let mut sessions = self
            .embedding_sessions
            .lock()
            .map_err(|_| FaceIndexError::RuntimeUnavailable)?;
        if let Some(session) = sessions.get(&model_pack_id) {
            return Ok(Arc::clone(session));
        }
        let path = model_pack_file_path(&self.storage_root, model_pack_id, &config.model_path)?;
        let session = Arc::new(Mutex::new(open_session(&path, self.device)?));
        sessions.insert(model_pack_id, Arc::clone(&session));
        Ok(session)
    }
}

impl FaceRuntime for OnnxFaceRuntime {
    fn detect_and_embed(
        &self,
        request: FaceRuntimeRequest<'_>,
    ) -> Result<Vec<IndexedFace>, FaceIndexError> {
        let detection_config = request
            .detection_manifest
            .face_detection
            .as_ref()
            .ok_or(ModelPackError::InvalidManifest("face_detection"))?;
        let embedding_config = request
            .embedding_manifest
            .face_embedding
            .as_ref()
            .ok_or(ModelPackError::InvalidManifest("face_embedding"))?;
        let detector = detector_adapter(detection_config, request.detection_manifest)?;
        let embedder = embedder_adapter(embedding_config)?;
        let image = decode_face_image(request.bytes, request.media_type, &self.heif_convert_path)?;
        let (inputs, detector_ctx) = detector.preprocess(&image)?;
        let detection_session =
            self.detection_session(request.detection_model_pack_id, request.detection_manifest)?;
        let detection_outputs = {
            let mut session = detection_session
                .lock()
                .map_err(|_| FaceIndexError::RuntimeUnavailable)?;
            run_onnx(&mut session, inputs, &detector.output_names())?
        };
        let faces = detector.postprocess(&detection_outputs, &detector_ctx)?;
        let embedding_session =
            self.embedding_session(request.embedding_model_pack_id, request.embedding_manifest)?;
        let mut indexed = Vec::with_capacity(faces.len());
        for face in faces {
            let chip = make_face_chip(&image, face, embedder.chip_spec())?;
            let chip_image = encode_face_chip(&chip.image)?;
            let inputs = embedder.preprocess_chip(&chip)?;
            let mut session = embedding_session
                .lock()
                .map_err(|_| FaceIndexError::RuntimeUnavailable)?;
            let outputs = run_onnx(&mut session, inputs, &embedder.output_names())?;
            let embedding = embedder.postprocess(&outputs)?;
            indexed.push(IndexedFace {
                bbox: chip.source_bbox,
                quality: Some(face.quality),
                embedding,
                chip: Some(chip_image),
            });
        }

        Ok(indexed)
    }

    fn self_test_output(
        &self,
        request: FaceSelfTestRequest<'_>,
    ) -> Result<Vec<f32>, FaceIndexError> {
        let validated = models::validate_model_pack_manifest(request.manifest)?;
        match validated.kind {
            ModelPackKind::FaceDetection => self.self_test_detector(request),
            ModelPackKind::FaceEmbedding => self.self_test_embedder(request),
            ModelPackKind::FaceIdentity => {
                let faces = self.detect_and_embed(FaceRuntimeRequest {
                    bytes: request.bytes,
                    media_type: request.media_type,
                    detection_model_pack_id: request.model_pack_id,
                    detection_manifest: request.manifest,
                    embedding_model_pack_id: request.model_pack_id,
                    embedding_manifest: request.manifest,
                })?;
                Ok(flatten_indexed_faces_for_self_test(&faces))
            }
            ModelPackKind::SemanticImageText => Err(ModelPackError::InvalidManifest("kind").into()),
        }
    }
}

/// Face indexing failure.
#[derive(Debug, Error)]
pub enum FaceIndexError {
    /// Worker does not have a configured face runtime.
    #[error("face indexing runtime is unavailable")]
    RuntimeUnavailable,
    /// Wrong queue kind reached the face handler.
    #[error("unsupported face job kind")]
    UnsupportedJobKind,
    /// Job payload is missing required UUID fields.
    #[error("invalid face job payload")]
    InvalidJobPayload,
    /// Asset or active model pack no longer exists.
    #[error("face asset or model pack not found")]
    NotFound,
    /// Asset media type is not supported for face indexing.
    #[error("unsupported face media type")]
    UnsupportedMediaType,
    /// Original storage key is invalid.
    #[error("invalid face storage key")]
    InvalidStorageKey(#[from] StorageKeyError),
    /// Object storage failed.
    #[error("face storage error")]
    Storage(#[from] StorageError),
    /// Model-pack or embedding output is invalid.
    #[error("face model error")]
    Model(#[from] ModelPackError),
    /// Shared ML boundary failed.
    #[error("face ml error")]
    Ml(#[from] MlError),
    /// Still-image conversion failed before face inference.
    #[error("face image conversion error")]
    ImageConversion(MediaToolError),
    /// Database failed.
    #[error("face database error")]
    Database(#[from] sqlx::Error),
    /// Queue operation failed.
    #[error("face queue error")]
    Jobs(#[from] JobError),
}

#[derive(Debug)]
struct FaceIndexPayload {
    asset_id: Uuid,
    reindex_run_id: Option<Uuid>,
    detection_model_pack_id: Option<Uuid>,
    embedding_model_pack_id: Option<Uuid>,
}

struct AssetOriginal {
    asset_id: Uuid,
    owner_id: i16,
    storage_key: String,
    media_type: String,
}

struct FaceModelPack {
    id: Uuid,
    manifest: ModelPackManifest,
}

/// Enqueues face indexing for one promoted asset.
pub async fn enqueue_face_index(pool: &PgPool, asset_id: Uuid) -> Result<(), FaceIndexError> {
    let mut tx = pool.begin().await?;
    jobs::enqueue_in_tx(
        &mut tx,
        JobSpec::immediate(
            JobKind::IndexFaces,
            serde_json::json!({ "asset_id": asset_id }),
            format!("face-index:{asset_id}"),
        ),
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

/// Runs one face indexing job.
pub async fn run_face_index_job(
    pool: &PgPool,
    storage: &ObjectStorage,
    runtime: &SharedFaceRuntime,
    job: &LeasedJob,
) -> Result<(), FaceIndexError> {
    if job.kind != JobKind::IndexFaces {
        return Err(FaceIndexError::UnsupportedJobKind);
    }
    let payload = FaceIndexPayload::from_json(&job.payload)?;
    let Some(asset) = load_face_asset_original(pool, payload.asset_id).await? else {
        return Ok(());
    };
    if !is_supported_face_media_type(&asset.media_type) {
        return Err(FaceIndexError::UnsupportedMediaType);
    }
    let (detection_pack, embedding_pack) = load_face_model_packs(pool, &payload).await?;
    let storage_key = StorageKey::new(&asset.storage_key)?;
    let bytes = storage
        .read_bounded(&storage_key, MAX_FACE_IMAGE_BYTES)
        .await
        .map_err(FaceIndexError::Storage)?;

    let runtime = Arc::clone(runtime);
    let media_type = asset.media_type.clone();
    let detection_manifest = detection_pack.manifest.clone();
    let embedding_manifest = embedding_pack.manifest.clone();
    let detection_model_pack_id = detection_pack.id;
    let embedding_model_pack_id = embedding_pack.id;
    let faces = tokio::task::spawn_blocking(move || {
        runtime.detect_and_embed(FaceRuntimeRequest {
            bytes: &bytes,
            media_type: &media_type,
            detection_model_pack_id,
            detection_manifest: &detection_manifest,
            embedding_model_pack_id,
            embedding_manifest: &embedding_manifest,
        })
    })
    .await
    .map_err(|_| FaceIndexError::RuntimeUnavailable)??;

    persist_faces(
        pool,
        storage,
        &asset,
        detection_pack.id,
        &embedding_pack,
        faces,
    )
    .await?;
    if let Some(reindex_run_id) = payload.reindex_run_id {
        models::record_reindex_asset_result(pool, reindex_run_id, asset.asset_id, true, None)
            .await?;
    }
    Ok(())
}

/// Runs configured runtime self-tests for a face model pack.
pub async fn run_face_model_pack_self_tests(
    pool: &PgPool,
    storage: &ObjectStorage,
    runtime: &SharedFaceRuntime,
    model_pack_id: Uuid,
) -> Result<models::InstalledModelPack, FaceIndexError> {
    let manifest = load_face_model_pack_manifest(pool, model_pack_id).await?;
    let validated = models::validate_model_pack_manifest(&manifest)?;
    if !matches!(
        validated.kind,
        ModelPackKind::FaceDetection | ModelPackKind::FaceEmbedding | ModelPackKind::FaceIdentity
    ) {
        return Err(ModelPackError::InvalidManifest("kind").into());
    }

    for self_test in &manifest.self_tests {
        let storage_key = StorageKey::model_pack_file(model_pack_id, &self_test.input_path)?;
        let bytes = storage
            .read_bounded(&storage_key, MAX_FACE_IMAGE_BYTES)
            .await
            .map_err(FaceIndexError::Storage)?;
        let media_type = face_self_test_media_type(&self_test.input_path)?;
        let runtime = Arc::clone(runtime);
        let manifest = manifest.clone();
        let output = tokio::task::spawn_blocking(move || {
            runtime.self_test_output(FaceSelfTestRequest {
                bytes: &bytes,
                media_type,
                model_pack_id,
                manifest: &manifest,
            })
        })
        .await
        .map_err(|_| FaceIndexError::RuntimeUnavailable)?;
        match output {
            Ok(values) => {
                if crate::ml::sha256_f32_values(&values) != self_test.expected_output_sha256 {
                    return record_face_self_test_failure(
                        pool,
                        model_pack_id,
                        "self-test output mismatch",
                    )
                    .await;
                }
            }
            Err(error) if face_self_test_runtime_failure(&error) => {
                return record_face_self_test_failure(pool, model_pack_id, &error.to_string())
                    .await;
            }
            Err(error) => return Err(error),
        }
    }

    models::record_model_pack_self_test(pool, model_pack_id, true, None)
        .await
        .map_err(FaceIndexError::Model)
}

async fn load_face_model_pack_manifest(
    pool: &PgPool,
    model_pack_id: Uuid,
) -> Result<ModelPackManifest, FaceIndexError> {
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
    .ok_or(FaceIndexError::NotFound)
}

async fn record_face_self_test_failure(
    pool: &PgPool,
    model_pack_id: Uuid,
    message: &str,
) -> Result<models::InstalledModelPack, FaceIndexError> {
    models::record_model_pack_self_test(pool, model_pack_id, false, Some(message))
        .await
        .map_err(FaceIndexError::Model)
}

fn face_self_test_media_type(path: &str) -> Result<&'static str, FaceIndexError> {
    if path.ends_with(".jpg") || path.ends_with(".jpeg") {
        Ok("image/jpeg")
    } else if path.ends_with(".png") {
        Ok("image/png")
    } else if path.ends_with(".gif") {
        Ok("image/gif")
    } else if path.ends_with(".webp") {
        Ok("image/webp")
    } else if path.ends_with(".heic") {
        Ok("image/heic")
    } else if path.ends_with(".heif") {
        Ok("image/heif")
    } else {
        Err(ModelPackError::InvalidManifest("self_test.input_path").into())
    }
}

fn face_self_test_runtime_failure(error: &FaceIndexError) -> bool {
    matches!(
        error,
        FaceIndexError::UnsupportedMediaType
            | FaceIndexError::Model(ModelPackError::InvalidManifest(_))
            | FaceIndexError::Model(ModelPackError::InvalidEmbedding(_))
            | FaceIndexError::Ml(MlError::InvalidImage)
            | FaceIndexError::ImageConversion(_)
            | FaceIndexError::RuntimeUnavailable
    )
}

impl FaceIndexPayload {
    fn from_json(value: &Value) -> Result<Self, FaceIndexError> {
        Ok(Self {
            asset_id: face_uuid_field(value, "asset_id")?,
            reindex_run_id: optional_face_uuid_field(value, "reindex_run_id")?,
            detection_model_pack_id: optional_face_uuid_field(value, "detection_model_pack_id")?,
            embedding_model_pack_id: optional_face_uuid_field(value, "embedding_model_pack_id")?,
        })
    }
}

async fn load_face_asset_original(
    pool: &PgPool,
    asset_id: Uuid,
) -> Result<Option<AssetOriginal>, FaceIndexError> {
    let row = sqlx::query(
        r#"
        SELECT a.id, a.owner_id, o.storage_key, o.media_type
        FROM assets a
        JOIN originals o ON o.id = a.original_id
        WHERE a.id = $1
          AND a.trashed_at IS NULL
        "#,
    )
    .bind(asset_id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|row| AssetOriginal {
        asset_id: row.get("id"),
        owner_id: row.get("owner_id"),
        storage_key: row.get("storage_key"),
        media_type: row.get("media_type"),
    }))
}

async fn load_face_model_packs(
    pool: &PgPool,
    payload: &FaceIndexPayload,
) -> Result<(FaceModelPack, FaceModelPack), FaceIndexError> {
    if let (Some(detection_id), Some(embedding_id)) = (
        payload.detection_model_pack_id,
        payload.embedding_model_pack_id,
    ) {
        return Ok((
            load_model_pack(pool, detection_id, true, false).await?,
            load_model_pack(pool, embedding_id, false, true).await?,
        ));
    }
    if let Some(identity) = active_model_pack(pool, "face_identity").await? {
        let detection = FaceModelPack {
            id: identity.id,
            manifest: identity.manifest.clone(),
        };
        return Ok((detection, identity));
    }
    let detection = match payload.detection_model_pack_id {
        Some(id) => load_model_pack(pool, id, true, false).await?,
        None => active_model_pack(pool, "face_detection")
            .await?
            .ok_or(FaceIndexError::NotFound)?,
    };
    let embedding = match payload.embedding_model_pack_id {
        Some(id) => load_model_pack(pool, id, false, true).await?,
        None => active_model_pack(pool, "face_embedding")
            .await?
            .ok_or(FaceIndexError::NotFound)?,
    };
    Ok((detection, embedding))
}

async fn load_model_pack(
    pool: &PgPool,
    model_pack_id: Uuid,
    requires_detection: bool,
    requires_embedding: bool,
) -> Result<FaceModelPack, FaceIndexError> {
    let row = sqlx::query(
        r#"
        SELECT id, manifest
        FROM model_packs
        WHERE id = $1
          AND self_test_status = 'passed'
        "#,
    )
    .bind(model_pack_id)
    .fetch_optional(pool)
    .await?
    .ok_or(FaceIndexError::NotFound)?;
    let pack = FaceModelPack {
        id: row.get("id"),
        manifest: row.get::<Json<ModelPackManifest>, _>("manifest").0,
    };
    validate_face_pack(&pack.manifest, requires_detection, requires_embedding)?;
    Ok(pack)
}

async fn active_model_pack(
    pool: &PgPool,
    kind: &str,
) -> Result<Option<FaceModelPack>, FaceIndexError> {
    let row = sqlx::query(
        r#"
        SELECT id, manifest
        FROM model_packs
        WHERE kind = $1
          AND status = 'active'
          AND self_test_status = 'passed'
        ORDER BY activated_at DESC NULLS LAST, updated_at DESC, id ASC
        LIMIT 1
        "#,
    )
    .bind(kind)
    .fetch_optional(pool)
    .await?;
    row.map(|row| {
        let pack = FaceModelPack {
            id: row.get("id"),
            manifest: row.get::<Json<ModelPackManifest>, _>("manifest").0,
        };
        let requires_detection = matches!(kind, "face_detection" | "face_identity");
        let requires_embedding = matches!(kind, "face_embedding" | "face_identity");
        validate_face_pack(&pack.manifest, requires_detection, requires_embedding)?;
        Ok(pack)
    })
    .transpose()
}

fn validate_face_pack(
    manifest: &ModelPackManifest,
    requires_detection: bool,
    requires_embedding: bool,
) -> Result<(), FaceIndexError> {
    let validated = models::validate_model_pack_manifest(manifest)?;
    if validated.kind != ModelPackKind::FaceIdentity
        && requires_detection
        && validated.kind != ModelPackKind::FaceDetection
    {
        return Err(ModelPackError::InvalidManifest("kind").into());
    }
    if validated.kind != ModelPackKind::FaceIdentity
        && requires_embedding
        && validated.kind != ModelPackKind::FaceEmbedding
    {
        return Err(ModelPackError::InvalidManifest("kind").into());
    }
    Ok(())
}

async fn persist_faces(
    pool: &PgPool,
    storage: &ObjectStorage,
    asset: &AssetOriginal,
    detection_model_pack_id: Uuid,
    embedding_pack: &FaceModelPack,
    faces: Vec<IndexedFace>,
) -> Result<(), FaceIndexError> {
    let mut tx = pool.begin().await?;
    sqlx::query(
        r#"
        DELETE FROM face_occurrences fo
        WHERE fo.asset_id = $1
          AND NOT EXISTS (
              SELECT 1 FROM person_faces pf
              WHERE pf.face_occurrence_id = fo.id
          )
        "#,
    )
    .bind(asset.asset_id)
    .execute(&mut *tx)
    .await?;

    let embedding_config = embedding_pack
        .manifest
        .face_embedding
        .as_ref()
        .ok_or(ModelPackError::InvalidManifest("face_embedding"))?;
    for face in faces {
        if !face.bbox.is_valid() {
            return Err(ModelPackError::InvalidManifest("face.bbox").into());
        }
        let embedding = validate_embedding_output(&embedding_pack.manifest, face.embedding)?;
        let person_id = best_person_match(
            &mut tx,
            asset.owner_id,
            embedding_pack.id,
            embedding.values(),
            embedding_config.match_threshold,
        )
        .await?
        .unwrap_or_else(Uuid::now_v7);
        ensure_person(&mut tx, person_id, asset.owner_id).await?;
        let face_id = Uuid::now_v7();
        let chip = if let Some(chip) = face.chip {
            let key = StorageKey::face_chip(face_id, chip.format, FACE_CHIP_GENERATOR_VERSION)?;
            storage.write(&key, chip.bytes).await?;
            Some((
                key.as_str().to_owned(),
                i32::try_from(chip.width)
                    .map_err(|_| ModelPackError::InvalidManifest("face.chip.width"))?,
                i32::try_from(chip.height)
                    .map_err(|_| ModelPackError::InvalidManifest("face.chip.height"))?,
                chip.format.to_owned(),
            ))
        } else {
            None
        };
        sqlx::query(
            r#"
            INSERT INTO face_occurrences (
                id, asset_id, owner_id, detection_model_pack_id,
                bbox_left, bbox_top, bbox_width, bbox_height, quality, review_state,
                chip_storage_key, chip_width, chip_height, chip_format
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, 'assigned', $10, $11, $12, $13)
            "#,
        )
        .bind(face_id)
        .bind(asset.asset_id)
        .bind(asset.owner_id)
        .bind(detection_model_pack_id)
        .bind(face.bbox.left)
        .bind(face.bbox.top)
        .bind(face.bbox.width)
        .bind(face.bbox.height)
        .bind(face.quality)
        .bind(chip.as_ref().map(|chip| chip.0.as_str()))
        .bind(chip.as_ref().map(|chip| chip.1))
        .bind(chip.as_ref().map(|chip| chip.2))
        .bind(chip.as_ref().map(|chip| chip.3.as_str()))
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            r#"
            INSERT INTO face_embeddings (
                face_occurrence_id, owner_id, model_pack_id, embedding, embedding_dimension
            )
            VALUES ($1, $2, $3, $4, $5)
            "#,
        )
        .bind(face_id)
        .bind(asset.owner_id)
        .bind(embedding_pack.id)
        .bind(Vector::from(embedding.values().to_vec()))
        .bind(
            i32::try_from(embedding.values().len()).map_err(|_| {
                ModelPackError::InvalidManifest("face_embedding.embedding_dimension")
            })?,
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            r#"
            INSERT INTO person_faces (person_id, face_occurrence_id, owner_id)
            VALUES ($1, $2, $3)
            "#,
        )
        .bind(person_id)
        .bind(face_id)
        .bind(asset.owner_id)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

async fn ensure_person(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    person_id: Uuid,
    owner_id: i16,
) -> Result<(), FaceIndexError> {
    sqlx::query(
        r#"
        INSERT INTO people (id, owner_id, display_name, review_status)
        VALUES ($1, $2, NULL, 'unreviewed')
        ON CONFLICT (id) DO NOTHING
        "#,
    )
    .bind(person_id)
    .bind(owner_id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn best_person_match(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    owner_id: i16,
    model_pack_id: Uuid,
    embedding: &[f32],
    threshold: f32,
) -> Result<Option<Uuid>, FaceIndexError> {
    let rows = sqlx::query(
        r#"
        SELECT pf.person_id, fe.embedding
        FROM person_faces pf
        JOIN face_embeddings fe
          ON fe.face_occurrence_id = pf.face_occurrence_id
         AND fe.owner_id = pf.owner_id
        JOIN people p
          ON p.id = pf.person_id
         AND p.owner_id = pf.owner_id
        WHERE pf.owner_id = $1
          AND pf.review_state = 'assigned'
          AND p.review_status <> 'hidden'
          AND fe.model_pack_id = $2
        "#,
    )
    .bind(owner_id)
    .bind(model_pack_id)
    .fetch_all(&mut **tx)
    .await?;

    let mut best = None;
    for row in rows {
        let person_id: Uuid = row.get("person_id");
        let other: Vector = row.get("embedding");
        let Some(score) = cosine_similarity(embedding, other.as_slice()) else {
            continue;
        };
        if score >= threshold && best.is_none_or(|(_, best_score)| score > best_score) {
            best = Some((person_id, score));
        }
    }
    Ok(best.map(|(person_id, _)| person_id))
}

struct OnnxInput {
    name: String,
    shape: Vec<usize>,
    values: Vec<f32>,
}

struct OnnxInputs {
    tensors: Vec<OnnxInput>,
}

impl OnnxInputs {
    fn single(name: impl Into<String>, shape: Vec<usize>, values: Vec<f32>) -> Self {
        Self {
            tensors: vec![OnnxInput {
                name: name.into(),
                shape,
                values,
            }],
        }
    }
}

struct OnnxOutput {
    values: Vec<f32>,
}

struct OnnxOutputs {
    tensors: HashMap<String, OnnxOutput>,
}

impl OnnxOutputs {
    fn values(&self, name: &str, field: &'static str) -> Result<&[f32], FaceIndexError> {
        self.tensors
            .get(name)
            .map(|output| output.values.as_slice())
            .ok_or(ModelPackError::InvalidManifest(field).into())
    }
}

#[derive(Debug, Clone, Copy)]
struct DetectorPreprocessCtx {
    network_width: u32,
    network_height: u32,
    resized_width: f32,
    resized_height: f32,
    x_offset: f32,
    y_offset: f32,
}

#[derive(Debug, Clone, Copy)]
struct FaceChipSpec {
    width: u32,
    height: u32,
    alignment: FaceAlignmentMode,
    color_order_bgr: bool,
    tensor_layout: FaceTensorLayout,
    mean: [f32; 3],
    std: [f32; 3],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FaceAlignmentMode {
    ArcFace5Point,
    BboxCrop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FaceTensorLayout {
    Nchw,
    Nhwc,
}

struct FaceChip {
    image: RgbImage,
    source_bbox: FaceBox,
}

trait FaceDetectorAdapter {
    fn output_names(&self) -> Vec<String>;
    fn preprocess(
        &self,
        image: &DynamicImage,
    ) -> Result<(OnnxInputs, DetectorPreprocessCtx), FaceIndexError>;
    fn postprocess(
        &self,
        outputs: &OnnxOutputs,
        ctx: &DetectorPreprocessCtx,
    ) -> Result<Vec<DetectedFace>, FaceIndexError>;
}

trait FaceEmbedderAdapter {
    fn output_names(&self) -> Vec<String>;
    fn chip_spec(&self) -> FaceChipSpec;
    fn preprocess_chip(&self, chip: &FaceChip) -> Result<OnnxInputs, FaceIndexError>;
    fn postprocess(&self, outputs: &OnnxOutputs) -> Result<Vec<f32>, FaceIndexError>;
}

struct DecodedBoxesDetectorAdapter<'a> {
    config: &'a FaceDetectionModelConfig,
    preprocess: &'a ImagePreprocessConfig,
}

impl FaceDetectorAdapter for DecodedBoxesDetectorAdapter<'_> {
    fn output_names(&self) -> Vec<String> {
        let mut names = vec![
            self.config.boxes_output_name.clone(),
            self.config.scores_output_name.clone(),
        ];
        if let Some(name) = &self.config.landmarks_output_name {
            names.push(name.clone());
        }
        names
    }

    fn preprocess(
        &self,
        image: &DynamicImage,
    ) -> Result<(OnnxInputs, DetectorPreprocessCtx), FaceIndexError> {
        let (shape, values) = preprocess_image_pixels(image, self.preprocess)?;
        Ok((
            OnnxInputs::single(self.config.input_name.as_str(), shape, values),
            DetectorPreprocessCtx {
                network_width: self.preprocess.width,
                network_height: self.preprocess.height,
                resized_width: self.preprocess.width as f32,
                resized_height: self.preprocess.height as f32,
                x_offset: 0.0,
                y_offset: 0.0,
            },
        ))
    }

    fn postprocess(
        &self,
        outputs: &OnnxOutputs,
        ctx: &DetectorPreprocessCtx,
    ) -> Result<Vec<DetectedFace>, FaceIndexError> {
        let boxes = outputs.values(
            &self.config.boxes_output_name,
            "face_detection.boxes_output_name",
        )?;
        let scores = outputs.values(
            &self.config.scores_output_name,
            "face_detection.scores_output_name",
        )?;
        let landmarks = self
            .config
            .landmarks_output_name
            .as_deref()
            .map(|name| outputs.values(name, "face_detection.landmarks_output_name"))
            .transpose()?;
        select_faces(
            boxes,
            scores,
            landmarks,
            self.config,
            ctx.network_width,
            ctx.network_height,
        )
    }
}

struct ScrfdDetectorAdapter<'a> {
    config: &'a FaceDetectionModelConfig,
    preprocess: &'a ImagePreprocessConfig,
}

#[derive(Debug, Clone)]
struct ScrfdOutputMap {
    stride: u32,
    score_name: String,
    bbox_name: String,
    kps_name: Option<String>,
}

impl FaceDetectorAdapter for ScrfdDetectorAdapter<'_> {
    fn output_names(&self) -> Vec<String> {
        scrfd_output_maps(self.config)
            .into_iter()
            .flat_map(|map| {
                let mut names = vec![map.score_name, map.bbox_name];
                if let Some(name) = map.kps_name {
                    names.push(name);
                }
                names
            })
            .collect()
    }

    fn preprocess(
        &self,
        image: &DynamicImage,
    ) -> Result<(OnnxInputs, DetectorPreprocessCtx), FaceIndexError> {
        let (shape, values, ctx) = preprocess_letterboxed_image_pixels(image, self.preprocess)?;
        Ok((
            OnnxInputs::single(self.config.input_name.as_str(), shape, values),
            ctx,
        ))
    }

    fn postprocess(
        &self,
        outputs: &OnnxOutputs,
        ctx: &DetectorPreprocessCtx,
    ) -> Result<Vec<DetectedFace>, FaceIndexError> {
        let mut candidates = Vec::new();
        for map in scrfd_output_maps(self.config) {
            let scores = outputs.values(&map.score_name, "face_detection.output_names")?;
            let bboxes = outputs.values(&map.bbox_name, "face_detection.output_names")?;
            if !bboxes.len().is_multiple_of(4) {
                return Err(ModelPackError::InvalidManifest("face_detection.output_names").into());
            }
            let face_count = bboxes.len() / 4;
            let kps = map
                .kps_name
                .as_deref()
                .map(|name| outputs.values(name, "face_detection.output_names"))
                .transpose()?;
            if let Some(kps) = kps
                && kps.len() != face_count * 10
            {
                return Err(ModelPackError::InvalidManifest("face_detection.output_names").into());
            }
            for index in 0..face_count {
                let Some(score) = detection_score(scores, index, face_count) else {
                    continue;
                };
                if score < self.config.score_threshold {
                    continue;
                }
                let bbox = scrfd_bbox_from_distance(
                    index,
                    map.stride,
                    &bboxes[index * 4..index * 4 + 4],
                    ctx,
                );
                let landmarks = kps
                    .map(|values| {
                        scrfd_landmarks_from_distance(
                            index,
                            map.stride,
                            &values[index * 10..index * 10 + 10],
                            ctx,
                        )
                    })
                    .transpose()?;
                let face = DetectedFace {
                    bbox,
                    quality: score,
                    landmarks,
                };
                if detection_passes_filters(&face, self.config) {
                    candidates.push(face);
                }
            }
        }
        candidates.sort_by(|left, right| right.quality.total_cmp(&left.quality));
        let mut selected: Vec<DetectedFace> = Vec::new();
        for candidate in candidates {
            if selected.iter().all(|existing| {
                face_iou(candidate.bbox, existing.bbox) <= self.config.nms_threshold
            }) {
                selected.push(candidate);
            }
            if selected.len() >= usize::try_from(self.config.max_faces).unwrap_or(usize::MAX) {
                break;
            }
        }
        Ok(selected)
    }
}

fn scrfd_output_maps(config: &FaceDetectionModelConfig) -> Vec<ScrfdOutputMap> {
    if config.output_names.is_empty() {
        return [8_u32, 16, 32]
            .into_iter()
            .map(|stride| ScrfdOutputMap {
                stride,
                score_name: format!("score_{stride}"),
                bbox_name: format!("bbox_{stride}"),
                kps_name: Some(format!("kps_{stride}")),
            })
            .collect();
    }

    let mut named = Vec::new();
    for stride in [8_u32, 16, 32] {
        let score_name = format!("score_{stride}");
        let bbox_name = format!("bbox_{stride}");
        if config.output_names.iter().any(|name| name == &score_name)
            && config.output_names.iter().any(|name| name == &bbox_name)
        {
            let kps_name = format!("kps_{stride}");
            named.push(ScrfdOutputMap {
                stride,
                score_name,
                bbox_name,
                kps_name: config
                    .output_names
                    .iter()
                    .any(|name| name == &kps_name)
                    .then_some(kps_name),
            });
        }
    }
    if !named.is_empty() {
        return named;
    }

    let strides = [8_u32, 16, 32];
    if config.output_names.len() >= 6 {
        return strides
            .into_iter()
            .enumerate()
            .map(|(index, stride)| ScrfdOutputMap {
                stride,
                score_name: config.output_names[index].clone(),
                bbox_name: config.output_names[index + 3].clone(),
                kps_name: config.output_names.get(index + 6).cloned(),
            })
            .collect();
    }
    Vec::new()
}

fn scrfd_anchor(index: usize, stride: u32, ctx: &DetectorPreprocessCtx) -> (f32, f32) {
    let stride_usize = usize::try_from(stride).unwrap_or(1);
    let width = usize::try_from(ctx.network_width / stride)
        .unwrap_or(1)
        .max(1);
    let anchor_index = index / 2;
    let x = (anchor_index % width) * stride_usize;
    let y = (anchor_index / width) * stride_usize;
    (x as f32, y as f32)
}

fn scrfd_bbox_from_distance(
    index: usize,
    stride: u32,
    distance: &[f32],
    ctx: &DetectorPreprocessCtx,
) -> FaceBox {
    let (anchor_x, anchor_y) = scrfd_anchor(index, stride, ctx);
    let stride = stride as f32;
    let x1 = distance[0].mul_add(-stride, anchor_x);
    let y1 = distance[1].mul_add(-stride, anchor_y);
    let x2 = distance[2].mul_add(stride, anchor_x);
    let y2 = distance[3].mul_add(stride, anchor_y);
    normalized_letterbox_box(x1, y1, x2, y2, ctx)
}

fn scrfd_landmarks_from_distance(
    index: usize,
    stride: u32,
    distance: &[f32],
    ctx: &DetectorPreprocessCtx,
) -> Result<FaceLandmarks, FaceIndexError> {
    let (anchor_x, anchor_y) = scrfd_anchor(index, stride, ctx);
    let stride = stride as f32;
    let mut points = [[0.0_f32; 2]; 5];
    for point_index in 0..5 {
        let x = distance[point_index * 2].mul_add(stride, anchor_x);
        let y = distance[point_index * 2 + 1].mul_add(stride, anchor_y);
        let [x, y] = normalized_letterbox_point(x, y, ctx);
        if !x.is_finite() || !y.is_finite() {
            return Err(ModelPackError::InvalidManifest("face_detection.output_names").into());
        }
        points[point_index] = [x, y];
    }
    Ok(FaceLandmarks { points })
}

fn normalized_letterbox_box(
    x1: f32,
    y1: f32,
    x2: f32,
    y2: f32,
    ctx: &DetectorPreprocessCtx,
) -> FaceBox {
    let [left, top] = normalized_letterbox_point(x1, y1, ctx);
    let [right, bottom] = normalized_letterbox_point(x2, y2, ctx);
    let mut bbox = FaceBox {
        left,
        top,
        width: right - left,
        height: bottom - top,
    };
    bbox.left = bbox.left.clamp(0.0, 1.0);
    bbox.top = bbox.top.clamp(0.0, 1.0);
    bbox.width = bbox.width.clamp(0.0, 1.0 - bbox.left);
    bbox.height = bbox.height.clamp(0.0, 1.0 - bbox.top);
    bbox
}

fn normalized_letterbox_point(x: f32, y: f32, ctx: &DetectorPreprocessCtx) -> [f32; 2] {
    [
        ((x - ctx.x_offset) / ctx.resized_width).clamp(0.0, 1.0),
        ((y - ctx.y_offset) / ctx.resized_height).clamp(0.0, 1.0),
    ]
}

struct YuNetOpenCvDetectorAdapter<'a> {
    config: &'a FaceDetectionModelConfig,
    preprocess: &'a ImagePreprocessConfig,
}

#[derive(Debug, Clone)]
struct YuNetOutputMap {
    stride: u32,
    cls_name: String,
    obj_name: String,
    bbox_name: String,
    kps_name: String,
}

impl FaceDetectorAdapter for YuNetOpenCvDetectorAdapter<'_> {
    fn output_names(&self) -> Vec<String> {
        yunet_output_maps(self.config)
            .into_iter()
            .flat_map(|map| [map.cls_name, map.obj_name, map.bbox_name, map.kps_name])
            .collect()
    }

    fn preprocess(
        &self,
        image: &DynamicImage,
    ) -> Result<(OnnxInputs, DetectorPreprocessCtx), FaceIndexError> {
        let (shape, values, ctx) = preprocess_yunet_image_pixels(image, self.preprocess)?;
        Ok((
            OnnxInputs::single(self.config.input_name.as_str(), shape, values),
            ctx,
        ))
    }

    fn postprocess(
        &self,
        outputs: &OnnxOutputs,
        ctx: &DetectorPreprocessCtx,
    ) -> Result<Vec<DetectedFace>, FaceIndexError> {
        let mut candidates = Vec::new();
        for map in yunet_output_maps(self.config) {
            let cls = outputs.values(&map.cls_name, "face_detection.output_names")?;
            let obj = outputs.values(&map.obj_name, "face_detection.output_names")?;
            let bbox = outputs.values(&map.bbox_name, "face_detection.output_names")?;
            let kps = outputs.values(&map.kps_name, "face_detection.output_names")?;
            let cols = usize::try_from(ctx.network_width / map.stride)
                .map_err(|_| FaceIndexError::RuntimeUnavailable)?
                .max(1);
            let rows = usize::try_from(ctx.network_height / map.stride)
                .map_err(|_| FaceIndexError::RuntimeUnavailable)?
                .max(1);
            let cell_count = rows * cols;
            if cls.len() < cell_count
                || obj.len() < cell_count
                || bbox.len() < cell_count * 4
                || kps.len() < cell_count * 10
            {
                return Err(ModelPackError::InvalidManifest("face_detection.output_names").into());
            }
            for row in 0..rows {
                for col in 0..cols {
                    let index = row * cols + col;
                    let score = cls[index]
                        .clamp(0.0, 1.0)
                        .mul_add(obj[index].clamp(0.0, 1.0), 0.0)
                        .sqrt();
                    if score < self.config.score_threshold {
                        continue;
                    }
                    let bbox_offset = index * 4;
                    let stride = map.stride as f32;
                    let cx = (col as f32 + bbox[bbox_offset]) * stride;
                    let cy = (row as f32 + bbox[bbox_offset + 1]) * stride;
                    let width = bbox[bbox_offset + 2].exp() * stride;
                    let height = bbox[bbox_offset + 3].exp() * stride;
                    let x1 = cx - width / 2.0;
                    let y1 = cy - height / 2.0;
                    let face_box = normalized_stretched_box(x1, y1, width, height, ctx);
                    let mut points = [[0.0_f32; 2]; 5];
                    let kps_offset = index * 10;
                    for point_index in 0..5 {
                        let x = (kps[kps_offset + point_index * 2] + col as f32) * stride;
                        let y = (kps[kps_offset + point_index * 2 + 1] + row as f32) * stride;
                        points[point_index] = normalized_stretched_point(x, y, ctx);
                    }
                    let face = DetectedFace {
                        bbox: face_box,
                        quality: score,
                        landmarks: Some(FaceLandmarks { points }),
                    };
                    if detection_passes_filters(&face, self.config) {
                        candidates.push(face);
                    }
                }
            }
        }
        candidates.sort_by(|left, right| right.quality.total_cmp(&left.quality));
        let mut selected: Vec<DetectedFace> = Vec::new();
        for candidate in candidates {
            if selected.iter().all(|existing| {
                face_iou(candidate.bbox, existing.bbox) <= self.config.nms_threshold
            }) {
                selected.push(candidate);
            }
            if selected.len() >= usize::try_from(self.config.max_faces).unwrap_or(usize::MAX) {
                break;
            }
        }
        Ok(selected)
    }
}

fn yunet_output_maps(config: &FaceDetectionModelConfig) -> Vec<YuNetOutputMap> {
    let strides = [8_u32, 16, 32];
    if config.output_names.len() >= 12 {
        return strides
            .into_iter()
            .enumerate()
            .map(|(index, stride)| YuNetOutputMap {
                stride,
                cls_name: config.output_names[index].clone(),
                obj_name: config.output_names[index + 3].clone(),
                bbox_name: config.output_names[index + 6].clone(),
                kps_name: config.output_names[index + 9].clone(),
            })
            .collect();
    }
    strides
        .into_iter()
        .map(|stride| YuNetOutputMap {
            stride,
            cls_name: format!("cls_{stride}"),
            obj_name: format!("obj_{stride}"),
            bbox_name: format!("bbox_{stride}"),
            kps_name: format!("kps_{stride}"),
        })
        .collect()
}

fn normalized_stretched_box(
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    ctx: &DetectorPreprocessCtx,
) -> FaceBox {
    let [left, top] = normalized_stretched_point(x, y, ctx);
    let [right, bottom] = normalized_stretched_point(x + width, y + height, ctx);
    let mut bbox = FaceBox {
        left,
        top,
        width: right - left,
        height: bottom - top,
    };
    bbox.left = bbox.left.clamp(0.0, 1.0);
    bbox.top = bbox.top.clamp(0.0, 1.0);
    bbox.width = bbox.width.clamp(0.0, 1.0 - bbox.left);
    bbox.height = bbox.height.clamp(0.0, 1.0 - bbox.top);
    bbox
}

fn normalized_stretched_point(x: f32, y: f32, ctx: &DetectorPreprocessCtx) -> [f32; 2] {
    [
        (x / ctx.resized_width).clamp(0.0, 1.0),
        (y / ctx.resized_height).clamp(0.0, 1.0),
    ]
}

struct RawEmbeddingAdapter<'a> {
    config: &'a FaceEmbeddingModelConfig,
}

impl FaceEmbedderAdapter for RawEmbeddingAdapter<'_> {
    fn output_names(&self) -> Vec<String> {
        vec![self.config.output_name.clone()]
    }

    fn chip_spec(&self) -> FaceChipSpec {
        let alignment = match self.config.alignment.as_str() {
            "bbox_crop" => FaceAlignmentMode::BboxCrop,
            _ => FaceAlignmentMode::ArcFace5Point,
        };
        let tensor_layout = match self.config.tensor_layout.as_str() {
            "nhwc" => FaceTensorLayout::Nhwc,
            _ => FaceTensorLayout::Nchw,
        };
        FaceChipSpec {
            width: self.config.width,
            height: self.config.height,
            alignment,
            color_order_bgr: self.config.color_order == "bgr",
            tensor_layout,
            mean: self.config.mean,
            std: self.config.std,
        }
    }

    fn preprocess_chip(&self, chip: &FaceChip) -> Result<OnnxInputs, FaceIndexError> {
        let (shape, values) = preprocess_chip_pixels(&chip.image, self.chip_spec())?;
        Ok(OnnxInputs::single(
            self.config.input_name.as_str(),
            shape,
            values,
        ))
    }

    fn postprocess(&self, outputs: &OnnxOutputs) -> Result<Vec<f32>, FaceIndexError> {
        let mut embedding = outputs
            .values(&self.config.output_name, "face_embedding.output_name")?
            .to_vec();
        if self.config.l2_normalize_output
            || matches!(
                self.config.adapter.as_str(),
                "arcface" | "sface_opencv_compat"
            )
        {
            l2_normalize(&mut embedding);
        }
        Ok(embedding)
    }
}

fn detector_adapter<'a>(
    config: &'a FaceDetectionModelConfig,
    manifest: &'a ModelPackManifest,
) -> Result<Box<dyn FaceDetectorAdapter + 'a>, FaceIndexError> {
    match config.adapter.as_str() {
        "decoded_boxes_v1" => Ok(Box::new(DecodedBoxesDetectorAdapter {
            config,
            preprocess: &manifest.image_preprocess,
        })),
        "scrfd" => Ok(Box::new(ScrfdDetectorAdapter {
            config,
            preprocess: &manifest.image_preprocess,
        })),
        "yunet_opencv_compat" => Ok(Box::new(YuNetOpenCvDetectorAdapter {
            config,
            preprocess: &manifest.image_preprocess,
        })),
        _ => Err(ModelPackError::InvalidManifest("face_detection.adapter").into()),
    }
}

fn embedder_adapter<'a>(
    config: &'a FaceEmbeddingModelConfig,
) -> Result<Box<dyn FaceEmbedderAdapter + 'a>, FaceIndexError> {
    match config.adapter.as_str() {
        "raw_embedding_v1" | "arcface" | "sface_opencv_compat" => {
            Ok(Box::new(RawEmbeddingAdapter { config }))
        }
        _ => Err(ModelPackError::InvalidManifest("face_embedding.adapter").into()),
    }
}

fn run_onnx(
    session: &mut Session,
    inputs: OnnxInputs,
    output_names: &[String],
) -> Result<OnnxOutputs, FaceIndexError> {
    let [input]: [OnnxInput; 1] = inputs
        .tensors
        .try_into()
        .map_err(|_| FaceIndexError::RuntimeUnavailable)?;
    let tensor = Tensor::from_array((input.shape, input.values))
        .map_err(|_| FaceIndexError::RuntimeUnavailable)?;
    let outputs = session
        .run(ort::inputs! {
            input.name.as_str() => tensor
        })
        .map_err(|_| FaceIndexError::RuntimeUnavailable)?;
    collect_outputs(&outputs, output_names)
}

fn collect_outputs(
    outputs: &SessionOutputs<'_>,
    output_names: &[String],
) -> Result<OnnxOutputs, FaceIndexError> {
    let mut tensors = HashMap::with_capacity(output_names.len());
    for name in output_names {
        let values = extract_output(outputs, name).map_err(FaceIndexError::Ml)?;
        tensors.insert(name.clone(), OnnxOutput { values });
    }
    Ok(OnnxOutputs { tensors })
}

fn preprocess_image_pixels(
    image: &DynamicImage,
    config: &ImagePreprocessConfig,
) -> Result<(Vec<usize>, Vec<f32>), FaceIndexError> {
    let resized = image.resize_exact(config.width, config.height, FilterType::Triangle);
    let rgb = resized.to_rgb8();
    preprocess_pixels(
        &rgb,
        config.color_order == "bgr",
        config.tensor_layout.as_str(),
        config.mean,
        config.std,
        "image_preprocess.tensor_layout",
    )
}

fn preprocess_letterboxed_image_pixels(
    image: &DynamicImage,
    config: &ImagePreprocessConfig,
) -> Result<(Vec<usize>, Vec<f32>, DetectorPreprocessCtx), FaceIndexError> {
    let (original_width, original_height) = image.dimensions();
    let ratio = (config.width as f32 / original_width.max(1) as f32)
        .min(config.height as f32 / original_height.max(1) as f32);
    let resized_width = (original_width as f32 * ratio).round().max(1.0) as u32;
    let resized_height = (original_height as f32 * ratio).round().max(1.0) as u32;
    let resized = image
        .resize_exact(resized_width, resized_height, FilterType::CatmullRom)
        .to_rgb8();
    let mut padded = RgbImage::new(config.width, config.height);
    let x_offset = config.width.saturating_sub(resized_width) as f32 / 2.0;
    let y_offset = config.height.saturating_sub(resized_height) as f32 / 2.0;
    image::imageops::overlay(&mut padded, &resized, x_offset as i64, y_offset as i64);
    let (shape, values) = preprocess_pixels(
        &padded,
        config.color_order == "bgr",
        config.tensor_layout.as_str(),
        config.mean,
        config.std,
        "image_preprocess.tensor_layout",
    )?;
    Ok((
        shape,
        values,
        DetectorPreprocessCtx {
            network_width: config.width,
            network_height: config.height,
            resized_width: resized_width as f32,
            resized_height: resized_height as f32,
            x_offset,
            y_offset,
        },
    ))
}

fn preprocess_yunet_image_pixels(
    image: &DynamicImage,
    config: &ImagePreprocessConfig,
) -> Result<(Vec<usize>, Vec<f32>, DetectorPreprocessCtx), FaceIndexError> {
    let pad_width = config.width.div_ceil(32) * 32;
    let pad_height = config.height.div_ceil(32) * 32;
    let resized = image
        .resize_exact(config.width, config.height, FilterType::Triangle)
        .to_rgb8();
    let mut padded = RgbImage::new(pad_width, pad_height);
    image::imageops::overlay(&mut padded, &resized, 0, 0);
    let (shape, values) = preprocess_pixels(
        &padded,
        config.color_order == "bgr",
        config.tensor_layout.as_str(),
        config.mean,
        config.std,
        "image_preprocess.tensor_layout",
    )?;
    Ok((
        shape,
        values,
        DetectorPreprocessCtx {
            network_width: pad_width,
            network_height: pad_height,
            resized_width: config.width as f32,
            resized_height: config.height as f32,
            x_offset: 0.0,
            y_offset: 0.0,
        },
    ))
}

fn make_face_chip(
    image: &DynamicImage,
    face: DetectedFace,
    spec: FaceChipSpec,
) -> Result<FaceChip, FaceIndexError> {
    let chip_image = match spec.alignment {
        FaceAlignmentMode::ArcFace5Point => {
            let landmarks = face.landmarks.ok_or(ModelPackError::InvalidManifest(
                "face_detection.landmarks_output",
            ))?;
            let rgb = image.to_rgb8();
            align_face_5point(&rgb, landmarks, spec.width, spec.height)?
        }
        FaceAlignmentMode::BboxCrop => crop_face_chip(image, face.bbox, spec.width, spec.height)?,
    };
    Ok(FaceChip {
        image: chip_image,
        source_bbox: face.bbox,
    })
}

fn crop_face_chip(
    image: &DynamicImage,
    bbox: FaceBox,
    width: u32,
    height: u32,
) -> Result<RgbImage, FaceIndexError> {
    let (image_width, image_height) = image.dimensions();
    let left = (bbox.left * image_width as f32)
        .floor()
        .clamp(0.0, image_width.saturating_sub(1) as f32) as u32;
    let top = (bbox.top * image_height as f32)
        .floor()
        .clamp(0.0, image_height.saturating_sub(1) as f32) as u32;
    let crop_width = (bbox.width * image_width as f32).ceil().max(1.0) as u32;
    let crop_height = (bbox.height * image_height as f32).ceil().max(1.0) as u32;
    let crop_width = crop_width.min(image_width - left);
    let crop_height = crop_height.min(image_height - top);
    let crop = image
        .crop_imm(left, top, crop_width, crop_height)
        .resize_exact(width, height, FilterType::Triangle);
    Ok(crop.to_rgb8())
}

fn encode_face_chip(image: &RgbImage) -> Result<FaceChipImage, FaceIndexError> {
    let mut output = Cursor::new(Vec::new());
    DynamicImage::ImageRgb8(image.clone())
        .write_to(&mut output, ImageFormat::WebP)
        .map_err(|_| FaceIndexError::Ml(MlError::InvalidImage))?;
    Ok(FaceChipImage {
        bytes: output.into_inner(),
        width: image.width(),
        height: image.height(),
        format: "webp",
    })
}

fn preprocess_chip_pixels(
    rgb: &RgbImage,
    spec: FaceChipSpec,
) -> Result<(Vec<usize>, Vec<f32>), FaceIndexError> {
    let layout = match spec.tensor_layout {
        FaceTensorLayout::Nchw => "nchw",
        FaceTensorLayout::Nhwc => "nhwc",
    };
    preprocess_pixels(
        rgb,
        spec.color_order_bgr,
        layout,
        spec.mean,
        spec.std,
        "face_embedding.tensor_layout",
    )
}

fn preprocess_pixels(
    rgb: &RgbImage,
    bgr: bool,
    tensor_layout: &str,
    mean: [f32; 3],
    std: [f32; 3],
    invalid_layout_field: &'static str,
) -> Result<(Vec<usize>, Vec<f32>), FaceIndexError> {
    let width = usize::try_from(rgb.width()).map_err(|_| FaceIndexError::RuntimeUnavailable)?;
    let height = usize::try_from(rgb.height()).map_err(|_| FaceIndexError::RuntimeUnavailable)?;
    let mut values = vec![0.0_f32; width * height * 3];
    match tensor_layout {
        "nchw" => {
            for (x, y, pixel) in rgb.enumerate_pixels() {
                let x = usize::try_from(x).map_err(|_| FaceIndexError::RuntimeUnavailable)?;
                let y = usize::try_from(y).map_err(|_| FaceIndexError::RuntimeUnavailable)?;
                let channels = ordered_channels(pixel.0, bgr);
                for (channel, raw) in channels.iter().enumerate() {
                    values[channel * width * height + y * width + x] =
                        normalize_channel(*raw, mean, std, channel);
                }
            }
            Ok((vec![1, 3, height, width], values))
        }
        "nhwc" => {
            for (x, y, pixel) in rgb.enumerate_pixels() {
                let x = usize::try_from(x).map_err(|_| FaceIndexError::RuntimeUnavailable)?;
                let y = usize::try_from(y).map_err(|_| FaceIndexError::RuntimeUnavailable)?;
                let base = (y * width + x) * 3;
                let channels = ordered_channels(pixel.0, bgr);
                for (channel, raw) in channels.iter().enumerate() {
                    values[base + channel] = normalize_channel(*raw, mean, std, channel);
                }
            }
            Ok((vec![1, height, width, 3], values))
        }
        _ => Err(ModelPackError::InvalidManifest(invalid_layout_field).into()),
    }
}

fn l2_normalize(values: &mut [f32]) {
    let norm = values.iter().map(|value| value * value).sum::<f32>().sqrt();
    if norm > 1e-12 {
        let inv_norm = 1.0 / norm;
        for value in values {
            *value *= inv_norm;
        }
    }
}

fn flatten_detected_faces_for_self_test(faces: &[DetectedFace]) -> Vec<f32> {
    let mut values = Vec::new();
    for face in faces {
        values.extend_from_slice(&[
            face.quality,
            face.bbox.left,
            face.bbox.top,
            face.bbox.width,
            face.bbox.height,
        ]);
        if let Some(landmarks) = face.landmarks {
            for point in landmarks.points {
                values.extend_from_slice(&point);
            }
        }
    }
    values
}

fn flatten_indexed_faces_for_self_test(faces: &[IndexedFace]) -> Vec<f32> {
    let mut values = Vec::new();
    for face in faces {
        values.extend_from_slice(&[
            face.quality.unwrap_or(0.0),
            face.bbox.left,
            face.bbox.top,
            face.bbox.width,
            face.bbox.height,
        ]);
        values.extend_from_slice(&face.embedding);
    }
    values
}

fn decode_face_image(
    bytes: &[u8],
    media_type: &str,
    heif_convert_path: &std::path::Path,
) -> Result<DynamicImage, FaceIndexError> {
    let (bytes, media_type) =
        normalize_still_image_for_image_crate(bytes, media_type, heif_convert_path)
            .map_err(face_image_error)?;
    decode_still_image(&bytes, media_type).map_err(face_image_error)
}

fn face_image_error(error: MediaToolError) -> FaceIndexError {
    match error {
        MediaToolError::UnsupportedMediaType => FaceIndexError::UnsupportedMediaType,
        MediaToolError::Image(_) => FaceIndexError::Ml(MlError::InvalidImage),
        other => FaceIndexError::ImageConversion(other),
    }
}

fn select_faces(
    boxes: &[f32],
    scores: &[f32],
    landmarks: Option<&[f32]>,
    config: &FaceDetectionModelConfig,
    input_width: u32,
    input_height: u32,
) -> Result<Vec<DetectedFace>, FaceIndexError> {
    if !boxes.len().is_multiple_of(4) {
        return Err(ModelPackError::InvalidManifest("face_detection.boxes_output").into());
    }
    let face_count = boxes.len() / 4;
    if let Some(landmarks) = landmarks
        && landmarks.len() != face_count * 10
    {
        return Err(ModelPackError::InvalidManifest("face_detection.landmarks_output").into());
    }
    let mut candidates = boxes
        .chunks_exact(4)
        .enumerate()
        .filter_map(|(index, raw)| {
            let score = detection_score(scores, index, face_count)?;
            (score >= config.score_threshold).then(|| {
                let bbox = face_box_from_output(raw, config, input_width, input_height);
                let landmarks = landmarks.and_then(|values| {
                    face_landmarks_from_output(
                        &values[index * 10..index * 10 + 10],
                        config,
                        input_width,
                        input_height,
                    )
                });
                DetectedFace {
                    bbox,
                    quality: score,
                    landmarks,
                }
            })
        })
        .filter(|face| detection_passes_filters(face, config))
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| right.quality.total_cmp(&left.quality));
    let mut selected: Vec<DetectedFace> = Vec::new();
    for candidate in candidates {
        if selected
            .iter()
            .all(|existing| face_iou(candidate.bbox, existing.bbox) <= config.nms_threshold)
        {
            selected.push(candidate);
        }
        if selected.len() >= usize::try_from(config.max_faces).unwrap_or(usize::MAX) {
            break;
        }
    }
    Ok(selected)
}

fn detection_passes_filters(face: &DetectedFace, config: &FaceDetectionModelConfig) -> bool {
    face.bbox.is_valid()
        && face.quality.is_finite()
        && face.bbox.width >= config.min_face_size_ratio
        && face.bbox.height >= config.min_face_size_ratio
}

fn detection_score(scores: &[f32], index: usize, face_count: usize) -> Option<f32> {
    if scores.len() == face_count {
        scores.get(index).copied()
    } else if scores.len() == face_count * 2 {
        scores.get(index * 2 + 1).copied()
    } else {
        scores.get(index).copied()
    }
}

fn face_box_from_output(
    raw: &[f32],
    config: &FaceDetectionModelConfig,
    input_width: u32,
    input_height: u32,
) -> FaceBox {
    let (left, top, width, height) = match config.box_format.as_str() {
        "xyxy" => (raw[0], raw[1], raw[2] - raw[0], raw[3] - raw[1]),
        _ => (raw[0], raw[1], raw[2], raw[3]),
    };
    let (scale_x, scale_y) = if config.box_coordinate_space == "pixel" {
        (input_width.max(1) as f32, input_height.max(1) as f32)
    } else {
        (1.0, 1.0)
    };
    let mut bbox = FaceBox {
        left: left / scale_x,
        top: top / scale_y,
        width: width / scale_x,
        height: height / scale_y,
    };
    bbox.left = bbox.left.clamp(0.0, 1.0);
    bbox.top = bbox.top.clamp(0.0, 1.0);
    bbox.width = bbox.width.clamp(0.0, 1.0 - bbox.left);
    bbox.height = bbox.height.clamp(0.0, 1.0 - bbox.top);
    bbox
}

fn face_landmarks_from_output(
    raw: &[f32],
    config: &FaceDetectionModelConfig,
    input_width: u32,
    input_height: u32,
) -> Option<FaceLandmarks> {
    let (scale_x, scale_y) = if config.box_coordinate_space == "pixel" {
        (input_width.max(1) as f32, input_height.max(1) as f32)
    } else {
        (1.0, 1.0)
    };
    let mut points = [[0.0_f32; 2]; 5];
    for index in 0..5 {
        let x = raw[index * 2] / scale_x;
        let y = raw[index * 2 + 1] / scale_y;
        if !x.is_finite() || !y.is_finite() {
            return None;
        }
        points[index] = [x.clamp(0.0, 1.0), y.clamp(0.0, 1.0)];
    }
    Some(FaceLandmarks { points })
}

fn align_face_5point(
    image: &RgbImage,
    landmarks: FaceLandmarks,
    width: u32,
    height: u32,
) -> Result<RgbImage, FaceIndexError> {
    let image_width = image.width() as f32;
    let image_height = image.height() as f32;
    let src = landmarks
        .points
        .map(|[x, y]| [x * image_width, y * image_height]);
    let dst = canonical_arcface_landmarks(width, height);
    let transform = similarity_transform(src, dst).ok_or(ModelPackError::InvalidManifest(
        "face_detection.landmarks_output",
    ))?;
    let inverse = transform.inverse().ok_or(ModelPackError::InvalidManifest(
        "face_detection.landmarks_output",
    ))?;
    let mut output = RgbImage::new(width, height);
    for y in 0..height {
        for x in 0..width {
            let [src_x, src_y] = inverse.apply(x as f32, y as f32);
            output.put_pixel(x, y, sample_bilinear(image, src_x, src_y));
        }
    }
    Ok(output)
}

fn canonical_arcface_landmarks(width: u32, height: u32) -> [[f32; 2]; 5] {
    let scale_x = width as f32 / 112.0;
    let scale_y = height as f32 / 112.0;
    [
        [38.2946 * scale_x, 51.6963 * scale_y],
        [73.5318 * scale_x, 51.5014 * scale_y],
        [56.0252 * scale_x, 71.7366 * scale_y],
        [41.5493 * scale_x, 92.3655 * scale_y],
        [70.7299 * scale_x, 92.2041 * scale_y],
    ]
}

#[derive(Debug, Clone, Copy)]
struct SimilarityTransform {
    a: f32,
    b: f32,
    tx: f32,
    ty: f32,
}

impl SimilarityTransform {
    fn apply(self, x: f32, y: f32) -> [f32; 2] {
        [
            self.a * x - self.b * y + self.tx,
            self.b * x + self.a * y + self.ty,
        ]
    }

    fn inverse(self) -> Option<Self> {
        let denom = self.a * self.a + self.b * self.b;
        if denom <= f32::EPSILON {
            return None;
        }
        Some(Self {
            a: self.a / denom,
            b: -self.b / denom,
            tx: (-self.a * self.tx - self.b * self.ty) / denom,
            ty: (self.b * self.tx - self.a * self.ty) / denom,
        })
    }
}

fn similarity_transform(src: [[f32; 2]; 5], dst: [[f32; 2]; 5]) -> Option<SimilarityTransform> {
    let src_center = point_mean(src);
    let dst_center = point_mean(dst);
    let mut denom = 0.0;
    let mut a_num = 0.0;
    let mut b_num = 0.0;
    for (src, dst) in src.into_iter().zip(dst) {
        let sx = src[0] - src_center[0];
        let sy = src[1] - src_center[1];
        let dx = dst[0] - dst_center[0];
        let dy = dst[1] - dst_center[1];
        denom += sx * sx + sy * sy;
        a_num += dx * sx + dy * sy;
        b_num += dy * sx - dx * sy;
    }
    if denom <= f32::EPSILON {
        return None;
    }
    let a = a_num / denom;
    let b = b_num / denom;
    Some(SimilarityTransform {
        a,
        b,
        tx: dst_center[0] - a * src_center[0] + b * src_center[1],
        ty: dst_center[1] - b * src_center[0] - a * src_center[1],
    })
}

fn point_mean(points: [[f32; 2]; 5]) -> [f32; 2] {
    let mut x = 0.0;
    let mut y = 0.0;
    for point in points {
        x += point[0];
        y += point[1];
    }
    [x / 5.0, y / 5.0]
}

fn sample_bilinear(image: &RgbImage, x: f32, y: f32) -> Rgb<u8> {
    if x < 0.0 || y < 0.0 || x > (image.width() - 1) as f32 || y > (image.height() - 1) as f32 {
        return Rgb([0, 0, 0]);
    }
    let x0 = x.floor() as u32;
    let y0 = y.floor() as u32;
    let x1 = (x0 + 1).min(image.width() - 1);
    let y1 = (y0 + 1).min(image.height() - 1);
    let wx = x - x0 as f32;
    let wy = y - y0 as f32;
    let p00 = image.get_pixel(x0, y0).0;
    let p10 = image.get_pixel(x1, y0).0;
    let p01 = image.get_pixel(x0, y1).0;
    let p11 = image.get_pixel(x1, y1).0;
    let mut out = [0_u8; 3];
    for channel in 0..3 {
        let top = p00[channel] as f32 * (1.0 - wx) + p10[channel] as f32 * wx;
        let bottom = p01[channel] as f32 * (1.0 - wx) + p11[channel] as f32 * wx;
        out[channel] = (top * (1.0 - wy) + bottom * wy).round().clamp(0.0, 255.0) as u8;
    }
    Rgb(out)
}

fn face_iou(left: FaceBox, right: FaceBox) -> f32 {
    let x1 = left.left.max(right.left);
    let y1 = left.top.max(right.top);
    let x2 = (left.left + left.width).min(right.left + right.width);
    let y2 = (left.top + left.height).min(right.top + right.height);
    let intersection = (x2 - x1).max(0.0) * (y2 - y1).max(0.0);
    let left_area = left.width * left.height;
    let right_area = right.width * right.height;
    intersection / (left_area + right_area - intersection).max(f32::EPSILON)
}

fn cosine_similarity(left: &[f32], right: &[f32]) -> Option<f32> {
    if left.len() != right.len() || left.is_empty() {
        return None;
    }
    let mut dot = 0.0;
    let mut left_norm = 0.0;
    let mut right_norm = 0.0;
    for (left, right) in left.iter().zip(right) {
        dot += left * right;
        left_norm += left * left;
        right_norm += right * right;
    }
    if left_norm <= f32::EPSILON || right_norm <= f32::EPSILON {
        return None;
    }
    Some(dot / left_norm.sqrt() / right_norm.sqrt())
}

fn face_uuid_field(value: &Value, field: &str) -> Result<Uuid, FaceIndexError> {
    value
        .get(field)
        .and_then(Value::as_str)
        .and_then(|raw| Uuid::parse_str(raw).ok())
        .ok_or(FaceIndexError::InvalidJobPayload)
}

fn optional_face_uuid_field(value: &Value, field: &str) -> Result<Option<Uuid>, FaceIndexError> {
    value
        .get(field)
        .and_then(Value::as_str)
        .map(Uuid::parse_str)
        .transpose()
        .map_err(|_| FaceIndexError::InvalidJobPayload)
}

fn is_supported_face_media_type(media_type: &str) -> bool {
    matches!(
        media_type,
        "image/jpeg" | "image/png" | "image/gif" | "image/webp" | "image/heic" | "image/heif"
    )
}
