use log::warn;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;
use tokio::process::Command;
use tokio::sync::{Mutex, OnceCell};
use uuid::Uuid;

pub const FFPROBE_TIMEOUT: Duration = Duration::from_secs(60);

pub type LogBuffer = Arc<Mutex<VecDeque<String>>>;

pub fn new_log_buffer() -> LogBuffer {
    Arc::new(Mutex::new(VecDeque::new()))
}

pub async fn push_log(buffer: &LogBuffer, line: String) {
    let mut logs = buffer.lock().await;
    if logs.len() >= 200 {
        logs.pop_front();
    }
    logs.push_back(line);
}

pub async fn snapshot_logs(buffer: &LogBuffer) -> Vec<String> {
    buffer.lock().await.iter().cloned().collect()
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

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    pub id: Uuid,
    pub auth: String,
    pub from: String,
    pub to: Option<String>,
    pub state: JobState,
    total_frames: Option<u64>,
    bitrate: Option<u64>,
    fps: Option<u32>,
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
    pub fn new(auth_token: String, from: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            auth: auth_token,
            from,
            to: None,
            state: JobState::Processing,
            total_frames: None,
            bitrate: None,
            fps: None,
            video: OnceCell::new(),
            audio: OnceCell::new(),
        }
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

    pub fn reserved(&self) -> bool {
        self.to.is_some() && !self.completed() && !self.errored()
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
        let total_frames = duration_secs
            .map(|duration| (duration * f64::from(self.fps_hint())).round() as u64)
            .unwrap_or(0);
        self.total_frames = Some(total_frames);
        Ok(total_frames)
    }

    fn fps_hint(&self) -> u32 {
        self.video
            .get()
            .and_then(|metadata| {
                metadata
                    .streams
                    .iter()
                    .find(|probe| !probe.is_attached_picture())
                    .or_else(|| metadata.streams.first())
            })
            .and_then(|probe| parse_fps(&probe.r_frame_rate))
            .unwrap_or(30)
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
        if probe.pix_fmt.is_empty() {
            anyhow::bail!(
                "failed to get pixel format from ffprobe output for job {}",
                self.id
            );
        }
        Ok(probe.pix_fmt.clone())
    }

    // all mapped video streams (except attached pictures) in order, falling back to the first stream
    pub async fn video_stream_indices(&self) -> anyhow::Result<Vec<u32>> {
        let streams = &self.video_metadata().await?.streams;
        let indices: Vec<u32> = streams
            .iter()
            .filter(|probe| !probe.is_attached_picture())
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
            .find(|probe| !probe.is_attached_picture())
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
                let path = format!("input/{}.{}", self.id, self.from);
                let output = run_ffprobe(&[
                    "-v",
                    "error",
                    "-select_streams",
                    "v",
                    "-show_entries",
                    "stream=index,codec_name,width,height,pix_fmt,r_frame_rate,bit_rate:stream_disposition=attached_pic:format=duration",
                    "-of",
                    "json",
                    &path,
                ])
                .await?;

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
                let path = format!("input/{}.{}", self.id, self.from);
                let output = run_ffprobe(&[
                    "-v",
                    "error",
                    "-select_streams",
                    "a",
                    "-show_entries",
                    "stream=index,codec_name",
                    "-of",
                    "json",
                    &path,
                ])
                .await?;
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
    Error(String),
}
