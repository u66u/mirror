//! Mirror background worker process.

use std::{io, sync::Arc, time::Duration as StdDuration};

use futures_util::future::try_join_all;
use mirror_backend::{
    config::Config,
    db,
    face::{OnnxFaceRuntime, SharedFaceRuntime},
    jobs::JobKind,
    media::HeifImageProcessor,
    ml::MlRuntime,
    ml::onnx_embedder::{OnnxImageTextEmbedder, OnnxSessionOptions},
    runtime::io_other,
    storage::ObjectStorage,
    telemetry,
    video::FfmpegVideoProcessor,
    worker::{self, WorkerHandlers, WorkerPolicy},
};
use tracing::{error, info};

#[actix_web::main]
async fn main() -> io::Result<()> {
    let config = Config::from_env().map_err(io_other)?;
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
    let session_options = OnnxSessionOptions::from(&config);
    let ml_runtime = MlRuntime::with_optional_concurrency_and_max_image_bytes(
        Arc::new(OnnxImageTextEmbedder::with_session_options(
            config.storage_root.clone(),
            config.ml_device,
            session_options,
        )),
        config.ml_concurrency_limit(),
        config.ml_max_image_bytes,
    );
    let face_runtime: SharedFaceRuntime = Arc::new(OnnxFaceRuntime::with_session_options(
        config.storage_root.clone(),
        config.ml_device,
        session_options,
    ));
    let worker_concurrency = config.worker_concurrency;
    let job_kinds = worker::production_job_kinds(config.face_recognition_enabled);
    let process = WorkerProcess {
        pool: &pool,
        storage: &storage,
        image_processor: &image_processor,
        video_processor: &video_processor,
        ml_runtime: &ml_runtime,
        face_runtime: &face_runtime,
        job_kinds: &job_kinds,
    };

    info!(worker_concurrency, "starting mirror worker");
    let workers = (0..worker_concurrency).map(|lane| {
        let worker_id = format!("worker-{}-{lane}", uuid::Uuid::now_v7());
        process.run_lane(worker_id)
    });
    try_join_all(workers).await?;
    Ok(())
}

struct WorkerProcess<'a> {
    pool: &'a sqlx::PgPool,
    storage: &'a ObjectStorage,
    image_processor: &'a HeifImageProcessor,
    video_processor: &'a FfmpegVideoProcessor,
    ml_runtime: &'a MlRuntime<OnnxImageTextEmbedder>,
    face_runtime: &'a SharedFaceRuntime,
    job_kinds: &'a [JobKind],
}

impl WorkerProcess<'_> {
    async fn run_lane(&self, worker_id: String) -> io::Result<()> {
        info!(%worker_id, "starting mirror worker lane");
        loop {
            match worker::run_once(
                self.pool,
                WorkerHandlers {
                    storage: self.storage,
                    image_processor: self.image_processor,
                    video_processor: self.video_processor,
                    ml_runtime: self.ml_runtime,
                    face_runtime: self.face_runtime,
                    job_kinds: self.job_kinds,
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
                        "worker job timed out; worker restart required",
                    ));
                }
                Err(worker::WorkerError::LeaseLost) => {
                    return Err(io::Error::other(
                        "worker job lease lost; worker restart required",
                    ));
                }
                Err(error) => {
                    error!(%error, "worker iteration failed");
                    actix_web::rt::time::sleep(StdDuration::from_secs(5)).await;
                }
            }
        }
    }
}
