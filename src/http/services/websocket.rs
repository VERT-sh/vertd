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
        fallback::{self, FailureKind, RetryOverrides},
        format::ConverterFormat,
        gpu::ConverterGPU,
        job::{
            format_job_logs, has_video_stream, push_log, snapshot_logs, Job, JobState, LogBuffer,
            ProgressUpdate,
        },
        sequence::SequenceKind,
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
        settings: Box<ConversionSettings>,
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

const WS_SEND_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

async fn send_ws_message_timeout(session: &mut actix_ws::Session, message: Message) -> bool {
    match tokio::time::timeout(WS_SEND_TIMEOUT, send_ws_message(session, message)).await {
        Ok(ok) => ok,
        Err(_) => {
            log::warn!("websocket send timed out, closing connection");
            false
        }
    }
}

async fn try_send_ws_message(session: &mut actix_ws::Session, message: Message) {
    if !send_ws_message_timeout(session, message).await {
        log::warn!("could not deliver websocket message, client is probably gone");
    }
}

struct Attempt {
    label: String,
    logs: LogBuffer,
}

struct RetryState {
    job_id: Uuid,
    attempts: Vec<Attempt>,
    attempted_pix_fmts: Vec<String>,
    input_pix_fmt: String,
    // codec names of the job's audio streams, used to tell which stream the muxer rejected
    audio_codecs: Vec<String>,
    gpu: ConverterGPU,
    allow_cpu: bool,
    failed_encoder: String,
    probe_attempts: u8,
    // every recovery applied so far to know which ones were applied
    applied: RetryOverrides,
}

struct RetryPlan {
    gpu: ConverterGPU,
    overrides: RetryOverrides,
    action: fallback::RetryAction,
    label: String,
}

impl RetryState {
    fn new(
        job_id: Uuid,
        logs: LogBuffer,
        input_pix_fmt: String,
        audio_codecs: Vec<String>,
        gpu: ConverterGPU,
        allow_cpu: bool,
    ) -> Self {
        Self {
            job_id,
            attempts: vec![Attempt {
                label: "initial".to_string(),
                logs,
            }],
            attempted_pix_fmts: Vec::new(),
            input_pix_fmt,
            audio_codecs,
            gpu,
            allow_cpu,
            failed_encoder: String::new(),
            probe_attempts: 0,
            applied: RetryOverrides::default(),
        }
    }

    fn current_logs(&self) -> &LogBuffer {
        &self.attempts[self.attempts.len() - 1].logs
    }

    fn retries(&self) -> usize {
        self.attempts.len() - 1
    }

    fn exhausted(&self) -> bool {
        self.retries() >= fallback::MAX_ATTEMPTS
    }

    fn plan(&mut self, kind: FailureKind) -> Option<RetryPlan> {
        use fallback::RetryAction;

        if self.exhausted() {
            log::error!(
                "giving up on job {} after {} attempts",
                self.job_id,
                self.retries()
            );
            return None;
        }

        let action = fallback::next_action(
            kind,
            &self.failed_encoder,
            &self.input_pix_fmt,
            &self.attempted_pix_fmts,
            self.gpu,
            self.applied,
        )?;

        let mut overrides = self.applied;
        let mut gpu = self.gpu;
        let label = match action {
            RetryAction::PixFmt(pix_fmt) => {
                self.attempted_pix_fmts.push(pix_fmt.to_string());
                overrides.pix_fmt = Some(pix_fmt);
                format!("pix_fmt={pix_fmt}")
            }
            RetryAction::Software => {
                if !self.allow_cpu {
                    return None;
                }
                gpu = ConverterGPU::CPU;
                overrides.pix_fmt = None;
                overrides.sw_decode = false;
                "software".to_string()
            }
            RetryAction::SoftwareDecode => {
                overrides.sw_decode = true;
                "software decoding".to_string()
            }
            RetryAction::PadAlignment => {
                overrides.pad_alignment = true;
                "padded to alignment".to_string()
            }
            RetryAction::FixTimestamps => {
                overrides.fix_timestamps = true;
                "timestamp repair".to_string()
            }
            RetryAction::DeeperProbe => {
                self.probe_attempts += 1;
                format!("deeper probe ({})", self.probe_attempts)
            }
            RetryAction::TranscodeAudio => {
                overrides.transcode_audio = true;
                "audio transcode".to_string()
            }
            RetryAction::TranscodeVideo => {
                overrides.transcode_video = true;
                "video transcode".to_string()
            }
        };

        Some(RetryPlan {
            gpu,
            overrides,
            action,
            label,
        })
    }

    fn record(&mut self, plan: RetryPlan, logs: LogBuffer) {
        self.gpu = plan.gpu;
        self.applied = plan.overrides;
        self.attempts.push(Attempt {
            label: plan.label,
            logs,
        });
    }

    fn plan_for(&mut self, kind: FailureKind) -> Option<RetryPlan> {
        self.plan(kind)
    }

    fn record_plan(&mut self, plan: RetryPlan) {
        log::warn!("retrying job {} with {}", self.job_id, plan.label);
        self.record(plan, crate::converter::job::new_log_buffer());
    }

    async fn start_next(
        &mut self,
        converter: &Converter,
        job: &mut Job,
        vaapi_device_path: Option<&str>,
    ) -> Option<(
        tokio::sync::mpsc::Receiver<ProgressUpdate>,
        tokio::process::Child,
    )> {
        let stderr = format_job_logs(&snapshot_logs(self.current_logs()).await);
        let kind = fallback::classify(&stderr, &self.audio_codecs);
        self.start_for(kind, converter, job, vaapi_device_path)
            .await
    }

    async fn start_for(
        &mut self,
        kind: FailureKind,
        converter: &Converter,
        job: &mut Job,
        vaapi_device_path: Option<&str>,
    ) -> Option<(
        tokio::sync::mpsc::Receiver<ProgressUpdate>,
        tokio::process::Child,
    )> {
        // which encoder failed decides which formats are worth trying
        let stderr = format_job_logs(&snapshot_logs(self.current_logs()).await);
        if let Some(encoder) = fallback::detect_encoder(&stderr) {
            self.failed_encoder = encoder.to_string();
        }
        let plan = self.plan(kind)?;

        if matches!(plan.action, fallback::RetryAction::DeeperProbe) {
            job.widen_probe();
        }

        log::warn!(
            "retrying job {} with {} after {}",
            job.id,
            plan.label,
            kind_label(kind)
        );

        match converter
            .convert_with_overrides(job, &plan.gpu, vaapi_device_path, plan.overrides)
            .await
        {
            Ok((rx, process, logs)) => {
                self.record(plan, logs);
                Some((rx, process))
            }
            Err(e) => {
                log::warn!(
                    "retry ({}) failed to start for job {}: {}",
                    plan.label,
                    job.id,
                    e
                );
                push_log(
                    self.current_logs(),
                    format!("retry ({}) failed to start: {e}", plan.label),
                    1,
                )
                .await;
                None
            }
        }
    }
}

async fn attempt_report(attempts: &[Attempt]) -> String {
    if attempts.len() == 1 {
        return format_job_logs(&snapshot_logs(&attempts[0].logs).await);
    }

    let mut message = String::new();
    for (index, attempt) in attempts.iter().enumerate() {
        if index > 0 {
            message.push_str("\n\n");
        }
        message.push_str(&format!(
            "-- Attempt {} ({}) --\n",
            index + 1,
            attempt.label
        ));
        message.push_str(&format_job_logs(&snapshot_logs(&attempt.logs).await));
    }
    message
}

fn kind_label(kind: FailureKind) -> &'static str {
    match kind {
        FailureKind::PixelFormat => "a pixel format mismatch",
        FailureKind::Encoder => "an unsupported encoder",
        FailureKind::Decode => "a hardware decoding failure",
        FailureKind::Alignment => "an odd resolution size",
        FailureKind::Timestamps => "broken timestamps",
        FailureKind::Probe => "incomplete metadata",
        FailureKind::AudioCopy => "an audio stream the muxer rejected",
        FailureKind::VideoCopy => "a video stream the muxer rejected",
        FailureKind::Fatal => "an unrecoverable error",
    }
}

async fn remove_job_input(job: &Job) -> std::io::Result<()> {
    if job.is_sequence() {
        fs::remove_dir_all(job.input_dir()).await
    } else {
        fs::remove_file(job.input_path()).await
    }
}

async fn fail_job(
    session: &mut actix_ws::Session,
    job: &Job,
    job_id: Uuid,
    to: ConverterFormat,
    error_message: String,
) {
    {
        let mut app_state = APP_STATE.lock().await;
        if let Some(state_job) = app_state.jobs.get_mut(&job_id) {
            state_job.state = JobState::Failed;
        }
    }

    let output_path = format!("output/{}.{}", job_id, to);
    tokio::spawn(async move {
        tokio::time::sleep(OUTPUT_LIFETIME).await;
        APP_STATE.lock().await.jobs.remove(&job_id);
        if let Err(e) = fs::remove_file(&output_path).await {
            if e.kind() != ErrorKind::NotFound {
                log::error!("failed to remove failed output {}: {}", output_path, e);
            }
        }
    });

    try_send_ws_message(
        session,
        Message::Error {
            message: error_message.clone(),
        },
    )
    .await;

    // report regardless of the websocket state
    let from = job.from.clone();
    let to = to.to_string();
    tokio::spawn(async move {
        if let Err(e) = handle_job_failure(job_id, from, to, error_message).await {
            log::error!("failed to handle job failure: {e:#}");
        }
    });
}

#[get("/ws")]
pub async fn websocket(req: HttpRequest, stream: web::Payload) -> Result<HttpResponse, Error> {
    let (res, mut session, stream) = actix_ws::handle(&req, stream)?;

    let mut stream = stream
        .aggregate_continuations()
        .max_continuation_size(2_usize.pow(20));

    rt::spawn(async move {
        loop {
            let text = match stream.next().await {
                Some(Ok(AggregatedMessage::Text(text))) => text,
                Some(Ok(AggregatedMessage::Ping(payload))) => {
                    if session.pong(&payload).await.is_err() {
                        break;
                    }
                    continue;
                }
                Some(Ok(AggregatedMessage::Pong(_))) => continue,
                Some(Ok(AggregatedMessage::Close(_))) | None => break,
                Some(Ok(_)) => continue, // ignore binary/continuation frames we don't use
                Some(Err(_)) => break,   // transport error
            };

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
                let (job_clone, already_completed) = {
                    let app_state = APP_STATE.lock().await;
                    match app_state.jobs.get(&job_id) {
                        Some(job) => (Some(job.clone()), job.completed()),
                        None => (None, false),
                    }
                };

                if already_completed {
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

                let Some(mut job) = job_clone else {
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

                let from = if job.is_sequence() {
                    ConverterFormat::MediaSequence
                } else {
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

                    from
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

                if let Err(e) = settings.validate() {
                    log::warn!("invalid settings for job {}: {}", job_id, e);
                    if !send_ws_message(
                        &mut session,
                        Message::Error {
                            message: e.to_string(),
                        },
                    )
                    .await
                    {
                        break;
                    }
                    continue;
                }

                log::info!("settings for job {}: {:?}", job_id, settings);

                // determine speed - vertdspeedslider is 0-5, from very slow to very fast
                // but if bitrate is set, ignore speed slider
                let speed = match settings.video_bitrate.as_deref() {
                    Some("") | Some("auto") | None => {
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

                let converter = Converter::new(from, to, speed.clone(), (*settings).clone());

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

                // reserve job to prevent multiple ws clients from starting the same job simultaneously
                let reserved = {
                    let mut app_state = APP_STATE.lock().await;
                    if let Some(state_job) = app_state.jobs.get_mut(&job_id) {
                        state_job.try_start(to.to_string())
                    } else {
                        false
                    }
                };
                if !reserved {
                    if !send_ws_message(
                        &mut session,
                        Message::Error {
                            message: "job already started or no longer available".to_string(),
                        },
                    )
                    .await
                    {
                        break;
                    }
                    continue;
                }
                job.to = Some(to.to_string());

                let allow_cpu = env::var("ALLOW_CPU_FALLBACK")
                    .unwrap_or_else(|_| "true".to_string())
                    != "false";

                // do a retry of the job setup with a higher probe window if it fails
                let mut setup_retry = RetryState::new(
                    job_id,
                    crate::converter::job::new_log_buffer(),
                    job.pix_fmt().await.unwrap_or_default(),
                    job.audio_streams()
                        .await
                        .map(|streams| {
                            streams
                                .into_iter()
                                .map(|stream| stream.codec_name)
                                .collect::<Vec<String>>()
                        })
                        .unwrap_or_default(),
                    gpu,
                    allow_cpu,
                );

                let setup_result = 'setup: loop {
                    let (convert_result, setup_cancelled) = {
                        let setup = converter.convert(&mut job, &gpu, vaapi_device_path.as_deref());
                        tokio::pin!(setup);
                        let mut cancelled = false;
                        let result = loop {
                            tokio::select! {
                                result = &mut setup => break Some(result),
                                message = stream.next() => {
                                    match &message {
                                        Some(Ok(AggregatedMessage::Text(text))) => {
                                            if let Ok(Message::CancelJob { token: cancel_token, job_id: cancel_id }) =
                                                serde_json::from_str::<Message>(text)
                                            {
                                                if cancel_token == token && cancel_id == job_id {
                                                    cancelled = true;
                                                    break None;
                                                }
                                            }
                                        }
                                        Some(Ok(_)) => {} // ignore non-text/control frames during setup
                                        _ => break None, // disconnected/errored
                                    }
                                }
                            }
                        };
                        (result, cancelled)
                    };

                    let Some(result) = convert_result else {
                        // cancelled or disconnected during setup, clean up and stop
                        APP_STATE.lock().await.jobs.remove(&job_id);
                        if let Err(e) = remove_job_input(&job).await {
                            if e.kind() != ErrorKind::NotFound {
                                log::error!(
                                    "failed to remove input after setup cancellation: {}",
                                    e
                                );
                            }
                        }
                        if setup_cancelled {
                            let _ = send_ws_message(&mut session, Message::JobCancelled { job_id })
                                .await;
                        }
                        break None;
                    };

                    match result {
                        Ok(started) => break Some(started),
                        Err(e) => {
                            let message = e.to_string();
                            push_log(setup_retry.current_logs(), message.clone(), 1).await;
                            let kind = fallback::classify_probe(&message);

                            // keep retrying only while it looks recoverable
                            let can_retry = kind != FailureKind::Fatal && job.can_widen_probe();
                            if !can_retry {
                                // also report job setup failures to the discord webhook
                                log::error!(
                                    "job {} failed during setup after {} probe attempt(s): {e:#}",
                                    job_id,
                                    job.probe_attempts()
                                );
                                let error_message = if setup_retry.retries() > 0 {
                                    format!(
                                        "failed to convert: {e:#}\n\n{}",
                                        attempt_report(&setup_retry.attempts).await
                                    )
                                } else {
                                    format!("failed to convert: {e:#}")
                                };

                                fail_job(&mut session, &job, job_id, to, error_message).await;
                                break None;
                            }

                            // re-probe with a wider probe window and rerun job setup
                            match setup_retry.plan_for(kind) {
                                Some(plan) => {
                                    job.widen_probe();
                                    setup_retry.record_plan(plan);
                                    try_send_ws_message(
                                        &mut session,
                                        Message::JobRetried { job_id },
                                    )
                                    .await;
                                    continue 'setup;
                                }
                                None => break None,
                            }
                        }
                    }
                };

                let Some((mut rx, process, original_logs)) = setup_result else {
                    // setup was cancelled, failed unrecoverably, or retries ran out, earlier code reports what happened
                    continue;
                };

                let input_pix_fmt = job.pix_fmt().await.unwrap_or_default();
                let audio_codecs = job
                    .audio_streams()
                    .await
                    .map(|streams| {
                        streams
                            .into_iter()
                            .map(|stream| stream.codec_name)
                            .collect::<Vec<String>>()
                    })
                    .unwrap_or_default();
                let mut retry = RetryState::new(
                    job_id,
                    original_logs,
                    input_pix_fmt,
                    audio_codecs,
                    gpu,
                    allow_cpu,
                );
                let mut job_cancelled = false;
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
                                        push_log(retry.current_logs(), err.message, err.repeats).await;
                                    }
                                    Some(progress) => {
                                        // don't forward progress updates of retries, only the first attempt
                                        // TODO: is this bad ux?
                                        if retry.retries() > 0 {
                                            continue;
                                        }
                                        if !send_ws_message_timeout(&mut session, Message::ProgressUpdate(progress)).await {
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
                                match new_message {
                                    Some(Ok(AggregatedMessage::Text(text))) => {
                                        if let Ok(Message::CancelJob { token: cancel_token, job_id: cancel_job_id }) = serde_json::from_str::<Message>(&text) {
                                            if cancel_job_id == job_id && cancel_token == token {
                                                log::info!("cancelling job {}", job_id);
                                                job_cancelled = true;

                                                let _ = send_ws_message(&mut session, Message::JobCancelled { job_id }).await;

                                                break;
                                            } else if !send_ws_message(
                                                &mut session,
                                                Message::Error {
                                                    message: "invalid token or job id for cancellation".to_string(),
                                                },
                                            )
                                            .await
                                            {
                                                break;
                                            }
                                        }
                                    }
                                    Some(Ok(AggregatedMessage::Ping(payload))) => {
                                        if session.pong(&payload).await.is_err() {
                                            break;
                                        }
                                    }
                                    Some(Ok(AggregatedMessage::Pong(_))) => {}
                                    // close or transport error/end, stop converting
                                    Some(Ok(AggregatedMessage::Close(_))) | Some(Err(_)) | None => break,
                                    Some(Ok(_)) => {}
                                }
                            }
                        }
                    }

                    if !conversion_finished {
                        let process_to_kill = {
                            let mut app_state = APP_STATE.lock().await;
                            app_state.active_processes.remove(&job_id)
                        };

                        if job_cancelled {
                            log::warn!("job {} cancelled, killing process", job_id);

                            if let Some(mut process) = process_to_kill {
                                if let Err(e) = process.kill().await {
                                    log::error!("failed to kill process for job {}: {}", job_id, e);
                                } else {
                                    log::info!("killed process for job {}", job_id);
                                }
                            }

                            // TODO: possibly allow reconnection to websocket within certain timeframe?
                            // would need VERT ui changes, probably temporarily store job info in browser if refresh/other reasons?
                            {
                                let mut app_state = APP_STATE.lock().await;
                                app_state.jobs.remove(&job_id);
                            }

                            if let Err(e) = remove_job_input(&job).await {
                                if e.kind() != ErrorKind::NotFound {
                                    log::error!(
                                        "failed to remove input file after cancellation: {}",
                                        e
                                    );
                                }
                            }

                            break 'conversion;
                        }

                        log::warn!(
                            "client disconnected while watching job {}, letting it finish",
                            job_id
                        );

                        match process_to_kill {
                            Some(process) => {
                                process_opt = Some(process);
                            }
                            None => {
                                // nothing left to wait on, the process already exited
                                log::warn!("job {} has no process to wait for", job_id);
                                break 'conversion;
                            }
                        }
                    }

                    // check process exit status to update job state
                    let process = {
                        let mut app_state = APP_STATE.lock().await;
                        app_state.active_processes.remove(&job_id)
                    };
                    let process_succeeded = if let Some(mut process) = process {
                        let exit = loop {
                            tokio::select! {
                                exit = process.wait() => break exit,
                                message = stream.next() => {
                                    let mut terminate = false;
                                    match &message {
                                        Some(Ok(AggregatedMessage::Text(text))) => {
                                            let cancel = matches!(serde_json::from_str::<Message>(text),
                                                Ok(Message::CancelJob { token: cancel_token, job_id: cancel_id })
                                                if cancel_token == token && cancel_id == job_id);
                                            if cancel {
                                                job_cancelled = true;
                                                terminate = true;
                                            }
                                        }
                                        Some(Ok(AggregatedMessage::Ping(payload))) => {
                                            if session.pong(payload).await.is_err() {
                                                terminate = true;
                                            }
                                        }
                                        Some(Ok(AggregatedMessage::Pong(_))) => {}
                                        Some(Ok(AggregatedMessage::Close(_))) | Some(Err(_)) | None => {
                                            terminate = true;
                                        }
                                        Some(Ok(_)) => {}
                                    }
                                    if terminate {
                                        if job_cancelled {
                                            if let Err(e) = process.kill().await {
                                                log::error!("failed to kill waiting process for job {}: {}", job_id, e);
                                            }
                                            let _ = send_ws_message(&mut session, Message::JobCancelled { job_id }).await;
                                        } else {
                                            log::warn!(
                                                "client disconnected while waiting on job {}, letting it finish",
                                                job_id
                                            );
                                        }
                                        break process.wait().await;
                                    }
                                }
                            }
                        };
                        match exit {
                            Ok(status) => {
                                if !status.success() {
                                    let error = format!("FFmpeg exited with {}", status);
                                    push_log(retry.current_logs(), error, 1).await;
                                }
                                status.success()
                            }
                            Err(e) => {
                                log::error!("failed to wait for job {}: {}", job_id, e);
                                false
                            }
                        }
                    } else {
                        false
                    };

                    if job_cancelled {
                        APP_STATE.lock().await.jobs.remove(&job_id);
                        if let Err(e) = remove_job_input(&job).await {
                            if e.kind() != ErrorKind::NotFound {
                                log::error!(
                                    "failed to remove cancelled input for job {}: {}",
                                    job_id,
                                    e
                                );
                            }
                        }
                        break 'conversion;
                    }

                    // check if output/{}.{} exists and isn't empty
                    let output_path = format!("output/{}.{}", job_id, to);
                    let is_empty = fs::metadata(&output_path)
                        .await
                        .map(|m| m.len() == 0)
                        .unwrap_or(true);

                    // check if output actually has a video stream
                    let expects_video = !matches!(
                        to,
                        ConverterFormat::GIF | ConverterFormat::WEBP | ConverterFormat::APNG
                    ) && job.sequence_kind()
                        != Some(SequenceKind::FrameSequence);
                    let missing_video = !is_empty
                        && expects_video
                        && !has_video_stream(&output_path).await.unwrap_or(true);

                    if missing_video {
                        push_log(
                            retry.current_logs(),
                            "output has no video stream (the input may be corrupt or undecodable)"
                                .to_string(),
                            1,
                        )
                        .await;
                    }

                    if is_empty || missing_video || !process_succeeded {
                        if let Some((new_rx, new_process)) = retry
                            .start_next(&converter, &mut job, vaapi_device_path.as_deref())
                            .await
                        {
                            rx = new_rx;
                            process_opt = Some(new_process);
                            try_send_ws_message(&mut session, Message::JobRetried { job_id }).await;
                            continue 'conversion;
                        }

                        // nothing left to try, finally give up </3
                        let retries = retry.retries();
                        log::error!(
                            "job {} failed after {} retr{}",
                            job_id,
                            retries,
                            if retries == 1 { "y" } else { "ies" }
                        );

                        let error_message = attempt_report(&retry.attempts).await;
                        fail_job(&mut session, &job, job_id, to, error_message).await;
                    } else {
                        if let Some(job) = APP_STATE.lock().await.jobs.get_mut(&job_id) {
                            job.state = JobState::Completed;
                        }
                        if !send_ws_message(&mut session, Message::JobFinished { job_id }).await {
                            break 'conversion;
                        }
                    }

                    break 'conversion;
                }

                // kill running process if it is still somehow running
                let active_process = APP_STATE.lock().await.active_processes.remove(&job_id);
                if let Some(mut process) = process_opt.take().or(active_process) {
                    if let Err(e) = process.kill().await {
                        log::error!("failed to kill process for job {}: {}", job_id, e);
                    }
                }
                {
                    let mut app_state = APP_STATE.lock().await;
                    if let Some(job) = app_state.jobs.get_mut(&job_id) {
                        if job.processing() {
                            job.state = JobState::Failed;
                        }
                    }
                }

                tokio::spawn(async move {
                    // wait 15 seconds to let the user decide if they want to keep the file,
                    // and also for the copy op to finish...
                    tokio::time::sleep(tokio::time::Duration::from_secs(15)).await;
                    match remove_job_input(&job).await {
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

const WEBHOOK_MAX_ATTEMPTS: usize = 5;

fn discord_retry_after(error: &str) -> Option<u64> {
    // "Format Error: {"message": "You are being rate limited.", "retry_after": 1.25, "global": false}"
    if !error.contains("rate limited") {
        return None;
    }
    let (_, rest) = error.split_once("\"retry_after\"")?;
    let (_, value) = rest.split_once(':')?;
    let number: String = value
        .trim_start()
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    let seconds = number.parse::<f64>().ok()?;
    Some((seconds * 1000.0).ceil() as u64)
}

async fn send_webhook_with_retry(
    client: &DiscordWebhook,
    message: &message::Message,
    files: &BTreeMap<String, Vec<u8>>,
) -> anyhow::Result<()> {
    for attempt in 1..=WEBHOOK_MAX_ATTEMPTS {
        match client.send_with_files(message, files.clone()).await {
            Ok(_) => return Ok(()),
            Err(e) => {
                let error = e.to_string();
                let Some(retry_after_ms) = discord_retry_after(&error) else {
                    // not a rate limit, retrying won't help
                    return Err(anyhow::anyhow!("{error}"));
                };

                if attempt == WEBHOOK_MAX_ATTEMPTS {
                    return Err(anyhow::anyhow!(
                        "discord is still rate limiting after {} attempts: {}",
                        WEBHOOK_MAX_ATTEMPTS,
                        error
                    ));
                }

                let delay = tokio::time::Duration::from_millis(retry_after_ms.max(250));
                log::warn!(
                    "discord webhook rate limited, retrying in {}ms (attempt {}/{})",
                    delay.as_millis(),
                    attempt,
                    WEBHOOK_MAX_ATTEMPTS
                );
                tokio::time::sleep(delay).await;
            }
        }
    }

    Ok(())
}

async fn handle_job_failure(
    job_id: Uuid,
    from: String,
    to: String,
    logs: String,
) -> anyhow::Result<()> {
    let client_url = std::env::var("WEBHOOK_URL").unwrap_or_default();
    anyhow::ensure!(
        !client_url.trim().is_empty(),
        "WEBHOOK_URL is not configured"
    );
    let mentions = std::env::var("WEBHOOK_PINGS").unwrap_or_else(|_| "".to_string());

    let mut files = BTreeMap::new();
    files.insert(format!("{}.log", job_id), logs.into_bytes());

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

    send_webhook_with_retry(&client, &message, &files).await?;

    Ok(())
}
