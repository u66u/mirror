//! Media metadata and derivative job handlers.
//!
//! C005: media bytes are attacker-controlled. Still-image decoding runs in the
//! worker path only, never from request handlers. Video handling will use a
//! separate timeout-bounded external command wrapper.

use std::io::Cursor;

use image::{GenericImageView, ImageFormat};
use serde_json::{Value, json};
use sqlx::{PgPool, types::Json};
use uuid::Uuid;

use crate::{
    jobs::{JobKind, LeasedJob},
    storage::{ObjectStorage, StorageKey},
};

const GENERATOR_VERSION: &str = "media-v1-image-webp-1";

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
pub trait ImageProcessor {
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
        let resized = image.thumbnail(kind.max_edge(), kind.max_edge());
        let (width, height) = resized.dimensions();
        let mut output = Cursor::new(Vec::new());
        resized
            .write_to(&mut output, ImageFormat::WebP)
            .map_err(MediaToolError::Image)?;

        Ok(GeneratedDerivative {
            bytes: output.into_inner(),
            width,
            height,
            format: "webp",
        })
    }
}

/// Media job failure.
#[derive(Debug)]
pub enum MediaError {
    /// Job payload did not contain a valid internal asset UUID.
    InvalidJobPayload,
    /// Asset row was not found.
    AssetNotFound,
    /// Media type is not handled by the image pipeline.
    UnsupportedMediaType,
    /// Image processor failed.
    Tool(MediaToolError),
    /// Storage failed.
    Storage(crate::storage::StorageError),
    /// Database failed.
    Database(sqlx::Error),
}

impl std::fmt::Display for MediaError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::InvalidJobPayload => "invalid media job payload",
            Self::AssetNotFound => "asset not found",
            Self::UnsupportedMediaType => "unsupported media type",
            Self::Tool(_) => "media tool failed",
            Self::Storage(_) => "media storage error",
            Self::Database(_) => "media database error",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for MediaError {}

/// External media tool failure.
#[derive(Debug)]
pub enum MediaToolError {
    /// Media type is not image input supported by this processor.
    UnsupportedMediaType,
    /// Image decoder/encoder failed.
    Image(image::ImageError),
}

impl std::fmt::Display for MediaToolError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::UnsupportedMediaType => "unsupported media type",
            Self::Image(_) => "image processing failed",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for MediaToolError {}

/// Runs a leased media job without changing queue state.
///
/// The worker binary owns queue completion/failure so tests can exercise the
/// handler independently from leasing.
pub async fn run_media_job(
    pool: &PgPool,
    storage: &ObjectStorage,
    processor: &impl ImageProcessor,
    job: &LeasedJob,
) -> Result<(), MediaError> {
    let asset_id = asset_id_from_payload(&job.payload)?;
    match job.kind {
        JobKind::ExtractMetadata => extract_metadata(pool, storage, processor, asset_id).await,
        JobKind::GenerateDerivatives => {
            generate_derivatives(pool, storage, processor, asset_id).await
        }
    }
}

/// Extracts basic metadata for an asset original.
pub async fn extract_metadata(
    pool: &PgPool,
    storage: &ObjectStorage,
    processor: &impl ImageProcessor,
    asset_id: Uuid,
) -> Result<(), MediaError> {
    let original = load_asset_original(pool, asset_id).await?;
    let bytes = storage
        .read(
            &StorageKey::new(&original.storage_key)
                .map_err(|_| MediaError::UnsupportedMediaType)?,
        )
        .await
        .map_err(MediaError::Storage)?;
    ensure_supported_image(&original.media_type)?;
    let info = processor
        .inspect(&bytes, &original.media_type)
        .map_err(MediaError::Tool)?;
    let raw = json!({
        "width": info.width,
        "height": info.height,
        "extractor": GENERATOR_VERSION,
    });

    sqlx::query(
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
    )
    .bind(asset_id)
    .bind(i32::try_from(info.width).map_err(|_| MediaError::UnsupportedMediaType)?)
    .bind(i32::try_from(info.height).map_err(|_| MediaError::UnsupportedMediaType)?)
    .bind(Json(raw))
    .execute(pool)
    .await
    .map_err(MediaError::Database)?;

    Ok(())
}

/// Generates thumbnail and preview derivatives for an image asset.
///
/// Derivative object writes can outlive a failed DB transaction, but derivatives
/// are reproducible from originals and may be garbage-collected safely.
pub async fn generate_derivatives(
    pool: &PgPool,
    storage: &ObjectStorage,
    processor: &impl ImageProcessor,
    asset_id: Uuid,
) -> Result<(), MediaError> {
    let original = load_asset_original(pool, asset_id).await?;
    let original_key =
        StorageKey::new(&original.storage_key).map_err(|_| MediaError::UnsupportedMediaType)?;
    let bytes = storage
        .read(&original_key)
        .await
        .map_err(MediaError::Storage)?;
    ensure_supported_image(&original.media_type)?;

    for kind in [DerivativeKind::Thumbnail, DerivativeKind::Preview] {
        let generated = processor
            .generate(&bytes, &original.media_type, kind)
            .map_err(MediaError::Tool)?;
        let key = StorageKey::derivative(
            &original.blake3_hash,
            kind.as_str(),
            generated.format,
            GENERATOR_VERSION,
        )
        .map_err(|_| MediaError::UnsupportedMediaType)?;

        if !storage.exists(&key).await.map_err(MediaError::Storage)? {
            storage
                .write(&key, generated.bytes.clone())
                .await
                .map_err(MediaError::Storage)?;
        }

        sqlx::query(
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
        )
        .bind(Uuid::now_v7())
        .bind(asset_id)
        .bind(kind.as_str())
        .bind(generated.format)
        .bind(GENERATOR_VERSION)
        .bind(&original.blake3_hash)
        .bind(key.as_str())
        .bind(i32::try_from(generated.width).map_err(|_| MediaError::UnsupportedMediaType)?)
        .bind(i32::try_from(generated.height).map_err(|_| MediaError::UnsupportedMediaType)?)
        .bind(i64::try_from(generated.bytes.len()).map_err(|_| MediaError::UnsupportedMediaType)?)
        .execute(pool)
        .await
        .map_err(MediaError::Database)?;
    }

    Ok(())
}

#[derive(Debug)]
struct AssetOriginal {
    blake3_hash: String,
    storage_key: String,
    media_type: String,
}

async fn load_asset_original(pool: &PgPool, asset_id: Uuid) -> Result<AssetOriginal, MediaError> {
    sqlx::query_as::<_, (String, String, String)>(
        r#"
        SELECT o.blake3_hash, o.storage_key, o.media_type
        FROM assets a
        JOIN originals o ON o.id = a.original_id
        WHERE a.id = $1
        "#,
    )
    .bind(asset_id)
    .fetch_optional(pool)
    .await
    .map_err(MediaError::Database)?
    .map(|(blake3_hash, storage_key, media_type)| AssetOriginal {
        blake3_hash,
        storage_key,
        media_type,
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

fn ensure_supported_image(media_type: &str) -> Result<(), MediaError> {
    match media_type {
        "image/jpeg" | "image/png" | "image/gif" | "image/webp" => Ok(()),
        _ => Err(MediaError::UnsupportedMediaType),
    }
}

fn decode_image(bytes: &[u8], media_type: &str) -> Result<image::DynamicImage, MediaToolError> {
    let format = match media_type {
        "image/jpeg" => ImageFormat::Jpeg,
        "image/png" => ImageFormat::Png,
        "image/gif" => ImageFormat::Gif,
        "image/webp" => ImageFormat::WebP,
        _ => return Err(MediaToolError::UnsupportedMediaType),
    };
    image::load_from_memory_with_format(bytes, format).map_err(MediaToolError::Image)
}
