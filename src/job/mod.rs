pub mod gpu;
pub mod types;

use actix_ws::{AggregatedMessageStream, Session};
use enum_dispatch::enum_dispatch;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;
use strum_macros::AsRefStr;
use tokio::process::Command;
use types::{CompressionJob as Compression, ConversionJob as Conversion};
use uuid::Uuid;

#[macro_export]
macro_rules! wait_for_message {
    ($stream:expr, $session:expr, $message:pat => $($data:expr),*) => {{
        use futures_util::StreamExt as _;
        let mut data = None;
        while let Some(Ok(msg)) = $stream.next().await {
            match msg {
                ::actix_ws::AggregatedMessage::Ping(b) => {
                    $session.pong(&b).await?;
                }
                ::actix_ws::AggregatedMessage::Text(text) => {
                    let msg: Message = ::serde_json::from_str(&text)?;
                    if let $message = msg {
                        data = Some(($($data),*));
                        break;
                    } else {
                        log::error!("Invalid message: {:?}", msg);
                    }
                }
                _ => {}
            }
        }

        data.ok_or_else(|| anyhow::anyhow!("No message received"))
    }};
}

#[enum_dispatch]
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, AsRefStr)]
#[serde(rename_all = "camelCase")]
#[serde(tag = "type")]
#[skip_serializing_none]
pub enum Job {
    Conversion,
    Compression,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum JobType {
    Conversion,
    Compression,
}

#[enum_dispatch(Job)]
pub trait JobTrait: Clone {
    fn id(&self) -> Uuid;
    fn auth(&self) -> &str;

    async fn handle_ws(
        &mut self,
        session: Session,
        stream: AggregatedMessageStream,
    ) -> anyhow::Result<()>;

    fn completed(&self) -> bool;
    fn output_path(&self) -> Option<String>;
}

pub async fn get_total_frames(path: impl Into<String>) -> anyhow::Result<u64> {
    let path = path.into();
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
        .map_err(|e| anyhow::anyhow!("failed to parse total frames: {}", e))?;

    log::debug!("Total frames: {}", total_frames);

    let total_frames = total_frames
        .trim()
        .parse::<u64>()
        .map_err(|e| anyhow::anyhow!("failed to parse total frames: {}", e))?;

    Ok(total_frames)
}

pub async fn get_fps(path: impl Into<String>) -> anyhow::Result<u32> {
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
            &path.into(),
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

    Ok(fps)
}
