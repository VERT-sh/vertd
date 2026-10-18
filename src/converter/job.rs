use log::warn;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;
use tokio::process::Command;
use tokio::sync::{Mutex, OnceCell};
use uuid::Uuid;

use super::sequence;

pub const FFPROBE_TIMEOUT: Duration = Duration::from_secs(60);
pub const MAX_PROBE_ATTEMPTS: u8 = 3;
pub type LogBuffer = Arc<Mutex<VecDeque<LogLine>>>;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogLine {
    pub message: String,
    pub repeats: usize,
}

pub fn new_log_buffer() -> LogBuffer {
    Arc::new(Mutex::new(VecDeque::new()))
}

pub async fn push_log(buffer: &LogBuffer, line: String, repeats: usize) {
    let mut logs = buffer.lock().await;
    if line.trim().is_empty() {
        return;
    }

    if let Some(last) = logs.back_mut() {
        if last.message == line {
            last.repeats += repeats;
            return;
        }
    }

    if logs.len() >= 200 {
        logs.pop_front();
    }
    logs.push_back(LogLine {
        message: line,
        repeats: repeats.max(1),
    });
}

pub async fn snapshot_logs(buffer: &LogBuffer) -> Vec<LogLine> {
    buffer.lock().await.iter().cloned().collect()
}

pub fn format_job_logs(logs: &[LogLine]) -> String {
    if logs.is_empty() {
        return "No error logs.".to_string();
    }

    let mut message = String::new();
    for (index, line) in logs.iter().enumerate() {
        if index > 0 {
            message.push('\n');
        }
        message.push_str(line.message.trim_end());
        if line.repeats > 1 {
            message.push_str(&format!(" [x{}]", line.repeats));
        }
    }
    message
}

async fn run_ffprobe(args: &[&str]) -> anyhow::Result<std::process::Output> {
    let child = Command::new("ffprobe")
        .args(args)
        .kill_on_drop(true)
        .stdin(std::process::Stdio::null())
        .output();

    match tokio::time::timeout(FFPROBE_TIMEOUT, child).await {
        Ok(result) => Ok(result?),
        Err(_) => Err(anyhow::anyhow!(
            "ffprobe timed out after {}s",
            FFPROBE_TIMEOUT.as_secs()
        )),
    }
}

fn validate_ffprobe_output(
    output: &std::process::Output,
    path: &str,
    context: &str,
) -> anyhow::Result<()> {
    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    Err(anyhow::anyhow!(
        "ffprobe failed while {} for {}: {}",
        context,
        path,
        stderr.trim()
    ))
}

pub async fn has_video_stream(path: &str) -> anyhow::Result<bool> {
    let output = run_ffprobe(&[
        "-v",
        "error",
        "-select_streams",
        "v",
        "-show_entries",
        "stream=index",
        "-of",
        "csv=p=0",
        path,
    ])
    .await?;

    validate_ffprobe_output(&output, path, "checking the output for a video stream")?;
    Ok(!String::from_utf8_lossy(&output.stdout).trim().is_empty())
}

// most likely pixel format for each codec, only used if ffprobe can't report one (e.g. an unparseable streams).
fn default_pix_fmt_for_codec(codec: &str) -> &'static str {
    match codec.to_lowercase().as_str() {
        "h264" | "hevc" | "vp8" | "vp9" | "av1" | "mpeg2video" | "mpeg1video" | "mpeg4"
        | "msmpeg4v3" | "msmpeg4v2" | "h263" | "flv1" | "wmv1" | "wmv2" | "wmv3" | "theora"
        | "rv30" | "rv40" | "h261" | "amv" | "dnxhd" => "yuv420p",
        "mjpeg" | "jpeg2000" | "svq1" | "svq3" => "yuvj420p",
        "prores" => "yuv422p10le",
        _ => "yuv420p",
    }
}

// parse fps which could be in the form of "30", "29.97", or "30000/1001"
fn parse_fps(value: &str) -> Option<u32> {
    let value = value.trim();
    if let Some((numerator, denominator)) = value.split_once('/') {
        match (
            numerator.trim().parse::<f64>(),
            denominator.trim().parse::<f64>(),
        ) {
            (Ok(numerator), Ok(denominator)) if denominator != 0.0 => {
                Some((numerator / denominator).round() as u32)
            }
            _ => None,
        }
    } else {
        value.parse::<f64>().ok().map(|fps| fps.round() as u32)
    }
}

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct MediaSequence {
    pub kind: sequence::SequenceKind,
    pub extension: String,
    pub entry: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    pub id: Uuid,
    pub auth: String,
    pub from: String,
    pub to: Option<String>,
    pub state: JobState,
    #[serde(default)]
    pub sequence: Option<MediaSequence>,
    total_frames: Option<u64>,
    bitrate: Option<u64>,
    fps: Option<u32>,
    // how many times probing has been widened
    #[serde(default)]
    probe_attempt: u8,
    #[serde(skip)]
    video: OnceCell<VideoMetadata>,
    #[serde(skip)]
    audio: OnceCell<Vec<AudioStream>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct VideoProbe {
    #[serde(default)]
    index: u32,
    #[serde(default)]
    codec_name: String,
    #[serde(default)]
    width: u32,
    #[serde(default)]
    height: u32,
    #[serde(default)]
    pix_fmt: String,
    #[serde(default)]
    r_frame_rate: String,
    #[serde(default)]
    bit_rate: Option<String>,
    #[serde(default)]
    disposition: StreamDisposition,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct StreamDisposition {
    #[serde(default)]
    pub attached_pic: u32,
}

impl VideoProbe {
    fn is_attached_picture(&self) -> bool {
        self.disposition.attached_pic != 0
    }

    fn has_usable_video(&self) -> bool {
        !self.is_attached_picture()
            && !self.codec_name.is_empty()
            && self.width > 0
            && self.height > 0
            && !self.pix_fmt.is_empty()
            && self.pix_fmt != "unknown"
    }
}

#[derive(Deserialize)]
struct Streams<T> {
    streams: Vec<T>,
    #[serde(default)]
    format: Option<FormatInfo>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct FormatInfo {
    #[serde(default)]
    pub duration: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct VideoMetadata {
    streams: Vec<VideoProbe>,
    duration_secs: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AudioStream {
    pub index: u32,
    #[serde(default)]
    pub codec_name: String,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Debug)]
pub enum JobState {
    Processing,
    Completed,
    Failed,
}

impl Job {
    pub fn new(id: Uuid, auth_token: String, from: String) -> Self {
        Self {
            id,
            auth: auth_token,
            from,
            to: None,
            state: JobState::Processing,
            sequence: None,
            total_frames: None,
            bitrate: None,
            fps: None,
            probe_attempt: 0,
            video: OnceCell::new(),
            audio: OnceCell::new(),
        }
    }

    pub fn new_sequence(
        id: Uuid,
        auth_token: String,
        prepared: sequence::PreparedSequence,
    ) -> Self {
        Self {
            id,
            auth: auth_token,
            from: prepared.extension.clone(),
            to: None,
            state: JobState::Processing,
            sequence: Some(MediaSequence {
                kind: prepared.kind,
                extension: prepared.extension,
                entry: prepared.entry,
            }),
            total_frames: prepared.frame_count, // image sequences only
            bitrate: None,
            fps: None,
            probe_attempt: 0,
            video: OnceCell::new(),
            audio: OnceCell::new(),
        }
    }

    pub fn is_sequence(&self) -> bool {
        self.sequence.is_some()
    }

    pub fn sequence_kind(&self) -> Option<sequence::SequenceKind> {
        self.sequence.as_ref().map(|sequence| sequence.kind)
    }

    pub fn input_path(&self) -> String {
        match &self.sequence {
            Some(sequence) => {
                // ffmpeg reads the sibling files from the process cwd, so give it an absolute path
                let relative = format!("input/{}/{}", self.id, sequence.entry);
                std::fs::canonicalize(&relative)
                    .map(|path| path.display().to_string())
                    .unwrap_or(relative)
            }
            None => format!("input/{}.{}", self.id, self.from),
        }
    }

    pub fn input_dir(&self) -> String {
        format!("input/{}", self.id)
    }

    fn sequence_input_args(&self) -> &'static [&'static str] {
        if self.sequence_kind() == Some(sequence::SequenceKind::FrameSequence) {
            &["-f", "concat", "-safe", "0"]
        } else {
            &[]
        }
    }

    fn probe_input_args(&self) -> Vec<String> {
        if self.probe_attempt == 0 {
            return Vec::new();
        }

        let scale = u64::from(self.probe_attempt);
        let probesize = 5_000_000u64.saturating_mul(scale);
        let analyzeduration = 5_000_000u64.saturating_mul(scale);

        vec![
            "-probesize".to_string(),
            probesize.to_string(),
            "-analyzeduration".to_string(),
            analyzeduration.to_string(),
        ]
    }

    pub fn can_widen_probe(&self) -> bool {
        self.probe_attempt < MAX_PROBE_ATTEMPTS
    }

    pub fn probe_attempts(&self) -> u8 {
        self.probe_attempt
    }

    pub fn widen_probe(&mut self) {
        self.probe_attempt = self.probe_attempt.saturating_add(1);
        self.video = OnceCell::new();
        self.audio = OnceCell::new();
        self.total_frames = None;
        self.bitrate = None;
        self.fps = None;
        log::warn!(
            "job {} re-probing with a wider analysis window (attempt {})",
            self.id,
            self.probe_attempt
        );
    }

    pub fn completed(&self) -> bool {
        self.state == JobState::Completed
    }

    pub fn errored(&self) -> bool {
        self.state == JobState::Failed
    }

    pub fn processing(&self) -> bool {
        self.state == JobState::Processing
    }

    pub fn try_start(&mut self, target: String) -> bool {
        if self.to.is_some() || self.completed() {
            return false;
        }
        self.to = Some(target);
        self.state = JobState::Processing;
        true
    }

    pub async fn bitrate(&mut self) -> anyhow::Result<u64> {
        if let Some(bitrate) = self.bitrate {
            return Ok(bitrate);
        }

        let probe = self.primary_video_stream().await?;
        let (width, height) = (probe.width, probe.height);
        let default_bitrate = match (width, height) {
            (w, h) if w >= 3840 || h >= 2160 => 30_000_000, // >4K - 30 Mbps
            (w, h) if w >= 2560 || h >= 1440 => 14_000_000, // >2K - 14 Mbps
            (w, h) if w >= 1920 || h >= 1080 => 7_000_000,  // >1080p - 7 Mbps
            (w, h) if w >= 1280 || h >= 720 => 4_000_000,   // >720p - 4 Mbps
            _ => 1_500_000,                                 // <SD - 1.5 Mbps
        };

        let bitrate_value = probe
            .bit_rate
            .as_deref()
            .and_then(|b| b.trim().parse::<u64>().ok());
        if let Some(bitrate_value) = bitrate_value {
            let bitrate = bitrate_value.min(default_bitrate);
            self.bitrate = Some(bitrate);

            if bitrate_value > default_bitrate {
                warn!(
                    "detected bitrate {} exceeds default for resolution {}x{}, using default {}",
                    bitrate_value, width, height, default_bitrate
                );
            }
            return Ok(bitrate);
        }

        self.bitrate = Some(default_bitrate);
        Ok(default_bitrate)
    }

    pub async fn total_frames(&mut self) -> anyhow::Result<u64> {
        if let Some(total_frames) = self.total_frames {
            return Ok(total_frames);
        }

        let duration_secs = self.video_metadata().await?.duration_secs;
        let fps = self.fps().await?;
        let total_frames = duration_secs
            .map(|duration| (duration * f64::from(fps)).round() as u64)
            .unwrap_or(0);
        self.total_frames = Some(total_frames);
        Ok(total_frames)
    }

    pub async fn fps(&mut self) -> anyhow::Result<u32> {
        if let Some(fps) = self.fps {
            return Ok(fps);
        }

        let probe = self.primary_video_stream().await?;
        let fps_trim = probe.r_frame_rate.trim();

        if fps_trim.is_empty() {
            warn!("ffprobe returned empty fps for job {}", self.id);
            let default = 30u32;
            self.fps = Some(default);
            return Ok(default);
        }

        let result = parse_fps(fps_trim).unwrap_or_else(|| {
            warn!("failed to parse fps '{}' from ffprobe", fps_trim);
            30u32
        });

        self.fps = Some(result);
        Ok(result)
    }

    pub async fn resolution(&self) -> anyhow::Result<(u32, u32)> {
        let probe = self.primary_video_stream().await?;
        if probe.width == 0 || probe.height == 0 {
            anyhow::bail!(
                "failed to get resolution from ffprobe output for job {}",
                self.id
            );
        }
        Ok((probe.width, probe.height))
    }

    pub async fn pix_fmt(&self) -> anyhow::Result<String> {
        let probe = self.primary_video_stream().await?;

        if probe.pix_fmt.trim().is_empty() || probe.pix_fmt == "unknown" {
            let fallback = default_pix_fmt_for_codec(&probe.codec_name);
            warn!(
                "ffprobe reported no pixel format for job {} (codec {}), assuming {}",
                self.id, probe.codec_name, fallback
            );
            return Ok(fallback.to_string());
        }

        Ok(probe.pix_fmt.clone())
    }

    // all mapped video streams (except attached pictures / unusable video) in order, falling back to the first stream
    pub async fn video_stream_indices(&self) -> anyhow::Result<Vec<u32>> {
        let streams = &self.video_metadata().await?.streams;
        let indices: Vec<u32> = streams
            .iter()
            .filter(|probe| probe.has_usable_video())
            .map(|probe| probe.index)
            .collect();

        if indices.is_empty() {
            return Ok(vec![streams.first().map_or(0, |probe| probe.index)]);
        }

        Ok(indices)
    }

    // first non-attached-picture video stream, falling back to the first stream
    async fn primary_video_stream(&self) -> anyhow::Result<&VideoProbe> {
        let streams = &self.video_metadata().await?.streams;
        let primary = streams
            .iter()
            .find(|probe| probe.has_usable_video())
            .or_else(|| streams.iter().find(|probe| !probe.is_attached_picture()))
            .or_else(|| streams.first())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "no video stream found in ffprobe output for job {}",
                    self.id
                )
            })?;
        Ok(primary)
    }

    async fn video_metadata(&self) -> anyhow::Result<&VideoMetadata> {
        self.video
            .get_or_try_init(|| async {
                let path = self.input_path();
                let mut args = vec!["-v", "error"];
                let probe_args = self.probe_input_args();
                args.extend(probe_args.iter().map(String::as_str));
                args.extend_from_slice(self.sequence_input_args());
                args.extend_from_slice(&[
                    "-select_streams",
                    "v",
                    "-show_entries",
                    "stream=index,codec_name,width,height,pix_fmt,r_frame_rate,bit_rate:stream_disposition=attached_pic:format=duration",
                    "-of",
                    "json",
                    path.as_str(),
                ]);
                let output = run_ffprobe(&args).await?;

                validate_ffprobe_output(&output, &path, "reading video metadata")?;
                let probe: Streams<VideoProbe> =
                    serde_json::from_slice(&output.stdout).map_err(|error| {
                        anyhow::anyhow!("invalid video metadata for {}: {}", path, error)
                    })?;
                let duration_secs = probe
                    .format
                    .and_then(|format| format.duration)
                    .and_then(|duration| duration.trim().parse::<f64>().ok())
                    .filter(|duration| *duration > 0.0);
                Ok(VideoMetadata {
                    streams: probe.streams,
                    duration_secs,
                })
            })
            .await
    }

    pub async fn audio_streams(&self) -> anyhow::Result<Vec<AudioStream>> {
        Ok(self.audio_streams_probe().await?.clone())
    }

    async fn audio_streams_probe(&self) -> anyhow::Result<&Vec<AudioStream>> {
        self.audio
            .get_or_try_init(|| async {
                let path = self.input_path();
                let mut args = vec!["-v", "error"];
                let probe_args = self.probe_input_args();
                args.extend(probe_args.iter().map(String::as_str));
                args.extend_from_slice(self.sequence_input_args());
                args.extend_from_slice(&[
                    "-select_streams",
                    "a",
                    "-show_entries",
                    "stream=index,codec_name",
                    "-of",
                    "json",
                    path.as_str(),
                ]);
                let output = run_ffprobe(&args).await?;
                validate_ffprobe_output(&output, &path, "reading all audio streams")?;
                let probe: Streams<AudioStream> =
                    serde_json::from_slice(&output.stdout).map_err(|error| {
                        anyhow::anyhow!("invalid audio stream metadata for {}: {}", path, error)
                    })?;
                Ok(probe.streams)
            })
            .await
    }

    // codecs.0 = video codec, codecs.1 = audio codec
    pub async fn codecs(&self) -> anyhow::Result<(String, String)> {
        let video_codec = self.primary_video_stream().await?.codec_name.to_lowercase();
        let video_codec = if video_codec.is_empty() {
            "none".to_string()
        } else {
            video_codec
        };

        let audio_codec = self
            .audio_streams_probe()
            .await?
            .first()
            .map_or_else(|| "none".to_string(), |stream| stream.codec_name.clone());

        Ok((video_codec, audio_codec))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "camelCase")]
pub enum ProgressUpdate {
    #[serde(rename = "frame", rename_all = "camelCase")]
    Frame(u64),
    #[serde(rename = "fps", rename_all = "camelCase")]
    FPS(f64),
    #[serde(rename = "error", rename_all = "camelCase")]
    Error(LogLine),
}
