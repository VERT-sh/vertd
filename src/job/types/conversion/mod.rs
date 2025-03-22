mod converter;
mod format;
mod speed;

use std::collections::BTreeMap;

use converter::{Converter, ProgressUpdate};
use discord_webhook2::{message, webhook::DiscordWebhook};
pub use format::*;
use speed::ConversionSpeed;

use crate::{
    job::{get_fps, get_total_frames, JobTrait},
    send_message,
    state::APP_STATE,
    wait_for_message, OUTPUT_LIFETIME,
};
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;
use tokio::{fs, process::Command};
use uuid::Uuid;

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "type", rename_all = "camelCase")]
enum Message {
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
        let (speed, to) =
            wait_for_message!(stream, session, Message::StartJob { speed, to } => speed, to)?;

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

        let total_frames = get_total_frames(format!("input/{}.{}", self.id, self.from)).await?;

        self.total_frames = Some(total_frames);
        Ok(total_frames)
    }

    pub async fn fps(&mut self) -> anyhow::Result<u32> {
        if let Some(fps) = self.fps {
            return Ok(fps);
        }

        let fps = get_fps(format!("input/{}.{}", self.id, self.from)).await?;

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
