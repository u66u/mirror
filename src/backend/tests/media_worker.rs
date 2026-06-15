use std::io::Cursor;

use image::{ImageBuffer, ImageFormat, Rgba};
use mirror_backend::{
    assets::promote_verified_upload,
    jobs::{JobKind, LeasedJob},
    media::{
        DerivativeKind, ImageInfo, ImageProcessor, MediaError, RustImageProcessor,
        extract_metadata, generate_derivatives, run_media_job,
    },
    storage::StorageKey,
};
use serde_json::json;
use uuid::Uuid;

mod support;
use support::{
    FakeImageProcessor, StorageTestDeps, TestResult, create_verified_jpeg_upload, storage_test_deps,
};

#[test]
fn rust_image_processor_generates_bounded_webp_without_preserving_source_format() -> TestResult {
    let processor = RustImageProcessor;
    let bytes = png_fixture()?;

    let info = processor.inspect(&bytes, "image/png")?;
    let generated = processor.generate(&bytes, "image/png", DerivativeKind::Thumbnail)?;
    let generated_info = processor.inspect(&generated.bytes, "image/webp")?;

    assert_eq!(
        info,
        ImageInfo {
            width: 64,
            height: 32
        }
    );
    assert!(generated.width <= 512);
    assert!(generated.height <= 512);
    assert_eq!(generated.format, "webp");
    assert_eq!(
        generated_info,
        ImageInfo {
            width: generated.width,
            height: generated.height
        }
    );

    Ok(())
}

#[tokio::test]
#[ignore = "requires MIRROR_TEST_DATABASE_URL pointing at a dedicated test database"]
async fn metadata_and_derivative_handlers_persist_expected_rows() -> TestResult {
    let deps = storage_test_deps().await?;
    let asset_id = promoted_asset(&deps, "media.jpg").await?;
    let processor = FakeImageProcessor;

    extract_metadata(&deps.pool, &deps.storage, &processor, asset_id).await?;
    generate_derivatives(&deps.pool, &deps.storage, &processor, asset_id).await?;
    generate_derivatives(&deps.pool, &deps.storage, &processor, asset_id).await?;

    let (width, height, extractor) = sqlx::query_as::<_, (i32, i32, String)>(
        "SELECT width, height, raw->>'extractor' FROM asset_metadata WHERE asset_id = $1",
    )
    .bind(asset_id)
    .fetch_one(&deps.pool)
    .await?;
    let derivatives = sqlx::query_as::<_, (String, String, i32, i32, String)>(
        r#"
        SELECT kind, format, width, height, storage_key
        FROM derivatives
        WHERE asset_id = $1
        ORDER BY kind
        "#,
    )
    .bind(asset_id)
    .fetch_all(&deps.pool)
    .await?;

    assert_eq!((width, height), (4000, 3000));
    assert!(extractor.starts_with("media-v1-image-webp-"));
    assert_eq!(derivatives.len(), 2);
    assert_eq!(derivatives[0].0, "preview");
    assert_eq!(derivatives[0].1, "webp");
    assert_eq!((derivatives[0].2, derivatives[0].3), (1600, 1200));
    assert_eq!(derivatives[1].0, "thumbnail");
    assert_eq!((derivatives[1].2, derivatives[1].3), (512, 384));
    for (_, _, _, _, key) in derivatives {
        assert!(deps.storage.exists(&StorageKey::new(key)?).await?);
    }

    Ok(())
}

#[tokio::test]
#[ignore = "requires MIRROR_TEST_DATABASE_URL pointing at a dedicated test database"]
async fn media_job_rejects_invalid_payload_without_side_effects() -> TestResult {
    let deps = storage_test_deps().await?;
    let job = LeasedJob {
        id: Uuid::now_v7(),
        kind: JobKind::ExtractMetadata,
        payload: json!({ "asset_id": "not-a-uuid" }),
        attempts: 1,
        max_attempts: 5,
    };

    let result = run_media_job(&deps.pool, &deps.storage, &FakeImageProcessor, &job).await;

    assert!(matches!(result, Err(MediaError::InvalidJobPayload)));

    Ok(())
}

async fn promoted_asset(deps: &StorageTestDeps, filename: &str) -> TestResult<Uuid> {
    let upload_id = create_verified_jpeg_upload(&deps.pool, &deps.storage, filename).await?;
    let promoted = promote_verified_upload(&deps.pool, &deps.storage, 1, upload_id).await?;
    Ok(promoted.asset_id)
}

fn png_fixture() -> TestResult<Vec<u8>> {
    let image =
        image::DynamicImage::ImageRgba8(ImageBuffer::from_pixel(64, 32, Rgba([8, 80, 120, 255])));
    let mut output = Cursor::new(Vec::new());
    image.write_to(&mut output, ImageFormat::Png)?;
    Ok(output.into_inner())
}
