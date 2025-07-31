use std::collections::BTreeMap;

use actix_web::{get, rt, web, Error, HttpRequest, HttpResponse};
use actix_ws::AggregatedMessage;
use discord_webhook2::{message, webhook::DiscordWebhook};
use futures_util::StreamExt as _;
use log::error;
use serde::{Deserialize, Serialize};
use tokio::fs;
use uuid::Uuid;

use crate::{
    converter::{format::ConverterFormat, job::ProgressUpdate, speed::ConversionSpeed, Converter},
    state::APP_STATE,
    OUTPUT_LIFETIME,
};

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "camelCase")]
pub enum Message {
    #[serde(rename = "startJob", rename_all = "camelCase")]
    StartJob {
        token: String,
        job_id: Uuid,
        to: String,
        speed: ConversionSpeed,
    },

    #[serde(rename = "jobFinished", rename_all = "camelCase")]
    JobFinished { job_id: Uuid },

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

#[get("/ws")]
pub async fn websocket(req: HttpRequest, stream: web::Payload) -> Result<HttpResponse, Error> {
    let (res, mut session, stream) = actix_ws::handle(&req, stream)?;

    let mut stream = stream
        .aggregate_continuations()
        .max_continuation_size(2_usize.pow(20));

    rt::spawn(async move {
        while let Some(Ok(AggregatedMessage::Text(text))) = stream.next().await {
            let message: Message = match serde_json::from_str(&text) {
                Ok(message) => message,
                Err(e) => {
                    let message: String = Message::Error {
                        message: format!("failed to parse message: {}", e),
                    }
                    .into();
                    session.text(message).await.unwrap();
                    continue;
                }
            };

            match message {
                Message::StartJob {
                    token,
                    job_id,
                    to,
                    speed,
                } => {
                    let Some(mut job) = ({
                        let mut app_state = APP_STATE.lock().await;
                        let job = app_state.jobs.get_mut(&job_id);
                        let clone = job.as_ref().map(|j| (*j).clone());
                        if let Some(job) = job {
                            if job.completed {
                                let message: String = Message::Error {
                                    message: "job already completed".to_string(),
                                }
                                .into();
                                session.text(message).await.unwrap();
                                continue;
                            }
                            job.to = Some(to.clone());
                        }
                        clone
                    }) else {
                        let message: String = Message::Error {
                            message: "job not found".to_string(),
                        }
                        .into();
                        session.text(message).await.unwrap();
                        continue;
                    };

                    if job.auth != token {
                        let message: String = Message::Error {
                            message: "invalid token".to_string(),
                        }
                        .into();
                        session.text(message).await.unwrap();
                        continue;
                    }

                    let Ok(from) = job.from.parse::<ConverterFormat>() else {
                        let message: String = Message::Error {
                            message: "invalid input format".to_string(),
                        }
                        .into();
                        session.text(message).await.unwrap();
                        continue;
                    };

                    let Ok(to) = to.parse::<ConverterFormat>() else {
                        let message: String = Message::Error {
                            message: "invalid output format".to_string(),
                        }
                        .into();
                        session.text(message).await.unwrap();
                        continue;
                    };

                    let converter = Converter::new(from, to, speed);

                    let mut rx = match converter.convert(&mut job).await {
                        Ok(rx) => rx,
                        Err(e) => {
                            let message: String = Message::Error {
                                message: format!("failed to convert: {}", e),
                            }
                            .into();
                            session.text(message).await.unwrap();
                            continue;
                        }
                    };

                    let mut logs = Vec::new();

                    while let Some(update) = rx.recv().await {
                        match update {
                            ProgressUpdate::Error(err) => {
                                logs.push(err);
                            }
                            _ => {
                                let message: String = Message::ProgressUpdate(update).into();
                                session.text(message).await.unwrap();
                            }
                        }
                    }

                    let mut app_state = APP_STATE.lock().await;
                    if let Some(job) = app_state.jobs.get_mut(&job_id) {
                        job.completed = true;
                    }
                    drop(app_state);

                    // check if output/{}.{} exists and isn't empty
                    let is_empty = fs::metadata(&format!("output/{}.{}", job_id, to.to_string()))
                        .await
                        .map(|m| m.len() == 0)
                        .unwrap_or(true);

                    if is_empty {
                        log::error!("job {} failed", job_id);
                        let message: String = Message::Error {
                            message: "oops -- your job failed! make sure your file is valid before trying again - maddie has been notified :)"
                                .to_string(),
                        }
                        .into();
                        session.text(message).await.unwrap();

                        let from = job.from.clone();
                        let to = to.to_string().to_string();

                        tokio::spawn(async move {
                            if let Err(e) =
                                handle_job_failure(job_id, from, to, logs.join("\n")).await
                            {
                                log::error!("failed to handle job failure: {}", e);
                            }
                        });
                    } else {
                        let message: String = Message::JobFinished { job_id }.into();
                        session.text(message).await.unwrap();
                    }

                    tokio::spawn(async move {
                        tokio::time::sleep(OUTPUT_LIFETIME).await;
                        let mut app_state = APP_STATE.lock().await;
                        app_state.jobs.remove(&job_id);
                        drop(app_state);

                        let path = format!("output/{}.{}", job_id, to.to_string());
                        if let Err(e) = fs::remove_file(&path).await {
                            if e.kind() != std::io::ErrorKind::NotFound {
                                log::error!("failed to remove output file: {}", e);
                            }
                        }
                    });

                    match fs::remove_file(&format!("input/{}.{}", job.id, job.from)).await {
                        Ok(_) => {}
                        Err(e) => {
                            error!("failed to remove input file: {}", e);
                            let message: String = Message::Error {
                                message: format!("failed to remove input file: {}", e),
                            }
                            .into();
                            session.text(message).await.unwrap();
                            continue;
                        }
                    };
                }

                _ => {}
            }
        }
    });

    Ok(res)
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
