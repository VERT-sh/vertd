mod converter;
mod format;
mod speed;

use std::collections::BTreeMap;

use actix_ws::AggregatedMessage;
use converter::{Converter, ProgressUpdate};
use discord_webhook2::{message, webhook::DiscordWebhook};
pub use format::*;
use futures_util::StreamExt as _;
use speed::ConversionSpeed;

use crate::{
    job::{Job, JobTrait},
    send_message,
    state::APP_STATE,
    OUTPUT_LIFETIME,
};
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;
use tokio::{fs, process::Command};
use uuid::Uuid;

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Message {
    #[serde(rename = "startJob", rename_all = "camelCase")]
    StartJob { to: String, speed: ConversionSpeed },

    #[serde(rename = "jobFinished", rename_all = "camelCase")]
    JobFinished,

    #[serde(rename = "progressUpdate", rename_all = "camelCase")]
    ProgressUpdate(ProgressUpdate),

    #[serde(rename = "error", rename_all = "camelCase")]
    Error { message: String },
}

impl Into<String> for Message {
    fn into(self) -> String {
        serde_json::to_string(&self).unwrap()
    }
}

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

    async fn handle_ws(
        &mut self,
        mut session: actix_ws::Session,
        mut stream: actix_ws::AggregatedMessageStream,
    ) -> anyhow::Result<()> {
        // wait till we receive a message where Message::StartJob
        let mut message = None;
        while let Some(Ok(msg)) = stream.next().await {
            match msg {
                AggregatedMessage::Ping(b) => {
                    session.pong(&b).await?;
                }

                AggregatedMessage::Text(text) => {
                    let msg: Message = serde_json::from_str(&text)?;
                    if matches!(msg, Message::StartJob { .. }) {
                        message = Some(msg);
                        break;
                    } else {
                        log::error!("Invalid message: {:?}", msg);
                    }
                }

                _ => {}
            }
        }

        let Message::StartJob { to, speed } =
            message.ok_or_else(|| anyhow::anyhow!("no message found"))?
        else {
            return Err(anyhow::anyhow!("no message found"));
        };

        let from = self.from.parse::<ConverterFormat>()?;
        let to = to.parse::<ConverterFormat>()?;

        self.to = Some(to.to_string());

        let converter = Converter::new(from, to, speed);

        let mut rx = converter.convert(self).await?;

        let mut logs = Vec::new();

        while let Some(update) = rx.recv().await {
            match update {
                ProgressUpdate::Error(err) => {
                    logs.push(err);
                }
                _ => {
                    send_message!(session, Message::ProgressUpdate(update)).await?;
                }
            }
        }

        let is_empty = fs::metadata(&format!("output/{}.{}", self.id, to.to_string()))
            .await
            .map(|m| m.len() == 0)
            .unwrap_or(true);

        if is_empty {
            log::error!("job {} failed", self.id);

            send_message!(
                session,
                Message::Error {
                    message: "oops -- your job failed! maddie has been notified :)".to_string()
                }
            )
            .await?;

            let from = self.from.clone();
            let to = to.to_string();

            let id = self.id;

            tokio::spawn(async move {
                if let Err(e) = handle_job_failure(id, from, to, logs.join("\n")).await {
                    log::error!("failed to handle job failure: {}", e);
                }
            });
        } else {
            send_message!(session, Message::JobFinished).await?;
            self.completed = true;
        }

        let id = self.id;

        tokio::spawn(async move {
            tokio::time::sleep(OUTPUT_LIFETIME).await;
            let mut app_state = APP_STATE.lock().await;
            app_state.jobs.remove(&id);
            drop(app_state);

            let path = format!("output/{}.{}", id, to.to_string());
            if let Err(e) = fs::remove_file(&path).await {
                if e.kind() != std::io::ErrorKind::NotFound {
                    log::error!("failed to remove output file: {}", e);
                }
            }
        });

        fs::remove_file(&format!("input/{}.{}", self.id, self.from)).await?;

        Ok(())
    }

    fn completed(&self) -> bool {
        self.completed
    }

    fn output_path(&self) -> Option<String> {
        self.to
            .as_ref()
            .map(|to| format!("output/{}.{}", self.id, to))
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

async fn handle_job_failure(
    job_id: Uuid,
    from: String,
    to: String,
    logs: String,
) -> anyhow::Result<()> {
    let client_url = std::env::var("WEBHOOK_URL")?;
    let mentions = std::env::var("WEBHOOK_PINGS").unwrap_or_else(|_| "".to_string());

    let mut files = BTreeMap::new();
    files.insert(format!("{}.log", job_id), logs.as_bytes().to_vec());

    let client = DiscordWebhook::new(&client_url)?;
    let message = message::Message::new(|m| {
        m.content(format!("🚨🚨🚨 {}", mentions)).embed(|e| {
            e.title("vertd job failed!")
                .field(|f| f.name("job id").value(job_id))
                .field(|f| f.name("from").value(format!(".{}", from)).inline(true))
                .field(|f| f.name("to").value(format!(".{}", to)).inline(true))
                .color(0xff83fa)
        })
    });

    client.send_with_files(&message, files).await?;

    Ok(())
}
