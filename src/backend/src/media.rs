//! Media metadata and derivative job handlers.
//!
//! C005: media bytes are attacker-controlled. Still-image decoding runs in the
//! worker path only, never from request handlers. Video handling will use a
//! separate timeout-bounded external command wrapper.

use std::{collections::BTreeMap, io::Cursor, path::PathBuf};
use thiserror::Error;

use image::{GenericImageView, ImageFormat, ImageReader, Limits};
use nom_exif::{Exif, ExifTag, MediaParser, MediaSource};
use serde_json::{Value, json};
use sqlx::{PgPool, types::Json};
use uuid::Uuid;

use crate::{
    jobs::{JobKind, LeasedJob},
    storage::{ObjectStorage, StorageKey},
    video::{VideoProcessor, VideoToolError},
};

const METADATA_VERSION: &str = "media-metadata-v2";
const IMAGE_GENERATOR_VERSION: &str = "media-v1-image-webp-1";
const VIDEO_GENERATOR_VERSION: &str = "media-v1-video-poster-webp-1";
const MAX_STILL_SOURCE_BYTES: i64 = 512 * 1024 * 1024;
const MAX_VIDEO_SOURCE_BYTES: i64 = 64 * 1024 * 1024 * 1024;
const MAX_IMAGE_DIMENSION: u32 = 32_768;
const MAX_IMAGE_ALLOC_BYTES: u64 = 256 * 1024 * 1024;
const MAX_OWNER_METADATA_ENTRIES: usize = 256;
const MAX_OWNER_METADATA_VALUE_CHARS: usize = 1_024;

/// Supported derivative kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DerivativeKind {
    /// Small grid thumbnail.
    Thumbnail,
    /// Larger preview for detail views.
    Preview,
}

impl DerivativeKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Thumbnail => "thumbnail",
            Self::Preview => "preview",
        }
    }

    fn max_edge(self) -> u32 {
        match self {
            Self::Thumbnail => 512,
            Self::Preview => 2048,
        }
    }
}

/// Basic image dimensions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageInfo {
    /// Pixel width.
    pub width: u32,
    /// Pixel height.
    pub height: u32,
}

/// Generated derivative bytes and dimensions.
#[derive(Debug, PartialEq, Eq)]
pub struct GeneratedDerivative {
    /// Encoded derivative bytes.
    pub bytes: Vec<u8>,
    /// Pixel width.
    pub width: u32,
    /// Pixel height.
    pub height: u32,
    /// Stable storage/database format.
    pub format: &'static str,
}

/// Image processor boundary.
pub trait ImageProcessor: Clone + Send + Sync + 'static {
    /// Inspects dimensions without mutating input bytes.
    fn inspect(&self, bytes: &[u8], media_type: &str) -> Result<ImageInfo, MediaToolError>;
    /// Generates a stripped derivative.
    fn generate(
        &self,
        bytes: &[u8],
        media_type: &str,
        kind: DerivativeKind,
    ) -> Result<GeneratedDerivative, MediaToolError>;
}

/// Pure-Rust still-image processor for v1 thumbnails/previews.
#[derive(Debug, Clone, Copy, Default)]
pub struct RustImageProcessor;

impl ImageProcessor for RustImageProcessor {
    fn inspect(&self, bytes: &[u8], media_type: &str) -> Result<ImageInfo, MediaToolError> {
        let image = decode_image(bytes, media_type)?;
        let (width, height) = image.dimensions();
        Ok(ImageInfo { width, height })
    }

    fn generate(
        &self,
        bytes: &[u8],
        media_type: &str,
        kind: DerivativeKind,
    ) -> Result<GeneratedDerivative, MediaToolError> {
        let image = decode_image(bytes, media_type)?;
        let (source_width, source_height) = image.dimensions();
        let max_edge = kind.max_edge().min(source_width.max(source_height));
        let resized = image.thumbnail(max_edge, max_edge);
        let (width, height) = resized.dimensions();
        let mut output = Cursor::new(Vec::new());
        resized.write_to(&mut output, ImageFormat::WebP)?;

        Ok(GeneratedDerivative {
            bytes: output.into_inner(),
            width,
            height,
            format: "webp",
        })
    }
}

/// Media job failure.
#[derive(Debug, Error)]
pub enum MediaError {
    /// Job payload did not contain a valid internal asset UUID.
    #[error("invalid media job payload")]
    InvalidJobPayload,
    /// Job kind belongs to another worker.
    #[error("unsupported media job kind")]
    UnsupportedJobKind,
    /// Asset row was not found.
    #[error("asset not found")]
    AssetNotFound,
    /// Media type is not handled by the image pipeline.
    #[error("unsupported media type")]
    UnsupportedMediaType,
    /// Source exceeds the configured in-process or staged-media bound.
    #[error("media source exceeds processing limit")]
    SourceTooLarge,
    /// Stored object length no longer matches immutable database metadata.
    #[error("original object size mismatch")]
    OriginalSizeMismatch,
    /// Image processor failed.
    #[error("media tool failed: {0}")]
    Tool(#[from] MediaToolError),
    /// External video processor failed.
    #[error("video tool failed: {0}")]
    VideoTool(#[from] VideoToolError),
    /// Blocking media task panicked or was cancelled.
    #[error("media processing task failed")]
    ProcessingTaskFailed,
    /// Private media staging directory could not be created.
    #[error("media temporary storage failed")]
    TemporaryStorage(std::io::Error),
    /// Storage failed.
    #[error("media storage error: {0}")]
    Storage(#[from] crate::storage::StorageError),
    /// Database failed.
    #[error("media database error: {0}")]
    Database(#[from] sqlx::Error),
}

/// External media tool failure.
#[derive(Debug, Error)]
pub enum MediaToolError {
    /// Media type is not image input supported by this processor.
    #[error("unsupported media type")]
    UnsupportedMediaType,
    /// Image decoder/encoder failed.
    #[error("image processing failed")]
    Image(#[from] image::ImageError),
}

/// Runs a leased media job without changing queue state.
///
/// The worker binary owns queue completion/failure so tests can exercise the
/// handler independently from leasing.
pub async fn run_media_job<I, V>(
    pool: &PgPool,
    storage: &ObjectStorage,
    image_processor: &I,
    video_processor: &V,
    job: &LeasedJob,
) -> Result<(), MediaError>
where
    I: ImageProcessor,
    V: VideoProcessor,
{
    let asset_id = asset_id_from_payload(&job.payload)?;
    match job.kind {
        JobKind::ExtractMetadata => {
            extract_metadata(pool, storage, image_processor, video_processor, asset_id).await
        }
        JobKind::GenerateDerivatives => {
            generate_derivatives(pool, storage, image_processor, video_processor, asset_id).await
        }
        JobKind::EmbedAsset => Err(MediaError::UnsupportedJobKind),
    }
}

/// Extracts basic metadata for an asset original.
pub async fn extract_metadata<I, V>(
    pool: &PgPool,
    storage: &ObjectStorage,
    image_processor: &I,
    video_processor: &V,
    asset_id: Uuid,
) -> Result<(), MediaError>
where
    I: ImageProcessor,
    V: VideoProcessor,
{
    let original = load_asset_original(pool, asset_id).await?;
    let media_kind = classify_media_type(&original.media_type)?;
    let (width, height, raw) = match media_kind {
        MediaKind::Image => extract_image_metadata(storage, image_processor, &original).await?,
        MediaKind::Video => extract_video_metadata(storage, video_processor, &original).await?,
    };

    sqlx::query!(
        r#"
        INSERT INTO asset_metadata (asset_id, width, height, raw)
        VALUES ($1, $2, $3, $4)
        ON CONFLICT (asset_id)
        DO UPDATE SET
            width = EXCLUDED.width,
            height = EXCLUDED.height,
            raw = EXCLUDED.raw,
            extracted_at = now(),
            updated_at = now()
        "#,
        asset_id,
        i32::try_from(width).map_err(|_| MediaError::UnsupportedMediaType)?,
        i32::try_from(height).map_err(|_| MediaError::UnsupportedMediaType)?,
        Json(raw) as _
    )
    .execute(pool)
    .await?;

    Ok(())
}

async fn extract_image_metadata(
    storage: &ObjectStorage,
    processor: &impl ImageProcessor,
    original: &AssetOriginal,
) -> Result<(u32, u32, Value), MediaError> {
    ensure_bounded_still_source(original)?;
    let bytes = storage
        .read(&original.storage_key()?)
        .await
        .map_err(MediaError::Storage)?;
    ensure_loaded_size(original, bytes.len())?;
    let processor = processor.clone();
    let media_type = original.media_type.clone();
    let (info, owner_metadata) = tokio::task::spawn_blocking(move || {
        let info = processor.inspect(&bytes, &media_type)?;
        Ok::<_, MediaToolError>((info, extract_owner_metadata(bytes)))
    })
    .await
    .map_err(|_| MediaError::ProcessingTaskFailed)??;
    let raw = json!({
        "width": info.width,
        "height": info.height,
        "extractor": METADATA_VERSION,
        "media_kind": "image",
        "owner_metadata": owner_metadata,
    });
    Ok((info.width, info.height, raw))
}

async fn extract_video_metadata(
    storage: &ObjectStorage,
    processor: &impl VideoProcessor,
    original: &AssetOriginal,
) -> Result<(u32, u32, Value), MediaError> {
    let (temp_dir, input) = stage_video_original(storage, original).await?;
    let processor = processor.clone();
    let info = tokio::task::spawn_blocking(move || {
        let _temp_dir = temp_dir;
        processor.inspect(&input)
    })
    .await
    .map_err(|_| MediaError::ProcessingTaskFailed)??;
    let raw = json!({
        "width": info.width,
        "height": info.height,
        "duration_ms": info.duration_ms,
        "extractor": METADATA_VERSION,
        "media_kind": "video",
        "owner_metadata": { "status": "unsupported" },
    });
    Ok((info.width, info.height, raw))
}

/// Extracts bounded owner-only EXIF metadata without making optional metadata
/// failures fatal to otherwise valid media processing.
///
/// C005: input is attacker-controlled, so persisted entry count and value size
/// are bounded. C011: callers serving shares must omit this entire value.
#[must_use]
pub fn extract_owner_metadata(bytes: Vec<u8>) -> Value {
    let source = match MediaSource::from_memory(bytes) {
        Ok(source) => source,
        Err(error) => return metadata_parse_failure(&error),
    };
    let mut parser = MediaParser::new();
    let parsed = match parser.parse_exif(source) {
        Ok(parsed) => parsed,
        Err(error) => return metadata_parse_failure(&error),
    };
    let exif: Exif = parsed.into();
    let mut entries = BTreeMap::<String, BTreeMap<String, String>>::new();
    let mut stored_entry_count = 0_usize;
    let mut omitted_entry_count = 0_usize;
    let mut truncated_value_count = 0_usize;

    for entry in exif.iter() {
        if stored_entry_count >= MAX_OWNER_METADATA_ENTRIES {
            omitted_entry_count += 1;
            continue;
        }

        let value = entry.value.to_string();
        let mut chars = value.chars();
        let bounded = chars
            .by_ref()
            .take(MAX_OWNER_METADATA_VALUE_CHARS)
            .collect::<String>();
        if chars.next().is_some() {
            truncated_value_count += 1;
        }
        entries
            .entry(format!("ifd{}", entry.ifd.as_usize()))
            .or_default()
            .insert(entry.tag.to_string(), bounded);
        stored_entry_count += 1;
    }

    let gps = exif.gps_info().map(|gps| {
        json!({
            "latitude": gps.latitude_decimal(),
            "longitude": gps.longitude_decimal(),
            "altitude_meters": gps.altitude_meters(),
            "iso6709": gps.to_iso6709(),
        })
    });

    json!({
        "status": "parsed",
        "camera": {
            "make": exif.get(ExifTag::Make).and_then(|value| value.as_str()),
            "model": exif.get(ExifTag::Model).and_then(|value| value.as_str()),
        },
        "captured_at": exif
            .get(ExifTag::DateTimeOriginal)
            .map(ToString::to_string),
        "gps": gps,
        "entries": entries,
        "entry_error_count": exif.errors().len(),
        "omitted_entry_count": omitted_entry_count,
        "truncated_value_count": truncated_value_count,
        "has_embedded_track": exif.has_embedded_track(),
    })
}

fn metadata_parse_failure(error: &nom_exif::Error) -> Value {
    let status = match error {
        nom_exif::Error::ExifNotFound => "absent",
        nom_exif::Error::UnsupportedFormat => "unsupported",
        nom_exif::Error::Malformed { .. } | nom_exif::Error::UnexpectedEof { .. } => "malformed",
        _ => "unavailable",
    };
    json!({ "status": status })
}

/// Generates thumbnail and preview derivatives for an image asset.
///
/// Derivative object writes can outlive a failed DB transaction, but derivatives
/// are reproducible from originals and may be garbage-collected safely.
pub async fn generate_derivatives<I, V>(
    pool: &PgPool,
    storage: &ObjectStorage,
    image_processor: &I,
    video_processor: &V,
    asset_id: Uuid,
) -> Result<(), MediaError>
where
    I: ImageProcessor,
    V: VideoProcessor,
{
    let original = load_asset_original(pool, asset_id).await?;
    let media_kind = classify_media_type(&original.media_type)?;
    let (generated_derivatives, generator_version) = match media_kind {
        MediaKind::Image => (
            generate_image_derivatives(storage, image_processor, &original).await?,
            IMAGE_GENERATOR_VERSION,
        ),
        MediaKind::Video => (
            generate_video_derivatives(storage, image_processor, video_processor, &original)
                .await?,
            VIDEO_GENERATOR_VERSION,
        ),
    };

    for (kind, generated) in generated_derivatives {
        let key = StorageKey::derivative(
            &original.blake3_hash,
            kind.as_str(),
            generated.format,
            generator_version,
        )
        .map_err(|_| MediaError::UnsupportedMediaType)?;

        if !storage.exists(&key).await? {
            storage.write(&key, generated.bytes.clone()).await?;
        }

        sqlx::query!(
            r#"
            INSERT INTO derivatives (
                id,
                asset_id,
                kind,
                format,
                generator_version,
                source_blake3,
                storage_key,
                width,
                height,
                size_bytes
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            ON CONFLICT (asset_id, kind, format, generator_version)
            DO UPDATE SET
                source_blake3 = EXCLUDED.source_blake3,
                storage_key = EXCLUDED.storage_key,
                width = EXCLUDED.width,
                height = EXCLUDED.height,
                size_bytes = EXCLUDED.size_bytes
            "#,
            Uuid::now_v7(),
            asset_id,
            "thumbnail",
            "webp",
            "1",
            original.blake3_hash,
            key.as_str(),
            generated.width as i32,
            generated.height as i32,
            generated.bytes.len() as i64,
        )
        .execute(pool)
        .await?;
    }

    Ok(())
}

async fn generate_image_derivatives(
    storage: &ObjectStorage,
    processor: &impl ImageProcessor,
    original: &AssetOriginal,
) -> Result<Vec<(DerivativeKind, GeneratedDerivative)>, MediaError> {
    ensure_bounded_still_source(original)?;
    let bytes = storage
        .read(&original.storage_key()?)
        .await
        .map_err(MediaError::Storage)?;
    ensure_loaded_size(original, bytes.len())?;
    let processor = processor.clone();
    let media_type = original.media_type.clone();
    Ok(
        tokio::task::spawn_blocking(move || generate_image_sizes(&processor, &bytes, &media_type))
            .await
            .map_err(|_| MediaError::ProcessingTaskFailed)??,
    )
}

async fn generate_video_derivatives(
    storage: &ObjectStorage,
    image_processor: &impl ImageProcessor,
    video_processor: &impl VideoProcessor,
    original: &AssetOriginal,
) -> Result<Vec<(DerivativeKind, GeneratedDerivative)>, MediaError> {
    let (temp_dir, input) = stage_video_original(storage, original).await?;
    let video_processor = video_processor.clone();
    let poster = tokio::task::spawn_blocking(move || {
        let _temp_dir = temp_dir;
        let info = video_processor.inspect(&input)?;
        let poster_edge = DerivativeKind::Preview
            .max_edge()
            .min(info.width.max(info.height));
        video_processor.generate_poster(&input, poster_edge)
    })
    .await
    .map_err(|_| MediaError::ProcessingTaskFailed)??;
    let image_processor = image_processor.clone();
    Ok(tokio::task::spawn_blocking(move || {
        generate_image_sizes(&image_processor, &poster, "image/webp")
    })
    .await
    .map_err(|_| MediaError::ProcessingTaskFailed)??)
}

fn generate_image_sizes(
    processor: &impl ImageProcessor,
    bytes: &[u8],
    media_type: &str,
) -> Result<Vec<(DerivativeKind, GeneratedDerivative)>, MediaToolError> {
    [DerivativeKind::Thumbnail, DerivativeKind::Preview]
        .into_iter()
        .map(|kind| {
            processor
                .generate(bytes, media_type, kind)
                .map(|generated| (kind, generated))
        })
        .collect()
}

#[derive(Debug)]
struct AssetOriginal {
    blake3_hash: String,
    storage_key: String,
    media_type: String,
    size_bytes: i64,
}

impl AssetOriginal {
    fn storage_key(&self) -> Result<StorageKey, MediaError> {
        StorageKey::new(&self.storage_key).map_err(|_| MediaError::UnsupportedMediaType)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MediaKind {
    Image,
    Video,
}

async fn load_asset_original(pool: &PgPool, asset_id: Uuid) -> Result<AssetOriginal, MediaError> {
    sqlx::query!(
        r#"
        SELECT o.blake3_hash, o.storage_key, o.media_type, o.size_bytes
        FROM assets a
        JOIN originals o ON o.id = a.original_id
        WHERE a.id = $1
        "#,
        asset_id
    )
    .fetch_optional(pool)
    .await?
    .map(|row| AssetOriginal {
        blake3_hash: row.blake3_hash,
        storage_key: row.storage_key,
        media_type: row.media_type,
        size_bytes: row.size_bytes,
    })
    .ok_or(MediaError::AssetNotFound)
}

fn asset_id_from_payload(payload: &Value) -> Result<Uuid, MediaError> {
    payload
        .get("asset_id")
        .and_then(Value::as_str)
        .and_then(|value| Uuid::parse_str(value).ok())
        .ok_or(MediaError::InvalidJobPayload)
}

fn classify_media_type(media_type: &str) -> Result<MediaKind, MediaError> {
    match media_type {
        "image/jpeg" | "image/png" | "image/gif" | "image/webp" => Ok(MediaKind::Image),
        "video/mp4" | "video/quicktime" | "video/webm" | "video/x-matroska" => Ok(MediaKind::Video),
        _ => Err(MediaError::UnsupportedMediaType),
    }
}

fn ensure_bounded_still_source(original: &AssetOriginal) -> Result<(), MediaError> {
    if original.size_bytes > MAX_STILL_SOURCE_BYTES {
        return Err(MediaError::SourceTooLarge);
    }
    Ok(())
}

async fn stage_video_original(
    storage: &ObjectStorage,
    original: &AssetOriginal,
) -> Result<(tempfile::TempDir, PathBuf), MediaError> {
    if original.size_bytes > MAX_VIDEO_SOURCE_BYTES {
        return Err(MediaError::SourceTooLarge);
    }
    let temp_dir = tempfile::Builder::new()
        .prefix("mirror-media-")
        .tempdir()
        .map_err(MediaError::TemporaryStorage)?;
    let input = temp_dir.path().join("original");
    let copied = storage
        .copy_to_path_bounded(
            &original.storage_key()?,
            &input,
            MAX_VIDEO_SOURCE_BYTES as u64,
        )
        .await?;
    if copied != u64::try_from(original.size_bytes).map_err(|_| MediaError::OriginalSizeMismatch)? {
        return Err(MediaError::OriginalSizeMismatch);
    }
    Ok((temp_dir, input))
}

fn ensure_loaded_size(original: &AssetOriginal, loaded: usize) -> Result<(), MediaError> {
    let loaded = i64::try_from(loaded).map_err(|_| MediaError::OriginalSizeMismatch)?;
    if loaded != original.size_bytes {
        return Err(MediaError::OriginalSizeMismatch);
    }
    Ok(())
}

fn decode_image(bytes: &[u8], media_type: &str) -> Result<image::DynamicImage, MediaToolError> {
    let format = match media_type {
        "image/jpeg" => ImageFormat::Jpeg,
        "image/png" => ImageFormat::Png,
        "image/gif" => ImageFormat::Gif,
        "image/webp" => ImageFormat::WebP,
        _ => return Err(MediaToolError::UnsupportedMediaType),
    };
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_IMAGE_DIMENSION);
    limits.max_image_height = Some(MAX_IMAGE_DIMENSION);
    limits.max_alloc = Some(MAX_IMAGE_ALLOC_BYTES);

    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    reader.limits(limits);
    reader.decode().map_err(From::from)
}
