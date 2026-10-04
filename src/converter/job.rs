use log::warn;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::process::Command;
use tokio::sync::OnceCell;
use uuid::Uuid;

pub const FFPROBE_TIMEOUT: Duration = Duration::from_secs(60);

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
    video: OnceCell<VideoProbe>,
    #[serde(skip)]
    audio: OnceCell<Vec<AudioStream>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct VideoProbe {
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
}

#[derive(Deserialize)]
struct Streams<T> {
    streams: Vec<T>,
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

        let probe = self.video_probe().await?;
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

        let path = format!("input/{}.{}", self.id, self.from);

        let output = run_ffprobe(&[
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-count_packets",
            "-show_entries",
            "stream=nb_read_packets",
            "-of",
            "csv=p=0",
            &path,
        ])
        .await?;

        validate_ffprobe_output(&output, &path, "reading total frames")?;

        let total_frames = String::from_utf8(output.stdout)
            .map_err(|e| anyhow::anyhow!("failed to parse total frames: {}", e))?
            .lines()
            .find_map(|s| {
                // Filter out non-numeric characters
                let numeric: String = s.chars().filter(|c| c.is_numeric()).collect();
                numeric.parse::<u64>().ok()
            })
            .ok_or_else(|| anyhow::anyhow!("Error parsing total frames from output"))?;

        self.total_frames = Some(total_frames);
        Ok(total_frames)
    }

    pub async fn fps(&mut self) -> anyhow::Result<u32> {
        if let Some(fps) = self.fps {
            return Ok(fps);
        }

        let probe = self.video_probe().await?;
        let fps_trim = probe.r_frame_rate.trim();

        if fps_trim.is_empty() {
            warn!("ffprobe returned empty fps for job {}", self.id);
            let default = 30u32;
            self.fps = Some(default);
            return Ok(default);
        }

        // parse fps which could be in the form of "30", "29.97", or "30000/1001"
        let parsed = if let Some((n_str, d_str)) = fps_trim.split_once('/') {
            match (n_str.trim().parse::<f64>(), d_str.trim().parse::<f64>()) {
                (Ok(n), Ok(d)) if d != 0.0 => Some((n / d).round() as u32),
                _ => None,
            }
        } else {
            fps_trim.parse::<f64>().ok().map(|f| f.round() as u32)
        };

        let result = parsed.unwrap_or_else(|| {
            warn!("failed to parse fps '{}' from ffprobe", fps_trim);
            30u32
        });

        self.fps = Some(result);
        Ok(result)
    }

    pub async fn resolution(&self) -> anyhow::Result<(u32, u32)> {
        let probe = self.video_probe().await?;
        if probe.width == 0 || probe.height == 0 {
            anyhow::bail!(
                "failed to get resolution from ffprobe output for job {}",
                self.id
            );
        }
        Ok((probe.width, probe.height))
    }

    pub async fn pix_fmt(&self) -> anyhow::Result<String> {
        let probe = self.video_probe().await?;
        if probe.pix_fmt.is_empty() {
            anyhow::bail!(
                "failed to get pixel format from ffprobe output for job {}",
                self.id
            );
        }
        Ok(probe.pix_fmt.clone())
    }

    async fn video_probe(&self) -> anyhow::Result<&VideoProbe> {
        self.video
            .get_or_try_init(|| async {
                let path = format!("input/{}.{}", self.id, self.from);
                let output = run_ffprobe(&[
                    "-v",
                    "error",
                    "-select_streams",
                    "v:0",
                    "-show_entries",
                    "stream=codec_name,width,height,pix_fmt,r_frame_rate,bit_rate",
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
                let video = probe.streams.into_iter().next().unwrap_or_default();
                Ok(video)
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
        let video_codec = self.video_probe().await?.codec_name.to_lowercase();
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

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "camelCase")]
pub enum ProgressUpdate {
    #[serde(rename = "frame", rename_all = "camelCase")]
    Frame(u64),
    #[serde(rename = "fps", rename_all = "camelCase")]
    FPS(f64),
    #[serde(rename = "error", rename_all = "camelCase")]
    Error(String),
}
