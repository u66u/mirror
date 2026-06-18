//! Export routes.

use actix_web::{HttpRequest, HttpResponse, get, web};
use bytes::Bytes;
use futures_util::{StreamExt, TryStreamExt, stream::BoxStream};
use std::{collections::VecDeque, pin::Pin};
use uuid::Uuid;

use crate::{
    config::RateLimitQuota,
    exports::{self, ExportManifestInput, ExportManifestItem, ExportOriginalInput},
    http::{auth, error::ApiError},
    rate_limit::{self, QuotaInput},
    state::AppState,
    storage::{ObjectStorage, StorageError, StorageKey},
};

const EXPORT_MANIFEST_ACTION: &str = "export_original_manifest";
const EXPORT_DOWNLOAD_ACTION: &str = "export_original_download";

/// Returns an owner export manifest for active originals.
#[get("/exports/originals/manifest")]
pub async fn original_manifest_route(
    state: web::Data<AppState>,
    req: HttpRequest,
) -> Result<HttpResponse, ApiError> {
    let Some(pool) = state.db.as_ref() else {
        return Err(ApiError::ServiceUnavailable(
            "database_unavailable",
            "database is unavailable",
        ));
    };
    let current = auth::require_owner(pool, &req).await?;
    reject_blocked_export(
        &state,
        pool,
        current.owner_id(),
        EXPORT_MANIFEST_ACTION,
        state.config.rate_limits.export_manifest,
    )
    .await?;
    let manifest = exports::original_manifest(
        pool,
        ExportManifestInput {
            owner_id: current.owner_id(),
        },
    )
    .await?;

    Ok(HttpResponse::Ok().json(manifest))
}

/// Streams one active original from an export manifest.
#[get("/exports/originals/archive.tar")]
pub async fn original_archive_route(
    state: web::Data<AppState>,
    req: HttpRequest,
) -> Result<HttpResponse, ApiError> {
    let Some(pool) = state.db.as_ref() else {
        return Err(ApiError::ServiceUnavailable(
            "database_unavailable",
            "database is unavailable",
        ));
    };
    let Some(storage) = state.storage.as_ref() else {
        return Err(ApiError::ServiceUnavailable(
            "storage_unavailable",
            "storage is unavailable",
        ));
    };
    let current = auth::require_owner(pool, &req).await?;
    reject_blocked_export(
        &state,
        pool,
        current.owner_id(),
        EXPORT_DOWNLOAD_ACTION,
        state.config.rate_limits.export_download,
    )
    .await?;
    let manifest = exports::original_manifest(
        pool,
        ExportManifestInput {
            owner_id: current.owner_id(),
        },
    )
    .await?;
    let manifest_bytes = serde_json::to_vec_pretty(&manifest).map_err(|_| ApiError::Internal)?;
    let mut items = Vec::with_capacity(manifest.items.len() + 1);
    items.push(ArchiveItem::Bytes {
        path: "manifest.json".to_owned(),
        bytes: Bytes::from(manifest_bytes),
    });
    for item in manifest.items {
        items.push(ArchiveItem::Object {
            path: archive_original_path(&item),
            storage_key: StorageKey::new(item.storage_key).map_err(|_| ApiError::Internal)?,
            size_bytes: u64::try_from(item.size_bytes).map_err(|_| ApiError::Internal)?,
        });
    }

    Ok(HttpResponse::Ok()
        .insert_header((
            "content-disposition",
            "attachment; filename=\"mirror-originals.tar\"",
        ))
        .content_type("application/x-tar")
        .streaming(tar_stream(storage.clone(), items)))
}

/// Streams one active original from an export manifest.
#[get("/exports/originals/{asset_id}")]
pub async fn original_blob_route(
    state: web::Data<AppState>,
    req: HttpRequest,
    path: web::Path<Uuid>,
) -> Result<HttpResponse, ApiError> {
    let Some(pool) = state.db.as_ref() else {
        return Err(ApiError::ServiceUnavailable(
            "database_unavailable",
            "database is unavailable",
        ));
    };
    let Some(storage) = state.storage.as_ref() else {
        return Err(ApiError::ServiceUnavailable(
            "storage_unavailable",
            "storage is unavailable",
        ));
    };
    let current = auth::require_owner(pool, &req).await?;
    reject_blocked_export(
        &state,
        pool,
        current.owner_id(),
        EXPORT_DOWNLOAD_ACTION,
        state.config.rate_limits.export_download,
    )
    .await?;
    let asset_id = path.into_inner();
    let original = exports::original_blob(
        pool,
        ExportOriginalInput {
            owner_id: current.owner_id(),
            asset_public_id: asset_id,
        },
    )
    .await?;
    let stream = storage
        .read_stream(&original.storage_key)
        .await
        .map_err(|_| ApiError::Internal)?;

    Ok(HttpResponse::Ok()
        .insert_header(("content-length", original.size_bytes.to_string()))
        .insert_header((
            "content-disposition",
            format!("attachment; filename=\"mirror-original-{asset_id}\""),
        ))
        .content_type(original.media_type)
        .streaming(stream.map_err(std::io::Error::other)))
}

async fn reject_blocked_export(
    state: &AppState,
    pool: &sqlx::PgPool,
    owner_id: i16,
    action: &'static str,
    quota: RateLimitQuota,
) -> Result<(), ApiError> {
    let key = owner_id.to_string();
    let now = time::OffsetDateTime::now_utc();
    if rate_limit::is_blocked(pool, &state.config.rate_limit_secret, action, &key, now)
        .await
        .map_err(|_| ApiError::Internal)?
    {
        return Err(ApiError::TooManyRequests(
            "rate_limited",
            "too many export requests",
        ));
    }

    let blocked = rate_limit::record_quota_attempt(
        pool,
        &state.config.rate_limit_secret,
        QuotaInput {
            action,
            key: &key,
            now,
            max_attempts: quota.max_per_window.saturating_add(1),
            window: quota.window,
            block_for: quota.block_for,
        },
    )
    .await
    .map_err(|_| ApiError::Internal)?;
    if blocked {
        Err(ApiError::TooManyRequests(
            "rate_limited",
            "too many export requests",
        ))
    } else {
        Ok(())
    }
}

enum ArchiveItem {
    Bytes {
        path: String,
        bytes: Bytes,
    },
    Object {
        path: String,
        storage_key: StorageKey,
        size_bytes: u64,
    },
}

enum TarPhase {
    Next,
    BytesContent {
        bytes: Option<Bytes>,
        padding: usize,
    },
    OpenObject {
        storage_key: StorageKey,
        size_bytes: u64,
    },
    ObjectContent {
        stream: Pin<Box<dyn futures_util::Stream<Item = Result<Bytes, StorageError>> + Send>>,
        remaining: u64,
        padding: usize,
    },
    Padding(usize),
    End(usize),
}

struct TarState {
    storage: ObjectStorage,
    items: VecDeque<ArchiveItem>,
    phase: TarPhase,
}

fn tar_stream(
    storage: ObjectStorage,
    items: Vec<ArchiveItem>,
) -> BoxStream<'static, Result<Bytes, std::io::Error>> {
    Box::pin(futures_util::stream::try_unfold(
        TarState {
            storage,
            items: items.into(),
            phase: TarPhase::Next,
        },
        |mut state| async move {
            loop {
                match state.phase {
                    TarPhase::Next => {
                        let Some(item) = state.items.pop_front() else {
                            state.phase = TarPhase::End(2);
                            continue;
                        };
                        match item {
                            ArchiveItem::Bytes { path, bytes } => {
                                let header = tar_header(&path, bytes.len() as u64)?;
                                state.phase = TarPhase::BytesContent {
                                    padding: tar_padding(bytes.len() as u64),
                                    bytes: Some(bytes),
                                };
                                return Ok(Some((header, state)));
                            }
                            ArchiveItem::Object {
                                path,
                                storage_key,
                                size_bytes,
                            } => {
                                let header = tar_header(&path, size_bytes)?;
                                state.phase = TarPhase::OpenObject {
                                    storage_key,
                                    size_bytes,
                                };
                                return Ok(Some((header, state)));
                            }
                        }
                    }
                    TarPhase::BytesContent {
                        ref mut bytes,
                        padding,
                    } => {
                        if let Some(bytes) = bytes.take() {
                            return Ok(Some((bytes, state)));
                        }
                        state.phase = TarPhase::Padding(padding);
                    }
                    TarPhase::OpenObject {
                        ref storage_key,
                        size_bytes,
                    } => {
                        let stream = state
                            .storage
                            .read_stream(storage_key)
                            .await
                            .map_err(std::io::Error::other)?;
                        state.phase = TarPhase::ObjectContent {
                            stream: Box::pin(stream),
                            remaining: size_bytes,
                            padding: tar_padding(size_bytes),
                        };
                    }
                    TarPhase::ObjectContent {
                        ref mut stream,
                        ref mut remaining,
                        padding,
                    } => match stream.next().await {
                        Some(Ok(bytes)) => {
                            let len = u64::try_from(bytes.len()).map_err(std::io::Error::other)?;
                            if len > *remaining {
                                return Err(std::io::Error::other(
                                    "export original exceeded manifest size",
                                ));
                            }
                            *remaining -= len;
                            return Ok(Some((bytes, state)));
                        }
                        Some(Err(error)) => return Err(std::io::Error::other(error)),
                        None => {
                            if *remaining != 0 {
                                return Err(std::io::Error::other(
                                    "export original shorter than manifest size",
                                ));
                            }
                            state.phase = TarPhase::Padding(padding);
                        }
                    },
                    TarPhase::Padding(remaining) => {
                        if remaining == 0 {
                            state.phase = TarPhase::Next;
                            continue;
                        }
                        state.phase = TarPhase::Next;
                        return Ok(Some((Bytes::from(vec![0_u8; remaining]), state)));
                    }
                    TarPhase::End(remaining_blocks) => {
                        if remaining_blocks == 0 {
                            return Ok(None);
                        }
                        state.phase = TarPhase::End(remaining_blocks - 1);
                        return Ok(Some((Bytes::from(vec![0_u8; 512]), state)));
                    }
                }
            }
        },
    ))
}

fn tar_header(path: &str, size: u64) -> Result<Bytes, std::io::Error> {
    if path.is_empty()
        || path.len() > 100
        || path.starts_with('/')
        || path.contains("..")
        || path.contains('\\')
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "invalid export archive path",
        ));
    }
    let mut header = [0_u8; 512];
    write_tar_field(&mut header[0..100], path.as_bytes());
    write_tar_octal(&mut header[100..108], 0o644);
    write_tar_octal(&mut header[108..116], 0);
    write_tar_octal(&mut header[116..124], 0);
    write_tar_octal(&mut header[124..136], size);
    write_tar_octal(&mut header[136..148], 0);
    header[148..156].fill(b' ');
    header[156] = b'0';
    write_tar_field(&mut header[257..263], b"ustar");
    write_tar_field(&mut header[263..265], b"00");
    let checksum = header.iter().map(|byte| u32::from(*byte)).sum::<u32>();
    let encoded = format!("{checksum:06o}\0 ");
    header[148..156].copy_from_slice(encoded.as_bytes());
    Ok(Bytes::copy_from_slice(&header))
}

fn write_tar_field(target: &mut [u8], value: &[u8]) {
    let len = value.len().min(target.len());
    target[..len].copy_from_slice(&value[..len]);
}

fn write_tar_octal(target: &mut [u8], value: u64) {
    let encoded = format!("{value:0width$o}\0", width = target.len() - 1);
    target.copy_from_slice(encoded.as_bytes());
}

fn tar_padding(size: u64) -> usize {
    let remainder = usize::try_from(size % 512).unwrap_or(0);
    if remainder == 0 { 0 } else { 512 - remainder }
}

fn archive_original_path(item: &ExportManifestItem) -> String {
    format!(
        "originals/{}{}",
        item.asset_id,
        archive_extension(&item.media_type)
    )
}

fn archive_extension(media_type: &str) -> &'static str {
    match media_type {
        "image/jpeg" => ".jpg",
        "image/png" => ".png",
        "image/gif" => ".gif",
        "image/webp" => ".webp",
        "image/heic" => ".heic",
        "image/heif" => ".heif",
        "video/mp4" => ".mp4",
        _ => "",
    }
}
