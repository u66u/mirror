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
    create_video_fixture(&input)?;
    let processor = FfmpegVideoProcessor::new("ffprobe", "ffmpeg", Duration::from_secs(10));

    let info = processor.inspect(&input)?;
    let poster = processor.generate_poster(&input, 512)?;
    let poster_info = RustImageProcessor.inspect(&poster, "image/webp")?;

    assert_eq!((info.width, info.height), (64, 32));
    assert!(info.duration_ms.is_some_and(|duration| duration > 0));
    assert_eq!((poster_info.width, poster_info.height), (64, 32));
    assert!(poster.len() < 32 * 1024 * 1024);
    Ok(())
}

#[cfg(unix)]
#[test]
fn video_processor_kills_probe_that_exceeds_deadline() -> TestResult {
    let temp_dir = TempDir::new()?;
    let probe = temp_dir.path().join("slow-probe");
    write_executable_script(&probe, "#!/bin/sh\nexec sleep 1\n")?;
    let processor = FfmpegVideoProcessor::new(&probe, "ffmpeg", Duration::from_millis(20));

    let result = processor.inspect(Path::new("unused-input"));

    assert!(matches!(result, Err(VideoToolError::TimedOut)));
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
