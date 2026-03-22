use std::{collections::BTreeMap, env, io::ErrorKind};

use actix_web::{get, rt, web, Error, HttpRequest, HttpResponse};
use actix_ws::AggregatedMessage;
use discord_webhook2::{message, webhook::DiscordWebhook};
use futures_util::StreamExt as _;
use serde::{Deserialize, Serialize};
use tokio::fs;
use uuid::Uuid;

use crate::{
    converter::{
        format::ConverterFormat,
        gpu::ConverterGPU,
        job::{JobState, ProgressUpdate},
        speed::ConversionSpeed,
        ConversionSettings, Converter,
    },
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
        settings: ConversionSettings,
    },

    #[serde(rename = "cancelJob", rename_all = "camelCase")]
    CancelJob { token: String, job_id: Uuid },

    #[serde(rename = "jobFinished", rename_all = "camelCase")]
    JobFinished { job_id: Uuid },

    #[serde(rename = "jobCancelled", rename_all = "camelCase")]
    JobCancelled { job_id: Uuid },

    #[serde(rename = "jobRetried", rename_all = "camelCase")]
    JobRetried { job_id: Uuid },

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

async fn send_ws_message(session: &mut actix_ws::Session, message: Message) -> bool {
    let payload: String = message.into();
    match session.text(payload).await {
        Ok(_) => true,
        Err(e) => {
            log::error!("failed to send websocket message: {}", e);
            false
        }
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
                    if !send_ws_message(
                        &mut session,
                        Message::Error {
                            message: format!("failed to parse message: {}", e),
                        },
                    )
                    .await
                    {
                        break;
                    }

                    continue;
                }
            };

            if let Message::StartJob {
                token,
                job_id,
                to,
                settings,
            } = message
            {
                let Some(mut job) = ({
                    let app_state = APP_STATE.lock().await;
                    let job = app_state.jobs.get(&job_id);
                    let clone = job.cloned();
                    if let Some(job) = job {
                        if job.completed() {
                            if !send_ws_message(
                                &mut session,
                                Message::Error {
                                    message: "job already completed".to_string(),
                                },
                            )
                            .await
                            {
                                break;
                            }

                            continue;
                        }
                    }

                    clone
                }) else {
                    if !send_ws_message(
                        &mut session,
                        Message::Error {
                            message: "job not found".to_string(),
                        },
                    )
                    .await
                    {
                        break;
                    }

                    continue;
                };

                if job.auth != token {
                    if !send_ws_message(
                        &mut session,
                        Message::Error {
                            message: "invalid token".to_string(),
                        },
                    )
                    .await
                    {
                        break;
                    }

                    continue;
                }

                let Ok(from) = job.from.parse::<ConverterFormat>() else {
                    if !send_ws_message(
                        &mut session,
                        Message::Error {
                            message: "invalid input format".to_string(),
                        },
                    )
                    .await
                    {
                        break;
                    }

                    continue;
                };

                let Ok(to) = to.parse::<ConverterFormat>() else {
                    if !send_ws_message(
                        &mut session,
                        Message::Error {
                            message: "invalid output format".to_string(),
                        },
                    )
                    .await
                    {
                        break;
                    }

                    continue;
                };

                {
                    let mut app_state = APP_STATE.lock().await;
                    if let Some(state_job) = app_state.jobs.get_mut(&job_id) {
                        state_job.to = Some(to.to_string());
                    }
                }
                job.to = Some(to.to_string());

                log::info!("settings for job {}: {:?}", job_id, settings);

                // determine speed - vertdspeedslider is 0-5, from very slow to very fast
                // but if bitrate is set, ignore speed slider
                let speed = match settings.video_bitrate.as_deref() {
                    Some("auto") | None => {
                        match settings.vertd_speed {
                            Some(0) => ConversionSpeed::VerySlow,
                            Some(1) => ConversionSpeed::Slower,
                            Some(2) => ConversionSpeed::Slow,
                            Some(3) => ConversionSpeed::Medium,
                            Some(4) => ConversionSpeed::Fast,
                            Some(5) => ConversionSpeed::UltraFast,
                            _ => ConversionSpeed::Medium, // fallback
                        }
                    }
                    Some(bitrate_str) => {
                        // use custom bitrate
                        match bitrate_str.parse::<u32>() {
                            Ok(bitrate) => ConversionSpeed::Bitrate(bitrate),
                            Err(_) => ConversionSpeed::Medium, // fallback
                        }
                    }
                };

                let converter = Converter::new(from, to, speed.clone(), settings.clone());

                let (gpu, vaapi_device_path) = {
                    let app_state = APP_STATE.lock().await;
                    let gpu = app_state
                        .gpu
                        .ok_or_else(|| "GPU not initialized, please restart vertd.".to_string());
                    let device_path = app_state.vaapi_device_path.clone();
                    (gpu, device_path)
                };

                let gpu = match gpu {
                    Ok(gpu) => gpu,
                    Err(msg) => {
                        if !send_ws_message(&mut session, Message::Error { message: msg }).await {
                            break;
                        }

                        continue;
                    }
                };

                let (mut rx, process) = match converter
                    .convert(&mut job, &gpu, vaapi_device_path.as_deref())
                    .await
                {
                    Ok((rx, process)) => (rx, process),
                    Err(e) => {
                        if !send_ws_message(
                            &mut session,
                            Message::Error {
                                message: format!("failed to convert: {}", e),
                            },
                        )
                        .await
                        {
                            break;
                        }

                        continue;
                    }
                };

                let mut logs = Vec::new();
                let mut fallback_logs = Vec::new();
                let mut is_fallback = false;
                let mut job_cancelled = false;
                let mut current_gpu = gpu;
                let mut process_opt = Some(process);
                'conversion: loop {
                    // store process in case user wants to cancel
                    if let Some(proc) = process_opt.take() {
                        let mut app_state = APP_STATE.lock().await;
                        app_state.active_processes.insert(job_id, proc);
                    }

                    let mut conversion_finished = false;

                    // send progress updates and listen for cancellation
                    loop {
                        tokio::select! {
                            update = rx.recv() => {
                                match update {
                                    Some(ProgressUpdate::Error(err)) => {
                                        if is_fallback {
                                            fallback_logs.push(err);
                                        } else {
                                            logs.push(err)
                                        }
                                    }
                                    Some(progress) => {
                                        if !send_ws_message(&mut session, Message::ProgressUpdate(progress)).await {
                                            break;
                                        }
                                    }
                                    None => {
                                        // conversion finished
                                        conversion_finished = true;
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
                                                job_cancelled = true;

                                                if !send_ws_message(&mut session, Message::JobCancelled { job_id }).await {
                                                    break 'conversion;
                                                }

                                                break;
                                            } else if !send_ws_message(
                                                &mut session,
                                                Message::Error {
                                                    message: "invalid token or job id for cancellation".to_string(),
                                                },
                                            )
                                            .await
                                            {
                                                break 'conversion;
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

                    if !conversion_finished {
                        // ws closed or job cancelled, clean up and exit outer loop
                        let process_to_kill = {
                            let mut app_state = APP_STATE.lock().await;
                            app_state.active_processes.remove(&job_id)
                        };

                        if let Some(mut process) = process_to_kill {
                            if let Err(e) = process.kill().await {
                                log::error!("failed to kill process for job {}: {}", job_id, e);
                            } else {
                                log::info!("killed process for job {}", job_id);
                            }
                        }

                        // TODO: possibly allow reconnection to websocket within certain timeframe?
                        // would need VERT ui changes, probably temporarily store job info in browser if refres/other reasons?
                        if job_cancelled {
                            {
                                let mut app_state = APP_STATE.lock().await;
                                app_state.jobs.remove(&job_id);
                            }

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
                        }

                        break 'conversion;
                    }

                    // clean up
                    {
                        let mut app_state = APP_STATE.lock().await;
                        if let Some(job) = app_state.jobs.get_mut(&job_id) {
                            job.state = JobState::Completed;
                        }

                        app_state.active_processes.remove(&job_id);

                        drop(app_state);
                    }

                    // check if output/{}.{} exists and isn't empty
                    let is_empty = fs::metadata(&format!("output/{}.{}", job_id, to))
                        .await
                        .map(|m| m.len() == 0)
                        .unwrap_or(true);

                    if is_empty {
                        // if GPU-related failure, try falling back to CPU/software conversion if allowed
                        let cpu_fallback =
                            env::var("ALLOW_CPU_FALLBACK").unwrap_or("true".to_string()) == "true";
                        if current_gpu != ConverterGPU::CPU && cpu_fallback {
                            log::info!("attempting CPU fallback for job {}", job_id);
                            let converter =
                                Converter::new(from, to, speed.clone(), settings.clone());
                            let (new_rx, new_process) =
                                match converter.convert(&mut job, &ConverterGPU::CPU, None).await {
                                    Ok((rx, process)) => (rx, process),
                                    Err(e) => {
                                        if !send_ws_message(
                                            &mut session,
                                            Message::Error {
                                                message: format!(
                                                    "failed to convert with CPU fallback: {}",
                                                    e
                                                ),
                                            },
                                        )
                                        .await
                                        {
                                            break 'conversion;
                                        }

                                        continue;
                                    }
                                };
                            rx = new_rx;
                            process_opt = Some(new_process);
                            current_gpu = ConverterGPU::CPU;
                            is_fallback = true;
                            if !send_ws_message(&mut session, Message::JobRetried { job_id }).await
                            {
                                break 'conversion;
                            }

                            continue 'conversion;
                        } else {
                            // if already CPU, CPU fallback failed, or CPU fallback not allowed, finally give up </3

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
                                // combine original and fallback logs if available
                                // ideally cpu wouldn't fail and we wouldn't need this, but who knows
                                let mut message = String::new();
                                if !fallback_logs.is_empty() {
                                    message.push_str("-- Original logs --\n");
                                    message.push_str(&logs.join("\n"));
                                    message.push_str("\n\n-- CPU fallback logs --\n");
                                    message.push_str(&fallback_logs.join("\n"));
                                } else {
                                    message.push_str(&logs.join("\n"));
                                }
                                message
                            };

                            if !send_ws_message(
                                &mut session,
                                Message::Error {
                                    message: error_message.clone(),
                                },
                            )
                            .await
                            {
                                break 'conversion;
                            }

                            let from = job.from.clone();
                            let to = to.to_string().to_string();

                            tokio::spawn(async move {
                                if let Err(e) =
                                    handle_job_failure(job_id, from, to, error_message.clone())
                                        .await
                                {
                                    log::error!("failed to handle job failure: {}", e);
                                }
                            });
                        }
                    } else if !send_ws_message(&mut session, Message::JobFinished { job_id }).await
                    {
                        break 'conversion;
                    }

                    break 'conversion;
                }

                tokio::spawn(async move {
                    // wait 15 seconds to let the user decide if they want to keep the file,
                    // and also for the copy op to finish...
                    tokio::time::sleep(tokio::time::Duration::from_secs(15)).await;
                    match fs::remove_file(&format!("input/{}.{}", job.id, job.from)).await {
                        Ok(_) => {}
                        Err(e) => {
                            // if "no such file / os error 2", dont print it
                            if e.kind() != ErrorKind::NotFound {
                                log::error!("failed to remove input file: {}", e);
                            }
                        }
                    };
                });

                tokio::spawn(async move {
                    tokio::time::sleep(OUTPUT_LIFETIME).await;
                    let mut app_state = APP_STATE.lock().await;
                    app_state.jobs.remove(&job_id);
                    drop(app_state);

                    let path = format!("output/{}.{}", job_id, to);
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
