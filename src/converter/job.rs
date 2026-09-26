use log::warn;
use serde::{Deserialize, Serialize};
use tokio::process::Command;
use uuid::Uuid;

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

    pub async fn bitrate(&mut self) -> anyhow::Result<u64> {
        if let Some(bitrate) = self.bitrate {
            return Ok(bitrate);
        }

        let output = Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-select_streams",
                "v:0",
                "-show_entries",
                "stream=bit_rate",
                "-of",
                "default=nokey=1:noprint_wrappers=1",
                &format!("input/{}.{}", self.id, self.from),
            ])
            .output()
            .await?;

        // use detected bitrate
        let bitrate = String::from_utf8(output.stdout)?.trim().parse::<u64>().ok();
        if let Some(bitrate_value) = bitrate {
            return Ok(bitrate_value);
        }

        // else check resolution and use default bitrate (based on resolution)
        let (width, height) = self.resolution().await?;
        let default_bitrate = match (width, height) {
            (w, h) if w >= 3840 || h >= 2160 => 30_000_000, // 4K - 30 Mbps
            (w, h) if w >= 2560 || h >= 1440 => 14_000_000, // 2K - 14 Mbps
            (w, h) if w >= 1920 || h >= 1080 => 7_000_000,  // 1080p - 7 Mbps
            (w, h) if w >= 1280 || h >= 720 => 4_000_000,   // 720p - 4 Mbps
            _ => 1_500_000,                                 // SD - 1.5 Mbps
        };

        self.bitrate = Some(default_bitrate);
        Ok(default_bitrate)
    }

    pub async fn total_frames(&mut self) -> anyhow::Result<u64> {
        if let Some(total_frames) = self.total_frames {
            return Ok(total_frames);
        }

        let path = format!("input/{}.{}", self.id, self.from);

        let output = Command::new("ffprobe")
            .args([
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
            .output()
            .await?;

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

        let path = format!("input/{}.{}", self.id, self.from);

        let output = Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-select_streams",
                "v:0",
                "-show_entries",
                "stream=r_frame_rate",
                "-of",
                "default=nokey=1:noprint_wrappers=1",
                &path,
            ])
            .output()
            .await?;

        let fps_out = String::from_utf8(output.stdout)?;
        let fps_trim = fps_out
            .lines()
            .find(|l| !l.trim().is_empty())
            .map(|s| s.trim())
            .unwrap_or("");

        if fps_trim.is_empty() {
            warn!("ffprobe returned empty fps for {}", path);
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
            warn!(
                "failed to parse fps '{}' from ffprobe for {}",
                fps_trim, path
            );
            30u32
        });

        self.fps = Some(result);
        Ok(result)
    }

    pub async fn bitrate_and_fps(&mut self) -> anyhow::Result<(u64, u32)> {
        let (bitrate, fps) = (self.bitrate().await?, self.fps().await?);
        Ok((bitrate, fps))
    }

    pub async fn resolution(&self) -> anyhow::Result<(u32, u32)> {
        let path = format!("input/{}.{}", self.id, self.from);

        let output = Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-select_streams",
                "v:0",
                "-show_entries",
                "stream=width,height",
                "-of",
                "csv=s=x:p=0",
                &path,
            ])
            .output()
            .await?;

        let res_out = String::from_utf8(output.stdout)?;
        let res_str = res_out
            .lines()
            .find(|l| !l.trim().is_empty())
            .map(|s| s.trim())
            .ok_or_else(|| {
                anyhow::anyhow!("failed to get resolution from ffprobe output: {}", res_out)
            })?;
        let mut parts = res_str.split('x');
        let width = parts
            .next()
            .ok_or_else(|| {
                anyhow::anyhow!("failed to get width from ffprobe output: '{}'", res_str)
            })?
            .trim()
            .parse::<u32>()?;
        let height = parts
            .next()
            .ok_or_else(|| {
                anyhow::anyhow!("failed to get height from ffprobe output: '{}'", res_str)
            })?
            .trim()
            .parse::<u32>()?;

        Ok((width, height))
    }

    pub async fn pix_fmt(&self) -> anyhow::Result<String> {
        let path = format!("input/{}.{}", self.id, self.from);

        let output = Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-select_streams",
                "v:0",
                "-show_entries",
                "stream=pix_fmt",
                "-of",
                "default=nokey=1:noprint_wrappers=1",
                &path,
            ])
            .output()
            .await?;

        let pix_out = String::from_utf8(output.stdout)?;
        let pix = pix_out
            .lines()
            .find(|l| !l.trim().is_empty())
            .map(|s| s.trim().to_string())
            .ok_or_else(|| anyhow::anyhow!("failed to get pixel format from ffprobe output"))?;

        Ok(pix)
    }

    pub async fn codecs(&self) -> anyhow::Result<(String, String)> {
        let path = format!("input/{}.{}", self.id, self.from);

        // Video codec
        let output = Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-select_streams",
                "v:0",
                "-show_entries",
                "stream=codec_name",
                "-of",
                "default=nokey=1:noprint_wrappers=1",
                &path,
            ])
            .output()
            .await?;

        let video_codec = String::from_utf8(output.stdout)?
            .lines()
            .next()
            .unwrap_or("none")
            .to_string();

        // Audio codec
        let output = Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-select_streams",
                "a:0",
                "-show_entries",
                "stream=codec_name",
                "-of",
                "default=nokey=1:noprint_wrappers=1",
                &path,
            ])
            .output()
            .await?;

        let audio_codec = String::from_utf8(output.stdout)?
            .lines()
            .next()
            .unwrap_or("none")
            .to_string();

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
