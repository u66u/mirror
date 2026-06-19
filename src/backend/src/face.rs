//! Face indexing pipeline.
//!
//! Runtime code detects faces and embeds aligned crops. This module owns Mirror
//! side effects: load originals, choose active face model packs, persist
//! face rows/embeddings, and keep owner-local people assignments.

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use image::{DynamicImage, GenericImageView, ImageFormat, Rgb, RgbImage, imageops::FilterType};
use ort::{session::Session, value::Tensor};
use pgvector::Vector;
use serde_json::Value;
use sqlx::{PgPool, Row, types::Json};
use thiserror::Error;
use uuid::Uuid;

use crate::{
    config::MlDevicePreference,
    jobs::{self, JobError, JobKind, JobSpec, LeasedJob},
    ml::MlError,
    models::{
        self, FaceDetectionModelConfig, FaceEmbeddingModelConfig, ModelPackError, ModelPackKind,
        ModelPackManifest, validate_embedding_output,
    },
    onnx_embedder::{
        extract_output, model_pack_file_path, normalize_channel, open_session, ordered_channels,
        preprocess_image_for_onnx,
    },
    storage::{ObjectStorage, StorageError, StorageKey, StorageKeyError},
};

const MAX_FACE_IMAGE_BYTES: usize = 25 * 1024 * 1024;

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

/// One detected and embedded face ready for DB storage.
#[derive(Debug, Clone, PartialEq)]
pub struct IndexedFace {
    /// Normalized bounding box.
    pub bbox: FaceBox,
    /// Detector confidence.
    pub quality: Option<f32>,
    /// Face identity embedding.
    pub embedding: Vec<f32>,
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
}

/// ONNX-backed face detection/embedding runtime.
pub struct OnnxFaceRuntime {
    storage_root: PathBuf,
    device: MlDevicePreference,
    detection_sessions: Mutex<HashMap<Uuid, Arc<Mutex<Session>>>>,
    embedding_sessions: Mutex<HashMap<Uuid, Arc<Mutex<Session>>>>,
}

impl OnnxFaceRuntime {
    /// Creates a lazy ONNX face runtime.
    #[must_use]
    pub fn new(storage_root: PathBuf, device: MlDevicePreference) -> Self {
        Self {
            storage_root,
            device,
            detection_sessions: Mutex::new(HashMap::new()),
            embedding_sessions: Mutex::new(HashMap::new()),
        }
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
        let image = decode_face_image(request.bytes, request.media_type)?;

        let (shape, values) = preprocess_image_for_onnx(
            request.bytes,
            request.media_type,
            request.detection_manifest,
        )
        .map_err(FaceIndexError::Ml)?;
        let input =
            Tensor::from_array((shape, values)).map_err(|_| FaceIndexError::RuntimeUnavailable)?;
        let detection_session =
            self.detection_session(request.detection_model_pack_id, request.detection_manifest)?;
        let (boxes, scores, landmarks) = {
            let mut session = detection_session
                .lock()
                .map_err(|_| FaceIndexError::RuntimeUnavailable)?;
            let outputs = session
                .run(ort::inputs! {
                    detection_config.input_name.as_str() => input
                })
                .map_err(|_| FaceIndexError::RuntimeUnavailable)?;
            (
                extract_output(&outputs, &detection_config.boxes_output_name)
                    .map_err(FaceIndexError::Ml)?,
                extract_output(&outputs, &detection_config.scores_output_name)
                    .map_err(FaceIndexError::Ml)?,
                detection_config
                    .landmarks_output_name
                    .as_deref()
                    .map(|name| extract_output(&outputs, name).map_err(FaceIndexError::Ml))
                    .transpose()?,
            )
        };

        let faces = select_faces(
            &boxes,
            &scores,
            landmarks.as_deref(),
            detection_config,
            request.detection_manifest.image_preprocess.width,
            request.detection_manifest.image_preprocess.height,
        )?;
        let embedding_session =
            self.embedding_session(request.embedding_model_pack_id, request.embedding_manifest)?;
        let mut indexed = Vec::with_capacity(faces.len());
        for face in faces {
            let (shape, values) = preprocess_face_input(&image, face, embedding_config)?;
            let input = Tensor::from_array((shape, values))
                .map_err(|_| FaceIndexError::RuntimeUnavailable)?;
            let mut session = embedding_session
                .lock()
                .map_err(|_| FaceIndexError::RuntimeUnavailable)?;
            let outputs = session
                .run(ort::inputs! {
                    embedding_config.input_name.as_str() => input
                })
                .map_err(|_| FaceIndexError::RuntimeUnavailable)?;
            let embedding = extract_output(&outputs, &embedding_config.output_name)
                .map_err(FaceIndexError::Ml)?;
            indexed.push(IndexedFace {
                bbox: face.bbox,
                quality: Some(face.quality),
                embedding,
            });
        }

        Ok(indexed)
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

    persist_faces(pool, &asset, detection_pack.id, &embedding_pack, faces).await?;
    if let Some(reindex_run_id) = payload.reindex_run_id {
        models::record_reindex_asset_result(pool, reindex_run_id, asset.asset_id, true, None)
            .await?;
    }
    Ok(())
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
        sqlx::query(
            r#"
            INSERT INTO face_occurrences (
                id, asset_id, owner_id, detection_model_pack_id,
                bbox_left, bbox_top, bbox_width, bbox_height, quality
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
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

fn decode_face_image(bytes: &[u8], media_type: &str) -> Result<DynamicImage, FaceIndexError> {
    let format = match media_type {
        "image/jpeg" => ImageFormat::Jpeg,
        "image/png" => ImageFormat::Png,
        _ => return Err(FaceIndexError::UnsupportedMediaType),
    };
    image::load_from_memory_with_format(bytes, format)
        .map_err(|_| FaceIndexError::Ml(MlError::InvalidImage))
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
        .filter(|face| face.bbox.is_valid() && face.quality.is_finite())
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

fn preprocess_face_input(
    image: &DynamicImage,
    face: DetectedFace,
    config: &FaceEmbeddingModelConfig,
) -> Result<(Vec<usize>, Vec<f32>), FaceIndexError> {
    if config.alignment == "five_point" {
        if let Some(landmarks) = face.landmarks {
            return preprocess_aligned_face(image, landmarks, config);
        }
        return Err(ModelPackError::InvalidManifest("face_detection.landmarks_output").into());
    }
    preprocess_face_crop(image, face.bbox, config)
}

fn preprocess_face_crop(
    image: &DynamicImage,
    bbox: FaceBox,
    config: &FaceEmbeddingModelConfig,
) -> Result<(Vec<usize>, Vec<f32>), FaceIndexError> {
    let (image_width, image_height) = image.dimensions();
    let left = (bbox.left * image_width as f32)
        .floor()
        .clamp(0.0, image_width.saturating_sub(1) as f32) as u32;
    let top = (bbox.top * image_height as f32)
        .floor()
        .clamp(0.0, image_height.saturating_sub(1) as f32) as u32;
    let width = (bbox.width * image_width as f32).ceil().max(1.0) as u32;
    let height = (bbox.height * image_height as f32).ceil().max(1.0) as u32;
    let width = width.min(image_width - left);
    let height = height.min(image_height - top);
    let crop = image.crop_imm(left, top, width, height).resize_exact(
        config.width,
        config.height,
        FilterType::Triangle,
    );
    preprocess_face_pixels(&crop.to_rgb8(), config)
}

fn preprocess_aligned_face(
    image: &DynamicImage,
    landmarks: FaceLandmarks,
    config: &FaceEmbeddingModelConfig,
) -> Result<(Vec<usize>, Vec<f32>), FaceIndexError> {
    let rgb = image.to_rgb8();
    let aligned = align_face_5point(&rgb, landmarks, config.width, config.height)?;
    preprocess_face_pixels(&aligned, config)
}

fn preprocess_face_pixels(
    rgb: &RgbImage,
    config: &FaceEmbeddingModelConfig,
) -> Result<(Vec<usize>, Vec<f32>), FaceIndexError> {
    let width = usize::try_from(rgb.width()).map_err(|_| FaceIndexError::RuntimeUnavailable)?;
    let height = usize::try_from(rgb.height()).map_err(|_| FaceIndexError::RuntimeUnavailable)?;
    let mut values = vec![0.0_f32; width * height * 3];
    let bgr = config.color_order == "bgr";
    match config.tensor_layout.as_str() {
        "nchw" => {
            for (x, y, pixel) in rgb.enumerate_pixels() {
                let x = usize::try_from(x).map_err(|_| FaceIndexError::RuntimeUnavailable)?;
                let y = usize::try_from(y).map_err(|_| FaceIndexError::RuntimeUnavailable)?;
                let channels = ordered_channels(pixel.0, bgr);
                for (channel, raw) in channels.iter().enumerate() {
                    values[channel * width * height + y * width + x] =
                        normalize_channel(*raw, config.mean, config.std, channel);
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
                    values[base + channel] =
                        normalize_channel(*raw, config.mean, config.std, channel);
                }
            }
            Ok((vec![1, height, width, 3], values))
        }
        _ => Err(ModelPackError::InvalidManifest("face_embedding.tensor_layout").into()),
    }
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
    matches!(media_type, "image/jpeg" | "image/png")
}
