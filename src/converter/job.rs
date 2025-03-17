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
    pub completed: bool,
    total_frames: Option<u64>,
    bitrate: Option<u64>,
}

impl Job {
    pub fn new(auth_token: String, from: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            auth: auth_token,
            from,
            to: None,
            completed: false,
            total_frames: None,
            bitrate: None,
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

        let duration = lines.next()
            .ok_or_else(|| anyhow::anyhow!("Missing Duration - Please check if your file is not corrupted or damaged"))?
            .trim()
            .parse::<f64>()
            .map_err(|_| anyhow::anyhow!("Invalid Duration - Please check if your file is not corrupted or damaged"))?;
            
        let total_frames = (avg_frame_rate * duration).ceil() as u64;
        self.total_frames = Some(total_frames);

        Ok(total_frames)
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
