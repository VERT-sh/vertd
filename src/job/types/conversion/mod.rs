mod format;
mod speed;

pub use format::*;

use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;
use tokio::process::Command;
use uuid::Uuid;

use crate::job::JobTrait;

const DEFAULT_BITRATE: u64 = 4 * 1_000_000;
const BITRATE_MULTIPLIER: f64 = 2.5;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[skip_serializing_none]
pub struct ConversionJob {
    pub id: Uuid,
    pub auth: String,
    pub from: String,
    pub to: Option<String>,
    pub completed: bool,
    total_frames: Option<u64>,
    bitrate: Option<u64>,
    fps: Option<u32>,
}

impl JobTrait for ConversionJob {
    fn id(&self) -> Uuid {
        self.id
    }

    fn auth(&self) -> &str {
        &self.auth
    }

    fn handle_ws(
        &self,
        session: actix_ws::Session,
        stream: actix_ws::AggregatedMessageStream,
        shutdown: std::sync::Arc<tokio::sync::Notify>,
    ) {
        todo!("implement handle_ws for ConversionJob");
    }
}

impl ConversionJob {
    pub fn new(auth_token: String, from: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            auth: auth_token,
            from,
            to: None,
            completed: false,
            total_frames: None,
            bitrate: None,
            fps: None,
        }
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

        let output = Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-select_streams",
                "v:0",
                "-show_entries",
                "stream=avg_frame_rate,duration",
                "-of",
                "default=nokey=1:noprint_wrappers=1",
                &format!("input/{}.{}", self.id, self.from),
            ])
            .output()
            .await?;

        let output_str = String::from_utf8(output.stdout)?;
        let mut lines = output_str.lines();

        let avg_frame_rate = lines.next()
            .unwrap_or("60/1")
            .trim()
            .split('/')
            .map(|s| s.parse::<f64>().map_err(|_| anyhow::anyhow!("Invalid Frame Rate - Please check if your file is not corrupted or damaged")))
            .collect::<Result<Vec<f64>, _>>() // Collect results and return an error if any parsing fails
            .and_then(|nums| {
                if nums.len() == 2 && nums[1] != 0.0 {
                    Ok(nums[0] / nums[1])
                } else {
                    Err(anyhow::anyhow!("Invalid Frame Rate - Please check if your file is not corrupted or damaged"))
                }
            })?;

        let duration = lines
            .next()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Missing Duration - Please check if your file is not corrupted or damaged"
                )
            })?
            .trim()
            .parse::<f64>()
            .map_err(|_| {
                anyhow::anyhow!(
                    "Invalid Duration - Please check if your file is not corrupted or damaged"
                )
            })?;

        let total_frames = (avg_frame_rate * duration).ceil() as u64;
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
}
