//! Mirror background worker process.

use std::{io, num::NonZeroUsize, sync::Arc, time::Duration as StdDuration};

use futures_util::future::try_join_all;
use mirror_backend::{
    config::Config,
    db,
    face::{OnnxFaceRuntime, SharedFaceRuntime},
    media::HeifImageProcessor,
    ml::MlRuntime,
    onnx_embedder::{OnnxImageTextEmbedder, OnnxSessionOptions},
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
    let session_options = OnnxSessionOptions {
        intra_threads: config.ml_intra_threads,
        inter_threads: config.ml_inter_threads,
        parallel_execution: config.ml_parallel_execution,
    };
    let ml_runtime = MlRuntime::with_optional_concurrency_and_max_image_bytes(
        Arc::new(OnnxImageTextEmbedder::with_session_options(
            config.storage_root.clone(),
            config.ml_device,
            session_options,
        )),
        config
            .ml_max_concurrent_inferences
            .and_then(NonZeroUsize::new),
        config.ml_max_image_bytes,
    );
    let face_runtime: SharedFaceRuntime = Arc::new(OnnxFaceRuntime::with_session_options(
        config.storage_root.clone(),
        config.ml_device,
        session_options,
    ));
    let worker_concurrency = config.worker_concurrency;
    let job_kinds = worker::production_job_kinds(config.face_recognition_enabled);

    info!(worker_concurrency, "starting mirror worker");
    let workers = (0..worker_concurrency).map(|lane| {
        let worker_id = format!("worker-{}-{lane}", uuid::Uuid::now_v7());
        let pool = &pool;
        let storage = &storage;
        let image_processor = &image_processor;
        let video_processor = &video_processor;
        let ml_runtime = &ml_runtime;
        let face_runtime = &face_runtime;
        let job_kinds = &job_kinds;
        async move {
            info!(%worker_id, "starting mirror worker lane");
            loop {
                match worker::run_once(
                    pool,
                    WorkerHandlers {
                        storage,
                        image_processor,
                        video_processor,
                        ml_runtime,
                        face_runtime,
                        job_kinds,
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
                        return Err::<(), io::Error>(io::Error::new(
                            io::ErrorKind::TimedOut,
                            "worker job timed out; worker restart required",
                        ));
                    }
                    Err(worker::WorkerError::LeaseLost) => {
                        return Err::<(), io::Error>(io::Error::other(
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
    });
    try_join_all(workers).await?;
    Ok(())
}
