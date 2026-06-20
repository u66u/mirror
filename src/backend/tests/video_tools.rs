use std::{path::Path, time::Duration};

use mirror_backend::{
    media::{ImageProcessor, RustImageProcessor},
    video::{FfmpegVideoProcessor, VideoProcessor, VideoToolError},
};
use tempfile::TempDir;

mod support;
use support::{TestResult, create_video_fixture, ffmpeg_is_available, write_executable_script};

#[test]
fn ffmpeg_video_processor_probes_and_generates_bounded_poster() -> TestResult {
    if !ffmpeg_is_available() {
        return Ok(());
    }
    let temp_dir = TempDir::new()?;
    let input = temp_dir.path().join("fixture.mp4");
    let extensionless_input = temp_dir.path().join("fixture");
    create_video_fixture(&input)?;
    std::fs::copy(&input, &extensionless_input)?;
    let processor = FfmpegVideoProcessor::new("ffprobe", "ffmpeg", Duration::from_secs(10));

    let info = processor.inspect(&input)?;
    let poster = processor.generate_poster(&input, 512)?;
    let large_edge_poster = processor.generate_poster(&input, 2048)?;
    let extensionless_poster = processor.generate_poster(&extensionless_input, 2048)?;
    let poster_info = RustImageProcessor.inspect(&poster, "image/webp")?;
    let large_edge_poster_info = RustImageProcessor.inspect(&large_edge_poster, "image/webp")?;
    let extensionless_poster_info =
        RustImageProcessor.inspect(&extensionless_poster, "image/webp")?;

    assert_eq!((info.width, info.height), (64, 32));
    assert!(info.duration_ms.is_some_and(|duration| duration > 0));
    assert_eq!((poster_info.width, poster_info.height), (64, 32));
    assert_eq!(
        (large_edge_poster_info.width, large_edge_poster_info.height),
        (64, 32)
    );
    assert_eq!(
        (
            extensionless_poster_info.width,
            extensionless_poster_info.height
        ),
        (64, 32)
    );
    assert!(poster.len() < 32 * 1024 * 1024);
    Ok(())
}

#[cfg(unix)]
#[test]
fn video_processor_kills_probe_that_exceeds_deadline() -> TestResult {
    let temp_dir = TempDir::new()?;
    let probe = temp_dir.path().join("slow-probe");
    write_executable_script(&probe, "#!/bin/sh\nexec sleep 60\n")?;
    let processor = FfmpegVideoProcessor::new(&probe, "ffmpeg", Duration::from_millis(100));

    let result = processor.inspect(Path::new("unused-input"));

    assert!(
        matches!(result, Err(VideoToolError::TimedOut)),
        "expected timeout, got {result:?}"
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn video_probe_does_not_deadlock_when_stdout_exceeds_limit() -> TestResult {
    let temp_dir = TempDir::new()?;
    let probe = temp_dir.path().join("noisy-probe");
    write_executable_script(
        &probe,
        "#!/bin/sh\ndd if=/dev/zero bs=1024 count=128 2>/dev/null\nsleep 5\n",
    )?;
    let processor = FfmpegVideoProcessor::new(&probe, "ffmpeg", Duration::from_secs(5));

    let result = processor.inspect(Path::new("unused-input"));

    assert!(matches!(result, Err(VideoToolError::OutputTooLarge)));
    Ok(())
}

#[cfg(unix)]
#[test]
fn video_processor_deletes_bad_poster_output() -> TestResult {
    let temp_dir = TempDir::new()?;
    let probe = temp_dir.path().join("probe");
    let ffmpeg = temp_dir.path().join("ffmpeg");
    let recorded_output = temp_dir.path().join("poster-path");
    write_executable_script(
        &probe,
        r#"#!/bin/sh
printf '%s' '{"streams":[{"width":64,"height":32}],"format":{"duration":"1.0"}}'
"#,
    )?;
    write_executable_script(
        &ffmpeg,
        &format!(
            r#"#!/bin/sh
last=
for arg do
    last="$arg"
done
printf '%s' "$last" > '{}'
: > "$last"
"#,
            recorded_output.display()
        ),
    )?;
    let input = temp_dir.path().join("input.mp4");
    let processor = FfmpegVideoProcessor::new(&probe, &ffmpeg, Duration::from_secs(1));

    let result = processor.generate_poster(&input, 512);

    assert!(
        matches!(result, Err(VideoToolError::PosterSizeOutOfRange)),
        "expected poster size error, got {result:?}"
    );
    let poster_path = std::fs::read_to_string(recorded_output)?;
    assert!(!Path::new(&poster_path).exists());
    Ok(())
}

#[cfg(unix)]
#[test]
fn video_probe_reports_display_dimensions_after_rotation() -> TestResult {
    let temp_dir = TempDir::new()?;
    let probe = temp_dir.path().join("rotated-probe");
    write_executable_script(
        &probe,
        r#"#!/bin/sh
printf '%s' '{"streams":[{"width":1920,"height":1080,"side_data_list":[{"rotation":90}]}],"format":{"duration":"1.5"}}'
"#,
    )?;
    let processor = FfmpegVideoProcessor::new(&probe, "ffmpeg", Duration::from_secs(1));

    let info = processor.inspect(Path::new("unused-input"))?;

    assert_eq!((info.width, info.height), (1080, 1920));
    assert_eq!(info.duration_ms, Some(1500));
    Ok(())
}
