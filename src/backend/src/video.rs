//! Timeout-bounded external video inspection and poster generation.
//!
//! C005: commands receive generated private paths as direct arguments, never
//! shell strings. Child output and generated poster bytes are bounded.

use std::{
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};
use thiserror::Error;
use wait_timeout::ChildExt;

use serde::Deserialize;

const MAX_PROBE_OUTPUT_BYTES: u64 = 64 * 1024;
const MAX_POSTER_BYTES: u64 = 32 * 1024 * 1024;
const PROCESS_POLL_INTERVAL: Duration = Duration::from_millis(10);

/// Video dimensions and duration reported by `ffprobe`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VideoInfo {
    /// Display width of first video stream.
    pub width: u32,
    /// Display height of first video stream.
    pub height: u32,
    /// Container duration in milliseconds when available.
    pub duration_ms: Option<i64>,
}

/// External video processor boundary.
pub trait VideoProcessor: Clone + Send + Sync + 'static {
    /// Probes first video stream and container duration.
    fn inspect(&self, input: &Path) -> Result<VideoInfo, VideoToolError>;
    /// Generates one metadata-free WebP poster bounded by `max_edge`.
    fn generate_poster(&self, input: &Path, max_edge: u32) -> Result<Vec<u8>, VideoToolError>;
}

/// `ffprobe`/`ffmpeg` implementation used by media workers.
#[derive(Debug, Clone)]
pub struct FfmpegVideoProcessor {
    ffprobe_path: PathBuf,
    ffmpeg_path: PathBuf,
    command_timeout: Duration,
}

impl FfmpegVideoProcessor {
    /// Creates a processor with explicit executable paths and command timeout.
    #[must_use]
    pub fn new(
        ffprobe_path: impl Into<PathBuf>,
        ffmpeg_path: impl Into<PathBuf>,
        command_timeout: Duration,
    ) -> Self {
        Self {
            ffprobe_path: ffprobe_path.into(),
            ffmpeg_path: ffmpeg_path.into(),
            command_timeout,
        }
    }

    /// Production defaults resolved through worker `PATH`.
    #[must_use]
    pub fn production() -> Self {
        Self::new("ffprobe", "ffmpeg", Duration::from_secs(60))
    }
}

impl VideoProcessor for FfmpegVideoProcessor {
    fn inspect(&self, input: &Path) -> Result<VideoInfo, VideoToolError> {
        let mut command = Command::new(&self.ffprobe_path);
        command.args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=width,height:stream_tags=rotate:stream_side_data=rotation:format=duration",
            "-of",
            "json",
        ]);
        command.arg(input);
        let output = run_command(&mut command, self.command_timeout, true)?;
        let probe: ProbeOutput =
            serde_json::from_slice(&output).map_err(VideoToolError::InvalidProbeJson)?;
        let stream = probe
            .streams
            .first()
            .ok_or(VideoToolError::MissingVideoStream)?;
        let mut width = stream
            .width
            .filter(|value| *value > 0)
            .ok_or(VideoToolError::InvalidVideoDimensions)?;
        let mut height = stream
            .height
            .filter(|value| *value > 0)
            .ok_or(VideoToolError::InvalidVideoDimensions)?;
        let rotation = stream
            .side_data_list
            .iter()
            .find_map(|side_data| side_data.rotation)
            .or_else(|| {
                stream
                    .tags
                    .as_ref()
                    .and_then(|tags| tags.rotate.as_deref())
                    .and_then(|value| value.parse::<i32>().ok())
            })
            .unwrap_or(0);
        if rotation.rem_euclid(180) == 90 {
            std::mem::swap(&mut width, &mut height);
        }
        let duration_ms = probe
            .format
            .and_then(|format| format.duration)
            .map(|duration| parse_duration_ms(&duration))
            .transpose()?;

        Ok(VideoInfo {
            width,
            height,
            duration_ms,
        })
    }

    fn generate_poster(&self, input: &Path, max_edge: u32) -> Result<Vec<u8>, VideoToolError> {
        if max_edge == 0 {
            return Err(VideoToolError::InvalidPosterEdge);
        }
        let info = self.inspect(input)?;
        let target_edge = max_edge.min(info.width.max(info.height));
        let seek_ms = info
            .duration_ms
            .map(|duration| (duration / 10).min(3_000))
            .unwrap_or(0);
        let seek_seconds = format!("{:.3}", seek_ms as f64 / 1000.0);
        let output_dir = input
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let output = tempfile::Builder::new()
            .prefix("poster-")
            .suffix(".webp")
            .tempfile_in(output_dir)?
            .into_temp_path();
        let output_path = output.to_path_buf();
        let filter = format!(
            "scale=w='if(gte(iw,ih),min(iw,{target_edge}),-2)':h='if(gte(iw,ih),-2,min(ih,{target_edge}))'"
        );
        let mut command = Command::new(&self.ffmpeg_path);
        command.args([
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-ss",
        ]);
        command.arg(seek_seconds);
        command.arg("-i");
        command.arg(input);
        command.args([
            "-map",
            "0:v:0",
            "-frames:v",
            "1",
            "-an",
            "-sn",
            "-dn",
            "-vf",
        ]);
        command.arg(filter);
        command.arg(&output_path);
        run_command(&mut command, self.command_timeout, false)?;

        let size = std::fs::metadata(&output_path)?.len();
        if size == 0 || size > MAX_POSTER_BYTES {
            return Err(VideoToolError::PosterSizeOutOfRange);
        }
        std::fs::read(&output_path).map_err(From::from)
    }
}

#[derive(Debug, Deserialize)]
struct ProbeOutput {
    #[serde(default)]
    streams: Vec<ProbeStream>,
    format: Option<ProbeFormat>,
}

#[derive(Debug, Deserialize)]
struct ProbeStream {
    width: Option<u32>,
    height: Option<u32>,
    tags: Option<ProbeTags>,
    #[serde(default)]
    side_data_list: Vec<ProbeSideData>,
}

#[derive(Debug, Deserialize)]
struct ProbeTags {
    rotate: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ProbeSideData {
    rotation: Option<i32>,
}

#[derive(Debug, Deserialize)]
struct ProbeFormat {
    duration: Option<String>,
}

fn parse_duration_ms(value: &str) -> Result<i64, VideoToolError> {
    let seconds = value
        .parse::<f64>()
        .map_err(|_| VideoToolError::InvalidDuration)?;
    if !seconds.is_finite() || seconds < 0.0 {
        return Err(VideoToolError::InvalidDuration);
    }
    let milliseconds = seconds * 1000.0;
    if milliseconds > i64::MAX as f64 {
        return Err(VideoToolError::InvalidDuration);
    }
    Ok(milliseconds.round() as i64)
}

fn run_command(
    command: &mut Command,
    timeout: Duration,
    capture_stdout: bool,
) -> Result<Vec<u8>, VideoToolError> {
    if timeout.is_zero() {
        return Err(VideoToolError::InvalidTimeout);
    }
    command.stdin(Stdio::null()).stderr(Stdio::null());
    if capture_stdout {
        command.stdout(Stdio::piped());
    } else {
        command.stdout(Stdio::null());
    }
    let mut child = command.spawn().map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            VideoToolError::ToolUnavailable
        } else {
            VideoToolError::Io(error)
        }
    })?;
    let mut stdout = if capture_stdout {
        let stdout = child.stdout.take().ok_or(VideoToolError::MissingStdout)?;
        Some(spawn_stdout_reader(stdout))
    } else {
        None
    };
    let started = Instant::now();
    let mut stdout_result = None;

    let status = loop {
        if let Some(stdout_reader) = stdout.as_ref()
            && stdout_result.is_none()
        {
            match stdout_reader.result.try_recv() {
                Ok(result) => {
                    if matches!(result, Err(VideoToolError::OutputTooLarge)) {
                        if child.try_wait()?.is_none() {
                            child.kill()?;
                            child.wait()?;
                        }
                        if let Some(stdout) = stdout.take() {
                            stdout.join()?;
                        }
                        return result;
                    }
                    stdout_result = Some(result);
                }
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => {
                    return Err(VideoToolError::Io(std::io::Error::other(
                        "video stdout reader stopped",
                    )));
                }
            }
        }

        if started.elapsed() >= timeout {
            if child.try_wait()?.is_none() {
                child.kill()?;
                child.wait()?;
            }
            if let Some(stdout) = stdout {
                stdout.join()?;
            }
            return Err(VideoToolError::TimedOut);
        }
        let wait_for = PROCESS_POLL_INTERVAL.min(timeout.saturating_sub(started.elapsed()));
        if let Some(status) = child.wait_timeout(wait_for)? {
            break status;
        }
    };
    if !capture_stdout {
        if !status.success() {
            return Err(VideoToolError::CommandFailed);
        }
        return Ok(Vec::new());
    }

    let stdout = stdout.ok_or(VideoToolError::MissingStdout)?;
    let output = match stdout_result {
        Some(result) => result,
        None => stdout.result.recv().map_err(|_| {
            VideoToolError::Io(std::io::Error::other("video stdout reader stopped"))
        })?,
    };
    stdout.join()?;
    let output = output?;
    if !status.success() {
        return Err(VideoToolError::CommandFailed);
    }
    Ok(output)
}

struct StdoutReader {
    result: mpsc::Receiver<Result<Vec<u8>, VideoToolError>>,
    thread: thread::JoinHandle<()>,
}

impl StdoutReader {
    fn join(self) -> Result<(), VideoToolError> {
        self.thread
            .join()
            .map_err(|_| VideoToolError::Io(std::io::Error::other("video stdout reader panicked")))
    }
}

fn spawn_stdout_reader(stdout: impl Read + Send + 'static) -> StdoutReader {
    let (sender, result) = mpsc::channel();
    let thread = thread::spawn(move || {
        let _ = sender.send(read_bounded(stdout, MAX_PROBE_OUTPUT_BYTES));
    });
    StdoutReader { result, thread }
}

fn read_bounded(mut file: impl Read, max_bytes: u64) -> Result<Vec<u8>, VideoToolError> {
    let mut output = Vec::new();
    file.by_ref().take(max_bytes + 1).read_to_end(&mut output)?;
    if u64::try_from(output.len()).map_err(|_| VideoToolError::OutputTooLarge)? > max_bytes {
        return Err(VideoToolError::OutputTooLarge);
    }
    Ok(output)
}

/// External video processing failure.
#[derive(Debug, Error)]
pub enum VideoToolError {
    /// Executable or generated-file I/O failed.
    #[error("video tool I/O failed")]
    Io(#[from] std::io::Error),
    /// Configured video executable was not found.
    #[error("video tool executable unavailable")]
    ToolUnavailable,
    /// Child exceeded configured deadline and was killed.
    #[error("video tool timed out")]
    TimedOut,
    /// Child exited unsuccessfully.
    #[error("video tool command failed")]
    CommandFailed,
    /// Probe stdout pipe was unexpectedly unavailable.
    #[error("video tool stdout unavailable")]
    MissingStdout,
    /// Probe output exceeded its fixed bound.
    #[error("video tool output exceeds limit")]
    OutputTooLarge,
    /// Probe JSON did not match expected structure.
    #[error("video probe returned invalid JSON")]
    InvalidProbeJson(#[from] serde_json::Error),
    /// No video stream was present.
    #[error("media has no video stream")]
    MissingVideoStream,
    /// Video dimensions were absent or zero.
    #[error("video dimensions are invalid")]
    InvalidVideoDimensions,
    /// Duration was negative, non-finite, or out of range.
    #[error("video duration is invalid")]
    InvalidDuration,
    /// Poster edge must be positive.
    #[error("video poster edge is invalid")]
    InvalidPosterEdge,
    /// Generated poster was empty or exceeded its fixed bound.
    #[error("video poster size is invalid")]
    PosterSizeOutOfRange,
    /// Command timeout must be positive.
    #[error("video command timeout is invalid")]
    InvalidTimeout,
}
