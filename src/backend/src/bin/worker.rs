//! Mirror background worker process.

use std::{io, num::NonZeroUsize, sync::Arc, time::Duration as StdDuration};

use mirror_backend::{
    config::Config,
    db,
    face::{OnnxFaceRuntime, SharedFaceRuntime},
    media::HeifImageProcessor,
    ml::MlRuntime,
    onnx_embedder::OnnxImageTextEmbedder,
    runtime::io_other,
    storage::ObjectStorage,
    telemetry,
    video::FfmpegVideoProcessor,
    worker::{self, WorkerHandlers, WorkerPolicy},
};
use tracing::{error, info};

#[actix_web::main]
async fn main() -> io::Result<()> {
    let config = Config::from_env();
    telemetry::init(&config.log_level);
    let database_url = config.database_url.as_deref().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "MIRROR_DATABASE_URL is required for worker",
        )
    })?;
    let pool = db::connect(database_url).await.map_err(io_other)?;
    db::run_migrations(&pool).await.map_err(io_other)?;
    std::fs::create_dir_all(&config.storage_root)?;
    let storage = ObjectStorage::local(&config.storage_root).map_err(io_other)?;
    let image_processor = HeifImageProcessor::production();
    let video_processor = FfmpegVideoProcessor::production();
    let ml_runtime = MlRuntime::with_max_image_bytes(
        Arc::new(OnnxImageTextEmbedder::new(
            config.storage_root.clone(),
            config.ml_device,
        )),
        NonZeroUsize::MIN,
        config.ml_max_image_bytes,
    );
    let face_runtime: SharedFaceRuntime = Arc::new(OnnxFaceRuntime::new(
        config.storage_root.clone(),
        config.ml_device,
    ));
    let worker_id = format!("worker-{}", uuid::Uuid::now_v7());
    let job_kinds = worker::production_job_kinds(config.face_recognition_enabled);

    info!(%worker_id, "starting mirror worker");
    loop {
        match worker::run_once(
            &pool,
            WorkerHandlers {
                storage: &storage,
                image_processor: &image_processor,
                video_processor: &video_processor,
                ml_runtime: &ml_runtime,
                face_runtime: &face_runtime,
                job_kinds: &job_kinds,
            },
            &worker_id,
            WorkerPolicy::production(),
        )
        .await
        {
            Ok(worker::WorkerStep::Idle) => {
                actix_web::rt::time::sleep(StdDuration::from_secs(1)).await;
            }
            Ok(worker::WorkerStep::Completed) => {}
            Ok(worker::WorkerStep::Failed) => {}
            Ok(worker::WorkerStep::TimedOut) => {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "media job timed out; worker restart required",
                ));
            }
            Err(worker::WorkerError::LeaseLost) => {
                return Err(io::Error::other(
                    "media job lease lost; worker restart required",
                ));
            }
            Err(error) => {
                error!(%error, "worker iteration failed");
                actix_web::rt::time::sleep(StdDuration::from_secs(5)).await;
            }
        }
    }
}
