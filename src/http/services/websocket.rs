use std::{collections::BTreeMap, io::ErrorKind};

use actix_web::{get, rt, web, Error, HttpRequest, HttpResponse};
use actix_ws::AggregatedMessage;
use discord_webhook2::{message, webhook::DiscordWebhook};
use futures_util::StreamExt as _;
use log::error;
use serde::{Deserialize, Serialize};
use tokio::fs;
use uuid::Uuid;

use crate::{
    converter::{
        format::ConverterFormat,
        job::{JobState, ProgressUpdate},
        speed::ConversionSpeed,
        Converter,
    },
    state::APP_STATE,
    OUTPUT_LIFETIME,
};

fn default_keep_metadata() -> bool {
    true
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "camelCase")]
pub enum Message {
    #[serde(rename = "startJob", rename_all = "camelCase")]
    StartJob {
        token: String,
        job_id: Uuid,
        to: String,
        speed: ConversionSpeed,
        #[serde(default = "default_keep_metadata")]
        keep_metadata: bool,
    },

    #[serde(rename = "cancelJob", rename_all = "camelCase")]
    CancelJob { token: String, job_id: Uuid },

    #[serde(rename = "jobFinished", rename_all = "camelCase")]
    JobFinished { job_id: Uuid },

    #[serde(rename = "jobCancelled", rename_all = "camelCase")]
    JobCancelled { job_id: Uuid },

    #[serde(rename = "progressUpdate", rename_all = "camelCase")]
    ProgressUpdate(ProgressUpdate),

    #[serde(rename = "error", rename_all = "camelCase")]
    Error { message: String },
}

impl From<Message> for String {
    fn from(val: Message) -> Self {
        serde_json::to_string(&val).unwrap_or_default()
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

            if let Message::StartJob {
                token,
                job_id,
                to,
                speed,
                keep_metadata,
            } = message
            {
                let Some(mut job) = ({
                    let mut app_state = APP_STATE.lock().await;
                    let job = app_state.jobs.get_mut(&job_id);
                    let clone = job.as_ref().map(|j| (*j).clone());
                    if let Some(job) = job {
                        if job.completed() {
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

                let converter = Converter::new(from, to, speed, keep_metadata);

                let gpu = {
                    let app_state = APP_STATE.lock().await;
                    app_state.gpu.ok_or_else(|| {
                        "GPU not initialized, please restart vertd.".to_string()
                    })
                };

                let gpu = match gpu {
                    Ok(gpu) => gpu,
                    Err(msg) => {
                        let message: String = Message::Error { message: msg }.into();
                        session.text(message).await.unwrap();
                        continue;
                    }
                };

                let (mut rx, process) = match converter.convert(&mut job, &gpu).await {
                    Ok((rx, process)) => (rx, process),
                    Err(e) => {
                        let message: String = Message::Error {
                            message: format!("failed to convert: {}", e),
                        }
                        .into();
                        session.text(message).await.unwrap();
                        continue;
                    }
                };

                // store process in case user wants to cancel
                {
                    let mut app_state = APP_STATE.lock().await;
                    app_state.active_processes.insert(job_id, process);
                }

                let mut logs = Vec::new();
                let mut job_cancelled = false;

                // send progress updates and listen for cancellation
                loop {
                    tokio::select! {
                        update = rx.recv() => {
                            match update {
                                Some(ProgressUpdate::Error(err)) => {
                                    logs.push(err);
                                }
                                Some(progress) => {
                                    let message: String = Message::ProgressUpdate(progress).into();
                                    session.text(message).await.unwrap();
                                }
                                None => {
                                    // conversion finished
                                    break;
                                }
                            }
                        }

                        new_message = stream.next() => {
                            if let Some(Ok(AggregatedMessage::Text(text))) = new_message {
                                if let Ok(parsed_message) = serde_json::from_str::<Message>(&text) {
                                    if let Message::CancelJob { token: cancel_token, job_id: cancel_job_id } = parsed_message {
                                        if cancel_job_id == job_id && cancel_token == token {
                                            log::info!("cancelling job {}", job_id);

                                            let mut app_state = APP_STATE.lock().await;
                                            if let Some(mut process) = app_state.active_processes.remove(&job_id) {
                                                if let Err(e) = process.kill().await {
                                                    log::error!("failed to kill process for job {}: {}", job_id, e);
                                                } else {
                                                    log::info!("killed process for job {}", job_id);
                                                    job_cancelled = true;
                                                }
                                            }

                                            if let Some(job) = app_state.jobs.get_mut(&job_id) {
                                                job.state = JobState::Completed;
                                            }
                                            drop(app_state);

                                            let message: String = Message::JobCancelled { job_id }.into();
                                            session.text(message).await.unwrap();

                                            break;
                                        } else {
                                            let message: String = Message::Error {
                                                message: "invalid token or job id for cancellation".to_string(),
                                            }
                                            .into();
                                            session.text(message).await.unwrap();
                                        }
                                    }
                                }
                            } else if new_message.is_none() {
                                // ws closed
                                break;
                            }
                        }
                    }
                }

                {
                    let mut app_state = APP_STATE.lock().await;
                    if let Some(job) = app_state.jobs.get_mut(&job_id) {
                        job.state = JobState::Completed;
                    }

                    if !job_cancelled {
                        // clean process only if not cancelled
                        app_state.active_processes.remove(&job_id);
                    } else {
                        // clean up job if cancelled
                        app_state.jobs.remove(&job_id);
                        drop(app_state);

                        if let Err(e) =
                            fs::remove_file(&format!("input/{}.{}", job.id, job.from)).await
                        {
                            if e.kind() != ErrorKind::NotFound {
                                log::error!(
                                    "failed to remove input file after cancellation: {}",
                                    e
                                );
                            }
                        }
                        continue;
                    }

                    drop(app_state);
                }

                // check if output/{}.{} exists and isn't empty
                let is_empty = fs::metadata(&format!("output/{}.{}", job_id, to))
                    .await
                    .map(|m| m.len() == 0)
                    .unwrap_or(true);

                if is_empty {
                    // hacky :/
                    let mut app_state = APP_STATE.lock().await;
                    if let Some(job) = app_state.jobs.get_mut(&job_id) {
                        job.state = JobState::Failed;
                    }
                    drop(app_state);
                    log::error!("job {} failed", job_id);

                    let error_message = if logs.is_empty() {
                        "No error logs.".to_string()
                    } else {
                        logs.join("\n")
                    };

                    let message: String = Message::Error {
                        message: error_message,
                    }
                    .into();
                    session.text(message).await.unwrap();

                    let from = job.from.clone();
                    let to = to.to_string().to_string();

                    tokio::spawn(async move {
                        if let Err(e) = handle_job_failure(job_id, from, to, logs.join("\n")).await
                        {
                            log::error!("failed to handle job failure: {}", e);
                        }
                    });
                } else {
                    let message: String = Message::JobFinished { job_id }.into();
                    session.text(message).await.unwrap();
                }

                tokio::spawn(async move {
                    // wait 15 seconds to let the user decide if they want to keep the file,
                    // and also for the copy op to finish...
                    tokio::time::sleep(tokio::time::Duration::from_secs(15)).await;
                    match fs::remove_file(&format!("input/{}.{}", job.id, job.from)).await {
                        Ok(_) => {}
                        Err(e) => {
                            error!("failed to remove input file: {}", e);
                        }
                    };
                });

                tokio::spawn(async move {
                    tokio::time::sleep(OUTPUT_LIFETIME).await;
                    let mut app_state = APP_STATE.lock().await;
                    app_state.jobs.remove(&job_id);
                    drop(app_state);

                    let path = format!("output/{}.{}", job_id, to.to_string());
                    if let Err(e) = fs::remove_file(&path).await {
                        if e.kind() != ErrorKind::NotFound {
                            log::error!("failed to remove output file: {}", e);
                        }
                    }
                });
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
