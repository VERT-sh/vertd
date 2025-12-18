use serde::{Deserialize, Serialize};
use tokio::process::Command;
use uuid::Uuid;

const DEFAULT_BITRATE: u64 = 4 * 1_000_000;
const BITRATE_MULTIPLIER: f64 = 2.5;

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

    // TODO: scale based on resolution
    pub async fn bitrate(&mut self) -> anyhow::Result<u64> {
        // Ok(DEFAULT_BITRATE)
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

        let bitrate = String::from_utf8(output.stdout)?;
        let bitrate = match bitrate.trim().parse::<u64>() {
            Ok(bitrate) => bitrate,
            Err(_) => DEFAULT_BITRATE,
        };

        self.bitrate = Some(bitrate);
        Ok(((bitrate as f64) * BITRATE_MULTIPLIER) as u64)
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
                &format!("input/{}.{}", self.id, self.from),
            ])
            .output()
            .await?;

        // its  gonna look like "30000/1001"
        let fps = String::from_utf8(output.stdout)
            .map_err(|e| anyhow::anyhow!("failed to parse fps: {}", e))?;

        let fps = fps.trim().split('/').collect::<Vec<&str>>();
        let fps = if fps.len() == 1 {
            fps[0].parse::<u32>()?
        } else if fps.len() == 2 {
            let numerator = fps[0].parse::<u32>()?;
            let denominator = fps[1].parse::<u32>()?;
            (numerator as f64 / denominator as f64).round() as u32
        } else if fps.len() == 3 {
            let numerator = fps[0].parse::<u32>()?;
            let denominator = fps[2].parse::<u32>()?;
            (numerator as f64 / denominator as f64).round() as u32
        } else {
            return Err(anyhow::anyhow!("failed to parse fps"));
        };

        self.fps = Some(fps);
        Ok(fps)
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

        let res_str = String::from_utf8(output.stdout)?;
        let mut parts = res_str.trim().split('x');
        let width = parts
            .next()
            .ok_or_else(|| anyhow::anyhow!("failed to get width"))?
            .parse::<u32>()?;
        let height = parts
            .next()
            .ok_or_else(|| anyhow::anyhow!("failed to get height"))?
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

        let pix_fmt = String::from_utf8(output.stdout)?
            .lines()
            .next()
            .ok_or_else(|| anyhow::anyhow!("failed to get pixel format"))?
            .to_string();

        Ok(pix_fmt)
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
