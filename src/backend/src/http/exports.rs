//! Export routes.

use actix_web::{HttpRequest, HttpResponse, get, web};
use bytes::Bytes;
use futures_channel::mpsc;
use futures_util::{Sink, SinkExt, TryStreamExt, ready, stream::BoxStream};
use std::{
    path::Path,
    pin::Pin,
    task::{Context, Poll},
};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio_tar::{Builder as TarBuilder, EntryType, Header as TarHeader};
use tokio_util::io::StreamReader;
use uuid::Uuid;

use crate::{
    config::RateLimitQuota,
    exports::{self, ExportManifestInput, ExportManifestItem, ExportOriginalInput},
    http::{auth, error::ApiError},
    rate_limit::{self, QuotaInput},
    state::AppState,
    storage::{ObjectStorage, StorageKey},
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

const TAR_STREAM_CHANNEL_CAPACITY: usize = 8;

fn tar_stream(
    storage: ObjectStorage,
    items: Vec<ArchiveItem>,
) -> BoxStream<'static, Result<Bytes, std::io::Error>> {
    let (sender, receiver) = mpsc::channel(TAR_STREAM_CHANNEL_CAPACITY);
    tokio::spawn(async move {
        let mut writer = TarStreamWriter { sender };
        if let Err(error) = write_tar_archive(&mut writer, storage, items).await {
            writer.send_error(error).await;
        }
    });
    Box::pin(receiver)
}

async fn write_tar_archive(
    writer: &mut TarStreamWriter,
    storage: ObjectStorage,
    items: Vec<ArchiveItem>,
) -> Result<(), std::io::Error> {
    let mut archive = TarBuilder::new_non_terminated(writer);
    for item in items {
        match item {
            ArchiveItem::Bytes { path, bytes } => {
                validate_archive_path(&path)?;
                let mut header = tar_file_header(bytes.len() as u64);
                archive
                    .append_data(&mut header, Path::new(&path), std::io::Cursor::new(bytes))
                    .await?;
            }
            ArchiveItem::Object {
                path,
                storage_key,
                size_bytes,
            } => {
                validate_archive_path(&path)?;
                let stream = storage
                    .read_stream(&storage_key)
                    .await
                    .map_err(std::io::Error::other)?;
                let reader = StreamReader::new(stream.map_err(std::io::Error::other));
                let mut header = tar_file_header(size_bytes);
                archive
                    .append_data(
                        &mut header,
                        Path::new(&path),
                        ExactSizeReader::new(reader, size_bytes),
                    )
                    .await?;
            }
        }
    }
    archive.finish().await
}

fn validate_archive_path(path: &str) -> Result<(), std::io::Error> {
    if path.is_empty()
        || path.starts_with('/')
        || path.contains("..")
        || path.contains('\\')
        || path.as_bytes().contains(&0)
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "invalid export archive path",
        ));
    }
    Ok(())
}

fn tar_file_header(size: u64) -> TarHeader {
    let mut header = TarHeader::new_ustar();
    header.set_entry_type(EntryType::Regular);
    header.set_mode(0o644);
    header.set_uid(0);
    header.set_gid(0);
    header.set_mtime(0);
    header.set_size(size);
    header
}

struct TarStreamWriter {
    sender: mpsc::Sender<Result<Bytes, std::io::Error>>,
}

impl TarStreamWriter {
    async fn send_error(&mut self, error: std::io::Error) {
        let _ = self.sender.send(Err(error)).await;
    }
}

impl AsyncWrite for TarStreamWriter {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<Result<usize, std::io::Error>> {
        if buffer.is_empty() {
            return Poll::Ready(Ok(0));
        }
        ready!(Pin::new(&mut self.sender).poll_ready(cx)).map_err(|_| broken_pipe())?;
        Pin::new(&mut self.sender)
            .start_send(Ok(Bytes::copy_from_slice(buffer)))
            .map_err(|_| broken_pipe())?;
        Poll::Ready(Ok(buffer.len()))
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), std::io::Error>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), std::io::Error>> {
        Pin::new(&mut self.sender)
            .poll_close(cx)
            .map_err(|_| broken_pipe())
    }
}

fn broken_pipe() -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::BrokenPipe, "export stream closed")
}

struct ExactSizeReader<R> {
    inner: R,
    remaining: u64,
    eof_checked: bool,
}

impl<R> ExactSizeReader<R> {
    fn new(inner: R, size: u64) -> Self {
        Self {
            inner,
            remaining: size,
            eof_checked: false,
        }
    }
}

impl<R: AsyncRead + Unpin> AsyncRead for ExactSizeReader<R> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<Result<(), std::io::Error>> {
        if self.eof_checked {
            return Poll::Ready(Ok(()));
        }

        if self.remaining == 0 {
            let mut extra = [0_u8; 1];
            let mut extra_buffer = ReadBuf::new(&mut extra);
            ready!(Pin::new(&mut self.inner).poll_read(cx, &mut extra_buffer))?;
            if extra_buffer.filled().is_empty() {
                self.eof_checked = true;
                return Poll::Ready(Ok(()));
            }
            return Poll::Ready(Err(std::io::Error::other(
                "export original exceeded manifest size",
            )));
        }

        let remaining = usize::try_from(self.remaining).unwrap_or(usize::MAX);
        let max_read = buffer.remaining().min(remaining);
        if max_read == 0 {
            return Poll::Ready(Ok(()));
        }
        let output = buffer.initialize_unfilled_to(max_read);
        let mut limited_buffer = ReadBuf::new(output);
        ready!(Pin::new(&mut self.inner).poll_read(cx, &mut limited_buffer))?;
        let bytes_read = limited_buffer.filled().len();
        if bytes_read == 0 {
            return Poll::Ready(Err(std::io::Error::other(
                "export original shorter than manifest size",
            )));
        }
        self.remaining -= bytes_read as u64;
        buffer.advance(bytes_read);
        Poll::Ready(Ok(()))
    }
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
