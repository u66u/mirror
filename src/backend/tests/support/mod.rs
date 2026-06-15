use std::{env, fmt};

use mirror_backend::{
    assets::{AssetReadError, ListAssetsError, PromoteError},
    auth::{
        DeviceTokenError, OwnerLoginError, OwnerSetupError, OwnerSetupInput, PasswordError,
        SessionError, SetupState, SetupTokenError, TokenError,
    },
    db::{connect, run_migrations},
    jobs::JobError,
    media::{
        DerivativeKind, GeneratedDerivative, ImageInfo, ImageProcessor, MediaError, MediaToolError,
    },
    storage::ObjectStorage,
    storage::{StorageError, StorageKeyError},
    uploads::{CreateUploadInput, UploadError, complete_upload, create_upload, put_part},
    worker::WorkerError,
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
    sqlx::query("TRUNCATE jobs, originals, owner_accounts CASCADE")
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
    let bytes = jpeg_bytes();
    let upload = create_upload(
        pool,
        CreateUploadInput {
            owner_id: 1,
            original_filename: filename.to_owned(),
            expected_size: i64::try_from(bytes.len())?,
            expected_blake3: blake3::hash(&bytes).to_hex().to_string(),
            media_type: "image/jpeg".to_owned(),
        },
    )
    .await?;

    put_part(pool, storage, 1, upload.upload_id, 0, bytes).await?;
    complete_upload(pool, storage, 1, upload.upload_id).await?;

    Ok(upload.upload_id)
}

#[allow(dead_code)] // T204/T302: shared row assertion for asset-producing integration tests.
pub async fn asset_count(pool: &sqlx::PgPool) -> TestResult<i64> {
    Ok(sqlx::query_scalar("SELECT count(*) FROM assets")
        .fetch_one(pool)
        .await?)
}

#[allow(dead_code)] // T204/T301: shared row assertion for enqueue/worker integration tests.
pub async fn job_count(pool: &sqlx::PgPool) -> TestResult<i64> {
    Ok(sqlx::query_scalar("SELECT count(*) FROM jobs")
        .fetch_one(pool)
        .await?)
}

#[allow(dead_code)] // T204: shared row assertion for promotion integration tests.
pub async fn original_count(pool: &sqlx::PgPool) -> TestResult<i64> {
    Ok(sqlx::query_scalar("SELECT count(*) FROM originals")
        .fetch_one(pool)
        .await?)
}

#[allow(dead_code)] // T301: media handler and worker tests share deterministic processor output.
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
