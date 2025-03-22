mod compressor;
mod format;

use std::env;

use crate::{
    job::{get_fps, get_total_frames, gpu::get_gpu, JobTrait},
    send_message,
    state::APP_STATE,
    wait_for_message, OUTPUT_LIFETIME,
};
use compressor::{first_pass, second_pass, ProgressUpdate};
pub use format::*;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;
use tokio::{fs, process::Command};
use uuid::Uuid;

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "type", rename_all = "camelCase")]
enum Message {
    #[serde(rename = "startJob", rename_all = "camelCase")]
    StartJob { size_kb: u64 },

    #[serde(rename = "jobFinished", rename_all = "camelCase")]
    JobFinished,

    #[serde(rename = "progressUpdate", rename_all = "camelCase")]
    ProgressUpdate(ProgressUpdate),

    // #[serde(rename = "progressUpdate", rename_all = "camelCase")]
    // ProgressUpdate(ProgressUpdate),
    #[serde(rename = "error", rename_all = "camelCase")]
    Error { message: String },
}

impl Into<String> for Message {
    fn into(self) -> String {
        serde_json::to_string(&self).unwrap()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[skip_serializing_none]
pub struct CompressionJob {
    pub id: Uuid,
    pub auth: String,
    pub target_size_kb: Option<u64>,
    pub completed: bool,
    pub format: CompressorFormat,
    total_frames: Option<u64>,
    fps: Option<u32>,
}

impl JobTrait for CompressionJob {
    fn auth(&self) -> &str {
        &self.auth
    }

    fn id(&self) -> Uuid {
        self.id
    }

    async fn handle_ws(
        &mut self,
        mut session: actix_ws::Session,
        mut stream: actix_ws::AggregatedMessageStream,
    ) -> anyhow::Result<()> {
        let size_kb =
            wait_for_message!(stream, session, Message::StartJob { size_kb } => size_kb)? - 128; // 128k for audio
        self.target_size_kb = Some(size_kb);

        let mut rx = first_pass(self, size_kb).await?;

        while let Some(msg) = rx.recv().await {
            match msg {
                ProgressUpdate::Error(err) => {
                    send_message!(
                        session,
                        Message::Error {
                            message: err.to_string()
                        }
                    )
                    .await?;
                }

                ProgressUpdate::Frame(frame) => {
                    send_message!(
                        session,
                        Message::ProgressUpdate(ProgressUpdate::Frame(frame))
                    )
                    .await?;
                }
            }
        }

        let total_frames = self.total_frames().await?;
        let mut rx = second_pass(self, size_kb).await?;

        while let Some(msg) = rx.recv().await {
            match msg {
                ProgressUpdate::Error(err) => {
                    send_message!(
                        session,
                        Message::Error {
                            message: err.to_string()
                        }
                    )
                    .await?;
                }

                ProgressUpdate::Frame(frame) => {
                    send_message!(
                        session,
                        Message::ProgressUpdate(ProgressUpdate::Frame(frame + total_frames))
                    )
                    .await?;
                }
            }
        }

        let is_empty =
            tokio::fs::metadata(format!("output/{}.{}", self.id, self.format.to_string()))
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

            log::warn!("TODO: handle job failure for compression");
            return Ok(());
        }

        self.completed = true;
        send_message!(session, Message::JobFinished).await?;

        let id = self.id;
        let format = self.format.to_string();

        tokio::spawn(async move {
            tokio::time::sleep(OUTPUT_LIFETIME).await;
            let mut app_state = APP_STATE.lock().await;
            app_state.jobs.remove(&id);
            drop(app_state);

            let path = format!("output/{}.{}", id, format);
            if let Err(e) = fs::remove_file(&path).await {
                if e.kind() != std::io::ErrorKind::NotFound {
                    log::error!("failed to remove output file: {}", e);
                }
            }
        });

        fs::remove_file(&format!("input/{}.{}", self.id, self.format)).await?;

        Ok(())
    }

    fn completed(&self) -> bool {
        self.completed
    }

    fn output_path(&self) -> Option<String> {
        Some(format!("output/{}.{}", self.id, self.format))
    }
}

impl CompressionJob {
    pub fn new(auth_token: String, from: CompressorFormat) -> Self {
        Self {
            id: Uuid::new_v4(),
            auth: auth_token,
            target_size_kb: None,
            completed: false,
            format: from,
            total_frames: None,
            fps: None,
        }
    }

    pub async fn total_frames(&mut self) -> anyhow::Result<u64> {
        if let Some(total_frames) = self.total_frames {
            return Ok(total_frames);
        }

        let total_frames = get_total_frames(format!("input/{}.{}", self.id, self.format)).await?;

        self.total_frames = Some(total_frames);
        Ok(total_frames)
    }

    pub async fn fps(&mut self) -> anyhow::Result<u32> {
        if let Some(fps) = self.fps {
            return Ok(fps);
        }

        let fps = get_fps(format!("input/{}.{}", self.id, self.format)).await?;

        self.fps = Some(fps);
        Ok(fps)
    }
}
