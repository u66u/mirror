use std::{
    fs,
    io::Cursor,
    process::{Command, Stdio},
    time::Duration,
};

use image::{ImageBuffer, ImageFormat, Rgba};
use mirror_backend::{
    assets::promote_verified_upload,
    jobs::{JobKind, LeasedJob},
    media::{
        DerivativeKind, HeifImageProcessor, ImageInfo, ImageProcessor, MediaError, MediaToolError,
        RustImageProcessor, extract_metadata, extract_owner_metadata, generate_derivatives,
        run_media_job,
    },
    storage::StorageKey,
    video::FfmpegVideoProcessor,
};
use serde_json::json;
use tempfile::TempDir;
use uuid::Uuid;

mod support;
use support::{
    FakeImageProcessor, FakeVideoProcessor, StorageTestDeps, TestResult,
    create_verified_jpeg_upload, create_verified_upload, create_video_fixture, ffmpeg_is_available,
    storage_test_deps,
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

#[test]
fn rust_image_processor_rejects_decompression_bomb_dimensions() -> TestResult {
    let processor = RustImageProcessor;
    let bytes = png_fixture_with_dimensions(32_769, 1)?;

    let result = processor.inspect(&bytes, "image/png");

    assert!(matches!(
        result,
        Err(MediaToolError::Image(image::ImageError::Limits(_)))
    ));
    Ok(())
}

#[test]
fn heif_processor_reports_missing_converter_for_heic_inputs() {
    let processor = HeifImageProcessor::new("__mirror_missing_heif_convert__");

    let result = processor.inspect(b"\0\0\0\x18ftypheic\0\0\0\0mif1heic", "image/heic");

    assert!(matches!(result, Err(MediaToolError::HeifToolUnavailable)));
}

#[test]
fn heif_processor_uses_host_converter_when_available() -> TestResult {
    if !command_available("heif-enc") || !command_available("heif-convert") {
        return Ok(());
    }
    let temp_dir = TempDir::new()?;
    let source = temp_dir.path().join("source.png");
    let heic = temp_dir.path().join("source.heic");
    fs::write(&source, png_fixture_with_dimensions(16, 8)?)?;
    let status = Command::new("heif-enc")
        .arg("-o")
        .arg(&heic)
        .arg(&source)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;
    if !status.success() {
        return Ok(());
    }
    let bytes = fs::read(heic)?;
    let processor = HeifImageProcessor::production();

    let info = processor.inspect(&bytes, "image/heic")?;
    let generated = processor.generate(&bytes, "image/heic", DerivativeKind::Thumbnail)?;

    assert_eq!(
        info,
        ImageInfo {
            width: 16,
            height: 8
        }
    );
    assert_eq!(generated.format, "webp");
    assert_eq!((generated.width, generated.height), (16, 8));

    Ok(())
}

#[test]
fn owner_metadata_extracts_camera_capture_time_and_gps() -> TestResult {
    let metadata = extract_owner_metadata(jpeg_with_exif_fixture()?);

    assert_eq!(metadata["status"], "parsed");
    assert_eq!(metadata["camera"]["make"], "Mirror");
    assert_eq!(metadata["camera"]["model"], "VaultCam");
    assert_eq!(metadata["captured_at"], "2026-06-15 12:00:00");
    assert_eq!(metadata["gps"]["latitude"], 55.75);
    assert!(
        (metadata["gps"]["longitude"].as_f64().unwrap_or_default() - 37.616_666_666_666_67).abs()
            < 0.000_001
    );
    assert_eq!(metadata["gps"]["altitude_meters"], 156.0);
    assert_eq!(metadata["entries"]["ifd0"]["Make"], "Mirror");
    assert_eq!(metadata["entry_error_count"], 0);

    Ok(())
}

#[test]
fn missing_exif_is_nonfatal_and_derivatives_strip_owner_metadata() -> TestResult {
    let processor = RustImageProcessor;
    let source = jpeg_with_exif_fixture()?;
    let source_metadata = extract_owner_metadata(source.clone());
    let generated = processor.generate(&source, "image/jpeg", DerivativeKind::Preview)?;
    let generated_metadata = extract_owner_metadata(generated.bytes.clone());

    assert_eq!(source_metadata["status"], "parsed");
    assert_ne!(generated_metadata["status"], "parsed");
    assert!(
        !generated
            .bytes
            .windows("Mirror".len())
            .any(|window| window == b"Mirror")
    );

    Ok(())
}

#[tokio::test]
#[ignore = "requires MIRROR_TEST_DATABASE_URL pointing at a dedicated test database"]
async fn metadata_and_derivative_handlers_persist_expected_rows() -> TestResult {
    let deps = storage_test_deps().await?;
    let asset_id = promoted_asset(&deps, "media.jpg").await?;
    let processor = FakeImageProcessor;

    extract_metadata(
        &deps.pool,
        &deps.storage,
        &processor,
        &FakeVideoProcessor,
        asset_id,
    )
    .await?;
    generate_derivatives(
        &deps.pool,
        &deps.storage,
        &processor,
        &FakeVideoProcessor,
        asset_id,
    )
    .await?;
    generate_derivatives(
        &deps.pool,
        &deps.storage,
        &processor,
        &FakeVideoProcessor,
        asset_id,
    )
    .await?;

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
    assert_eq!(extractor, "media-metadata-v2");
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
async fn video_handlers_stream_original_and_persist_posters() -> TestResult {
    if !ffmpeg_is_available() {
        return Ok(());
    }
    let deps = storage_test_deps().await?;
    let fixture_dir = TempDir::new()?;
    let fixture_path = fixture_dir.path().join("video.mp4");
    create_video_fixture(&fixture_path)?;
    let bytes = fs::read(&fixture_path)?;
    let upload_id =
        create_verified_upload(&deps.pool, &deps.storage, "video.mp4", "video/mp4", bytes).await?;
    let promoted = promote_verified_upload(&deps.pool, &deps.storage, 1, upload_id).await?;
    let asset_id = sqlx::query_scalar("SELECT id FROM assets WHERE public_id = $1")
        .bind(promoted.asset_id)
        .fetch_one(&deps.pool)
        .await?;
    let video_processor = FfmpegVideoProcessor::new("ffprobe", "ffmpeg", Duration::from_secs(10));

    extract_metadata(
        &deps.pool,
        &deps.storage,
        &RustImageProcessor,
        &video_processor,
        asset_id,
    )
    .await?;
    generate_derivatives(
        &deps.pool,
        &deps.storage,
        &RustImageProcessor,
        &video_processor,
        asset_id,
    )
    .await?;

    let (width, height, duration_ms) = sqlx::query_as::<_, (i32, i32, i64)>(
        r#"
        SELECT width, height, (raw->>'duration_ms')::bigint
        FROM asset_metadata
        WHERE asset_id = $1
        "#,
    )
    .bind(asset_id)
    .fetch_one(&deps.pool)
    .await?;
    let derivatives = sqlx::query_as::<_, (String, i32, i32, String)>(
        r#"
        SELECT kind, width, height, storage_key
        FROM derivatives
        WHERE asset_id = $1
        ORDER BY kind
        "#,
    )
    .bind(asset_id)
    .fetch_all(&deps.pool)
    .await?;

    assert_eq!((width, height), (64, 32));
    assert!(duration_ms > 0);
    assert_eq!(derivatives.len(), 2);
    for (_, derivative_width, derivative_height, storage_key) in derivatives {
        assert_eq!((derivative_width, derivative_height), (64, 32));
        assert!(deps.storage.exists(&StorageKey::new(storage_key)?).await?);
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

    let result = run_media_job(
        &deps.pool,
        &deps.storage,
        &FakeImageProcessor,
        &FakeVideoProcessor,
        &job,
    )
    .await;

    assert!(matches!(result, Err(MediaError::InvalidJobPayload)));

    Ok(())
}

async fn promoted_asset(deps: &StorageTestDeps, filename: &str) -> TestResult<Uuid> {
    let upload_id = create_verified_jpeg_upload(&deps.pool, &deps.storage, filename).await?;
    let promoted = promote_verified_upload(&deps.pool, &deps.storage, 1, upload_id).await?;
    let internal_asset_id = sqlx::query_scalar("SELECT id FROM assets WHERE public_id = $1")
        .bind(promoted.asset_id)
        .fetch_one(&deps.pool)
        .await?;
    Ok(internal_asset_id)
}

fn png_fixture() -> TestResult<Vec<u8>> {
    png_fixture_with_dimensions(64, 32)
}

fn png_fixture_with_dimensions(width: u32, height: u32) -> TestResult<Vec<u8>> {
    let image = image::DynamicImage::ImageRgba8(ImageBuffer::from_pixel(
        width,
        height,
        Rgba([8, 80, 120, 255]),
    ));
    let mut output = Cursor::new(Vec::new());
    image.write_to(&mut output, ImageFormat::Png)?;
    Ok(output.into_inner())
}

fn command_available(program: &str) -> bool {
    Command::new(program)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn jpeg_with_exif_fixture() -> TestResult<Vec<u8>> {
    let image =
        image::DynamicImage::ImageRgba8(ImageBuffer::from_pixel(2, 2, Rgba([8, 80, 120, 255])));
    let mut encoded = Cursor::new(Vec::new());
    image.write_to(&mut encoded, ImageFormat::Jpeg)?;
    let encoded = encoded.into_inner();
    assert_eq!(&encoded[..2], [0xff, 0xd8]);

    let tiff = exif_tiff_fixture();
    let segment_length = u16::try_from(tiff.len() + 8)?;
    let mut jpeg = Vec::with_capacity(encoded.len() + tiff.len() + 10);
    jpeg.extend_from_slice(&encoded[..2]);
    jpeg.extend_from_slice(&[0xff, 0xe1]);
    jpeg.extend_from_slice(&segment_length.to_be_bytes());
    jpeg.extend_from_slice(b"Exif\0\0");
    jpeg.extend_from_slice(&tiff);
    jpeg.extend_from_slice(&encoded[2..]);
    Ok(jpeg)
}

fn exif_tiff_fixture() -> Vec<u8> {
    let mut output = Vec::with_capacity(262);
    output.extend_from_slice(b"II");
    push_tiff_u16(&mut output, 42);
    push_tiff_u32(&mut output, 8);

    push_tiff_u16(&mut output, 4);
    push_tiff_entry(&mut output, 0x010f, 2, 7, 170);
    push_tiff_entry(&mut output, 0x0110, 2, 9, 177);
    push_tiff_entry(&mut output, 0x8769, 4, 1, 62);
    push_tiff_entry(&mut output, 0x8825, 4, 1, 80);
    push_tiff_u32(&mut output, 0);

    push_tiff_u16(&mut output, 1);
    push_tiff_entry(&mut output, 0x9003, 2, 20, 186);
    push_tiff_u32(&mut output, 0);

    push_tiff_u16(&mut output, 7);
    push_tiff_entry(&mut output, 0x0000, 1, 4, 0x0000_0302);
    push_tiff_entry(&mut output, 0x0001, 2, 2, u32::from(b'N'));
    push_tiff_entry(&mut output, 0x0002, 5, 3, 206);
    push_tiff_entry(&mut output, 0x0003, 2, 2, u32::from(b'E'));
    push_tiff_entry(&mut output, 0x0004, 5, 3, 230);
    push_tiff_entry(&mut output, 0x0005, 1, 1, 0);
    push_tiff_entry(&mut output, 0x0006, 5, 1, 254);
    push_tiff_u32(&mut output, 0);

    assert_eq!(output.len(), 170);
    output.extend_from_slice(b"Mirror\0");
    output.extend_from_slice(b"VaultCam\0");
    output.extend_from_slice(b"2026:06:15 12:00:00\0");
    for value in [55, 45, 0, 37, 37, 0, 156] {
        push_tiff_rational(&mut output, value, 1);
    }
    assert_eq!(output.len(), 262);
    output
}

fn push_tiff_entry(output: &mut Vec<u8>, tag: u16, format: u16, count: u32, value: u32) {
    push_tiff_u16(output, tag);
    push_tiff_u16(output, format);
    push_tiff_u32(output, count);
    push_tiff_u32(output, value);
}

fn push_tiff_rational(output: &mut Vec<u8>, numerator: u32, denominator: u32) {
    push_tiff_u32(output, numerator);
    push_tiff_u32(output, denominator);
}

fn push_tiff_u16(output: &mut Vec<u8>, value: u16) {
    output.extend_from_slice(&value.to_le_bytes());
}

fn push_tiff_u32(output: &mut Vec<u8>, value: u32) {
    output.extend_from_slice(&value.to_le_bytes());
}
