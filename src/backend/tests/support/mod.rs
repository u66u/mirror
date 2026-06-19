use std::{
    env, fmt,
    num::NonZeroUsize,
    path::Path,
    process::{Command, Stdio},
    sync::Arc,
    time::Duration,
};

use mirror_backend::{
    assets::{
        AssetMutationError, AssetReadError, ListAssetsError, PromoteError, promote_verified_upload,
    },
    auth::{
        DeviceTokenError, OwnerLoginError, OwnerSetupError, OwnerSetupInput, PasswordError,
        SessionError, SetupState, SetupTokenError, TokenError,
    },
    backups::BackupError,
    db::{connect, run_migrations},
    jobs::JobError,
    media::{
        DerivativeKind, GeneratedDerivative, ImageInfo, ImageProcessor, MediaError, MediaToolError,
    },
    ml::{EmbedImageRequest, EmbedTextRequest, ImageTextEmbedder, MlError, MlRuntime},
    models::{
        ImagePreprocessConfig, ModelPackError, ModelPackFileManifest, ModelPackManifest,
        ModelPackSelfTestManifest, OnnxModelPackConfig,
    },
    search::SearchError,
    semantic_index::SemanticIndexError,
    shares::ShareError,
    storage::ObjectStorage,
    storage::{StorageError, StorageKeyError},
    uploads::{CreateUploadInput, UploadError, complete_upload, create_upload, put_part},
    video::{VideoInfo, VideoProcessor, VideoToolError},
    worker::{WorkerError, WorkerPolicyError},
};
use tempfile::TempDir;
use uuid::Uuid;

#[allow(dead_code)] // T105/T202/T204: shared DB fixture is imported by tests with different fixture needs.
pub const OWNER_PASSWORD: &str = "correct horse battery staple";

pub type TestResult<T = ()> = Result<T, TestError>;

#[derive(Debug)]
pub struct TestError {
    context: &'static str,
    message: String,
}

impl TestError {
    fn new(context: &'static str, error: impl fmt::Display) -> Self {
        Self {
            context,
            message: error.to_string(),
        }
    }
}

impl fmt::Display for TestError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.context, self.message)
    }
}

impl std::error::Error for TestError {}

/// Reads one required string field from a JSON response.
#[allow(dead_code)] // T105: HTTP integration tests use this only for typed response assertions.
pub fn required_json_string(value: &serde_json::Value, field: &'static str) -> TestResult<String> {
    value
        .get(field)
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| TestError::new("required JSON string missing", field))
}

impl From<std::num::TryFromIntError> for TestError {
    fn from(error: std::num::TryFromIntError) -> Self {
        Self::new("integer conversion failed", error)
    }
}

impl From<std::io::Error> for TestError {
    fn from(error: std::io::Error) -> Self {
        Self::new("io failed", error)
    }
}

impl From<image::ImageError> for TestError {
    fn from(error: image::ImageError) -> Self {
        Self::new("image failed", error)
    }
}

impl From<sqlx::Error> for TestError {
    fn from(error: sqlx::Error) -> Self {
        Self::new("sql failed", error)
    }
}

impl From<sqlx::migrate::MigrateError> for TestError {
    fn from(error: sqlx::migrate::MigrateError) -> Self {
        Self::new("migration failed", error)
    }
}

impl From<PasswordError> for TestError {
    fn from(error: PasswordError) -> Self {
        Self::new("password failed", error)
    }
}

impl From<SetupTokenError> for TestError {
    fn from(error: SetupTokenError) -> Self {
        Self::new("setup token failed", error)
    }
}

impl From<TokenError> for TestError {
    fn from(error: TokenError) -> Self {
        Self::new("token failed", error)
    }
}

impl From<OwnerSetupError> for TestError {
    fn from(error: OwnerSetupError) -> Self {
        Self::new("owner setup failed", error)
    }
}

impl From<OwnerLoginError> for TestError {
    fn from(error: OwnerLoginError) -> Self {
        Self::new("owner login failed", error)
    }
}

impl From<SessionError> for TestError {
    fn from(error: SessionError) -> Self {
        Self::new("session failed", error)
    }
}

impl From<DeviceTokenError> for TestError {
    fn from(error: DeviceTokenError) -> Self {
        Self::new("device token failed", error)
    }
}

impl From<StorageError> for TestError {
    fn from(error: StorageError) -> Self {
        Self::new("storage failed", error)
    }
}

impl From<StorageKeyError> for TestError {
    fn from(error: StorageKeyError) -> Self {
        Self::new("storage key failed", error)
    }
}

impl From<UploadError> for TestError {
    fn from(error: UploadError) -> Self {
        Self::new("upload failed", error)
    }
}

impl From<PromoteError> for TestError {
    fn from(error: PromoteError) -> Self {
        Self::new("promote failed", error)
    }
}

impl From<ListAssetsError> for TestError {
    fn from(error: ListAssetsError) -> Self {
        Self::new("asset list failed", error)
    }
}

impl From<AssetReadError> for TestError {
    fn from(error: AssetReadError) -> Self {
        Self::new("asset read failed", error)
    }
}

impl From<AssetMutationError> for TestError {
    fn from(error: AssetMutationError) -> Self {
        Self::new("asset mutation failed", error)
    }
}

impl From<ShareError> for TestError {
    fn from(error: ShareError) -> Self {
        Self::new("share failed", error)
    }
}

impl From<BackupError> for TestError {
    fn from(error: BackupError) -> Self {
        Self::new("backup failed", error)
    }
}

impl From<JobError> for TestError {
    fn from(error: JobError) -> Self {
        Self::new("job failed", error)
    }
}

impl From<MediaError> for TestError {
    fn from(error: MediaError) -> Self {
        Self::new("media failed", error)
    }
}

impl From<ModelPackError> for TestError {
    fn from(error: ModelPackError) -> Self {
        Self::new("model pack failed", error)
    }
}

impl From<MlError> for TestError {
    fn from(error: MlError) -> Self {
        Self::new("ml failed", error)
    }
}

impl From<SemanticIndexError> for TestError {
    fn from(error: SemanticIndexError) -> Self {
        Self::new("semantic index failed", error)
    }
}

impl From<SearchError> for TestError {
    fn from(error: SearchError) -> Self {
        Self::new("search failed", error)
    }
}

impl From<MediaToolError> for TestError {
    fn from(error: MediaToolError) -> Self {
        Self::new("media tool failed", error)
    }
}

impl From<WorkerError> for TestError {
    fn from(error: WorkerError) -> Self {
        Self::new("worker failed", error)
    }
}

impl From<WorkerPolicyError> for TestError {
    fn from(error: WorkerPolicyError) -> Self {
        Self::new("worker policy failed", error)
    }
}

#[allow(dead_code)] // T501/T502: shared semantic model-pack fixture across model/ML/search tests.
pub fn valid_model_pack_manifest() -> ModelPackManifest {
    ModelPackManifest {
        kind: "semantic_image_text".to_owned(),
        runtime: "onnx".to_owned(),
        model_key: "siglip2-base-patch16-224".to_owned(),
        model_revision: "2026-06-18.1".to_owned(),
        license: "Apache-2.0".to_owned(),
        embedding_dimension: 768,
        distance_metric: "cosine".to_owned(),
        onnx: valid_onnx_config(),
        image_preprocess: valid_image_preprocess(),
        face_detection: None,
        face_embedding: None,
        files: vec![
            ModelPackFileManifest {
                path: "models/image_encoder.onnx".to_owned(),
                sha256: "a".repeat(64),
                size_bytes: 10,
            },
            ModelPackFileManifest {
                path: "models/text_encoder.onnx".to_owned(),
                sha256: "b".repeat(64),
                size_bytes: 11,
            },
            ModelPackFileManifest {
                path: "tokenizer/tokenizer.json".to_owned(),
                sha256: "c".repeat(64),
                size_bytes: 12,
            },
        ],
        self_tests: vec![ModelPackSelfTestManifest {
            name: "text_image_fixture_similarity".to_owned(),
            input_path: "self-tests/cat.jpg".to_owned(),
            expected_output_sha256: "d".repeat(64),
        }],
    }
}

#[allow(dead_code)] // T501/T502: shared semantic model-pack fixture across model/ML/search tests.
pub fn valid_onnx_config() -> OnnxModelPackConfig {
    OnnxModelPackConfig {
        image_model_path: "models/image_encoder.onnx".to_owned(),
        text_model_path: "models/text_encoder.onnx".to_owned(),
        tokenizer_path: "tokenizer/tokenizer.json".to_owned(),
        image_input_name: "pixel_values".to_owned(),
        image_output_name: "image_embeds".to_owned(),
        text_input_ids_name: "input_ids".to_owned(),
        text_attention_mask_name: "attention_mask".to_owned(),
        text_output_name: "text_embeds".to_owned(),
    }
}

#[allow(dead_code)] // T501/T502: shared semantic model-pack fixture across model/ML/search tests.
pub fn valid_image_preprocess() -> ImagePreprocessConfig {
    ImagePreprocessConfig {
        width: 224,
        height: 224,
        color_order: "rgb".to_owned(),
        tensor_layout: "nchw".to_owned(),
        mean: [0.5, 0.5, 0.5],
        std: [0.5, 0.5, 0.5],
    }
}

#[allow(dead_code)] // T506: shared face detector fixture config.
pub fn valid_face_detection_config() -> mirror_backend::models::FaceDetectionModelConfig {
    mirror_backend::models::FaceDetectionModelConfig {
        model_path: "models/face_detector.onnx".to_owned(),
        input_name: "image".to_owned(),
        boxes_output_name: "boxes".to_owned(),
        scores_output_name: "scores".to_owned(),
        landmarks_output_name: None,
        box_coordinate_space: "normalized".to_owned(),
        box_format: "xywh".to_owned(),
        score_threshold: 0.5,
        nms_threshold: 0.3,
        max_faces: 100,
    }
}

#[allow(dead_code)] // T506: shared face embedding fixture config.
pub fn valid_face_embedding_config() -> mirror_backend::models::FaceEmbeddingModelConfig {
    mirror_backend::models::FaceEmbeddingModelConfig {
        model_path: "models/face_embedding.onnx".to_owned(),
        input_name: "face".to_owned(),
        output_name: "embedding".to_owned(),
        width: 112,
        height: 112,
        color_order: "rgb".to_owned(),
        tensor_layout: "nchw".to_owned(),
        alignment: "five_point".to_owned(),
        mean: [0.5, 0.5, 0.5],
        std: [0.5, 0.5, 0.5],
        match_threshold: 0.75,
    }
}

impl From<VideoToolError> for TestError {
    fn from(error: VideoToolError) -> Self {
        Self::new("video tool failed", error)
    }
}

impl From<mirror_backend::face::FaceIndexError> for TestError {
    fn from(error: mirror_backend::face::FaceIndexError) -> Self {
        Self::new("face index failed", error)
    }
}

impl From<mirror_backend::people::PeopleReviewError> for TestError {
    fn from(error: mirror_backend::people::PeopleReviewError) -> Self {
        Self::new("people review failed", error)
    }
}

impl From<tokio::task::JoinError> for TestError {
    fn from(error: tokio::task::JoinError) -> Self {
        Self::new("async task failed", error)
    }
}

#[allow(dead_code)] // T103/T105/T202/T204: DB-backed integration tests share this opt-in fixture.
pub async fn connect_test_database() -> TestResult<sqlx::PgPool> {
    let database_url = env::var("MIRROR_TEST_DATABASE_URL").map_err(|source| TestError {
        context: "MIRROR_TEST_DATABASE_URL required",
        message: source.to_string(),
    })?;
    let pool = connect(&database_url).await?;
    run_migrations(&pool).await?;
    Ok(pool)
}

#[allow(dead_code)] // T105/T202/T204: auth/upload tests need a known owner, pure tests do not.
pub async fn fresh_owner_pool() -> TestResult<sqlx::PgPool> {
    let pool = connect_test_database().await?;
    reset_owner(&pool).await?;
    Ok(pool)
}

/// Shared Postgres plus isolated local object-storage fixture.
#[allow(dead_code)] // T202/T204/T301/T302: only DB/storage integration test crates construct this fixture.
pub struct StorageTestDeps {
    pub pool: sqlx::PgPool,
    pub storage: ObjectStorage,
    _temp_dir: TempDir,
}

#[allow(dead_code)] // T202/T204/T301/T302: imported by DB/storage integration tests.
pub async fn storage_test_deps() -> TestResult<StorageTestDeps> {
    let pool = fresh_owner_pool().await?;
    let temp_dir = TempDir::new()?;
    let storage = ObjectStorage::local(temp_dir.path())?;

    Ok(StorageTestDeps {
        pool,
        storage,
        _temp_dir: temp_dir,
    })
}

#[allow(dead_code)] // T105/T202/T204: called through `fresh_owner_pool` in DB-backed test crates.
pub async fn reset_owner(pool: &sqlx::PgPool) -> TestResult {
    sqlx::query!(
        "TRUNCATE jobs, originals, owner_accounts, rate_limit_buckets, model_packs CASCADE",
    )
    .execute(pool)
    .await?;

    let (setup, setup_token) = SetupState::pending()?;
    mirror_backend::auth::create_owner(
        pool,
        &setup,
        OwnerSetupInput {
            setup_token,
            display_name: "Owner".to_owned(),
            password: OWNER_PASSWORD.to_owned(),
        },
    )
    .await?;

    Ok(())
}

#[allow(dead_code)] // T202/T204/T302: upload/promotion/timeline tests share one valid tiny JPEG fixture.
pub fn jpeg_bytes() -> Vec<u8> {
    vec![
        0xff, 0xd8, 0xff, 0xe0, 0x00, 0x10, b'M', b'i', b'r', b'r', b'o', b'r', 0xff, 0xd9,
    ]
}

#[allow(dead_code)] // T202/T204/T301/T302: DB/storage integration tests share the verified-upload setup path.
pub async fn create_verified_jpeg_upload(
    pool: &sqlx::PgPool,
    storage: &ObjectStorage,
    filename: &str,
) -> TestResult<Uuid> {
    create_verified_upload(pool, storage, filename, "image/jpeg", jpeg_bytes()).await
}

#[allow(dead_code)] // T301: video integration uses the same verified upload contract.
pub async fn create_verified_upload(
    pool: &sqlx::PgPool,
    storage: &ObjectStorage,
    filename: &str,
    media_type: &str,
    bytes: Vec<u8>,
) -> TestResult<Uuid> {
    let upload = create_upload(
        pool,
        CreateUploadInput {
            owner_id: 1,
            original_filename: filename.to_owned(),
            expected_size: i64::try_from(bytes.len())?,
            expected_blake3: blake3::hash(&bytes).to_hex().to_string(),
            media_type: media_type.to_owned(),
            client_upload_key: None,
        },
    )
    .await?;

    put_part(pool, storage, 1, upload.upload_id, 0, bytes).await?;
    complete_upload(pool, storage, 1, upload.upload_id).await?;

    Ok(upload.upload_id)
}

#[allow(dead_code)] // T302/T401: timeline/share tests need both DB id and public API id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PromotedAssetIds {
    pub internal_id: Uuid,
    pub public_id: Uuid,
}

#[allow(dead_code)] // T302/T401: DB/storage integration tests share the verified-promotion path.
pub async fn create_promoted_asset(
    deps: &StorageTestDeps,
    filename: &str,
) -> TestResult<PromotedAssetIds> {
    let upload_id = create_verified_jpeg_upload(&deps.pool, &deps.storage, filename).await?;
    let promoted = promote_verified_upload(&deps.pool, &deps.storage, 1, upload_id).await?;
    let internal_id = sqlx::query_scalar!(
        "SELECT id FROM assets WHERE public_id = $1",
        promoted.asset_id
    )
    .fetch_one(&deps.pool)
    .await?;

    Ok(PromotedAssetIds {
        internal_id,
        public_id: promoted.asset_id,
    })
}

#[allow(dead_code)] // T301: external-tool and DB media tests share capability detection.
pub fn ffmpeg_is_available() -> bool {
    Command::new("ffmpeg")
        .arg("-version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
        && Command::new("ffprobe")
            .arg("-version")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
}

#[allow(dead_code)] // T301: external-tool and DB media tests share one tiny video fixture.
pub fn create_video_fixture(path: &Path) -> TestResult {
    let status = Command::new("ffmpeg")
        .args([
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "color=c=red:s=64x32:d=0.2",
            "-c:v",
            "mpeg4",
            "-pix_fmt",
            "yuv420p",
            "-y",
        ])
        .arg(path)
        .status()?;
    if !status.success() {
        return Err(TestError::new(
            "video fixture failed",
            "ffmpeg exited unsuccessfully",
        ));
    }
    Ok(())
}

#[cfg(unix)]
#[allow(dead_code)] // T301: video command tests share executable fixture setup.
pub fn write_executable_script(path: &Path, body: &str) -> TestResult {
    use std::os::unix::fs::PermissionsExt;

    std::fs::write(path, body)?;
    let mut permissions = std::fs::metadata(path)?.permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(path, permissions)?;
    Ok(())
}

#[allow(dead_code)] // T204/T302: shared row assertion for asset-producing integration tests.
pub async fn asset_count(pool: &sqlx::PgPool) -> TestResult<i64> {
    Ok(
        sqlx::query_scalar!(r#"SELECT count(*) as "count!" FROM assets"#)
            .fetch_one(pool)
            .await?,
    )
}

#[allow(dead_code)] // T204/T301: shared row assertion for enqueue/worker integration tests.
pub async fn job_count(pool: &sqlx::PgPool) -> TestResult<i64> {
    Ok(
        sqlx::query_scalar!(r#"SELECT count(*) as "count!" FROM jobs"#)
            .fetch_one(pool)
            .await?,
    )
}

#[allow(dead_code)] // T204: shared row assertion for promotion integration tests.
pub async fn original_count(pool: &sqlx::PgPool) -> TestResult<i64> {
    Ok(
        sqlx::query_scalar!(r#"SELECT count(*) as "count!" FROM originals"#)
            .fetch_one(pool)
            .await?,
    )
}

#[allow(dead_code)] // T301: media handler and worker tests share deterministic processor output.
#[derive(Clone, Copy)]
pub struct FakeImageProcessor;

impl ImageProcessor for FakeImageProcessor {
    fn inspect(&self, _bytes: &[u8], _media_type: &str) -> Result<ImageInfo, MediaToolError> {
        Ok(ImageInfo {
            width: 4000,
            height: 3000,
        })
    }

    fn generate(
        &self,
        _bytes: &[u8],
        _media_type: &str,
        kind: DerivativeKind,
    ) -> Result<GeneratedDerivative, MediaToolError> {
        let (bytes, width, height) = match kind {
            DerivativeKind::Thumbnail => (b"thumbnail".to_vec(), 512, 384),
            DerivativeKind::Preview => (b"preview".to_vec(), 1600, 1200),
        };
        Ok(GeneratedDerivative {
            bytes,
            width,
            height,
            format: "webp",
        })
    }
}

#[allow(dead_code)] // T301: image-focused worker tests still require complete media tool wiring.
#[derive(Clone, Copy)]
pub struct FakeVideoProcessor;

impl VideoProcessor for FakeVideoProcessor {
    fn inspect(&self, _input: &std::path::Path) -> Result<VideoInfo, VideoToolError> {
        Ok(VideoInfo {
            width: 1920,
            height: 1080,
            duration_ms: Some(10_000),
        })
    }

    fn generate_poster(
        &self,
        _input: &std::path::Path,
        _max_edge: u32,
    ) -> Result<Vec<u8>, VideoToolError> {
        Err(VideoToolError::CommandFailed)
    }
}

#[allow(dead_code)] // T301: worker timeout and heartbeat tests need deterministic slow CPU work.
#[derive(Clone, Copy)]
pub struct DelayedImageProcessor {
    pub delay: Duration,
}

impl ImageProcessor for DelayedImageProcessor {
    fn inspect(&self, _bytes: &[u8], _media_type: &str) -> Result<ImageInfo, MediaToolError> {
        std::thread::sleep(self.delay);
        Ok(ImageInfo {
            width: 4000,
            height: 3000,
        })
    }

    fn generate(
        &self,
        _bytes: &[u8],
        _media_type: &str,
        kind: DerivativeKind,
    ) -> Result<GeneratedDerivative, MediaToolError> {
        std::thread::sleep(self.delay);
        let (bytes, width, height) = match kind {
            DerivativeKind::Thumbnail => (b"thumbnail".to_vec(), 512, 384),
            DerivativeKind::Preview => (b"preview".to_vec(), 1600, 1200),
        };
        Ok(GeneratedDerivative {
            bytes,
            width,
            height,
            format: "webp",
        })
    }
}

#[allow(dead_code)] // T501: worker ML tests need deterministic embedding output.
#[derive(Clone, Copy)]
pub struct FakeImageTextEmbedder;

impl ImageTextEmbedder for FakeImageTextEmbedder {
    fn embed_image(&self, request: EmbedImageRequest<'_>) -> Result<Vec<f32>, MlError> {
        if !request.media_type.starts_with("image/") || request.bytes.is_empty() {
            return Err(MlError::UnsupportedMediaType);
        }
        Ok(vec![1.0; request.manifest.embedding_dimension as usize])
    }

    fn embed_text(&self, request: EmbedTextRequest<'_>) -> Result<Vec<f32>, MlError> {
        if request.text.is_empty() {
            return Err(MlError::InvalidTextQuery);
        }
        let mut values = vec![0.0; request.manifest.embedding_dimension as usize];
        values[0] = 1.0;
        Ok(values)
    }
}

#[allow(dead_code)] // T501: worker ML tests need a blocking-runtime wrapper.
pub fn fake_ml_runtime() -> MlRuntime<FakeImageTextEmbedder> {
    MlRuntime::new(Arc::new(FakeImageTextEmbedder), NonZeroUsize::MIN)
}
