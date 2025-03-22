use std::sync::Arc;

use actix_web::{get, rt, web, HttpRequest, Responder};
use actix_ws::{AggregatedMessage, Message};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::sync::Notify;

use crate::{
    job::{Job, JobTrait},
    state::APP_STATE,
};

#[macro_export]
macro_rules! send_message {
    ($session:expr, $msg:expr) => {{
        let msg: String = $msg.into();
        $session.text(msg)
    }};
}

#[get("/ws")]
pub async fn websocket(req: HttpRequest, body: web::Payload) -> actix_web::Result<impl Responder> {
    let (response, mut session, stream) = actix_ws::handle(&req, body)?;
    let mut stream = stream
        .aggregate_continuations()
        .max_continuation_size(2_usize.pow(50));

    rt::spawn(async move {
        let mut job: Option<Job> = None;
        while let Some(Ok(msg)) = stream.next().await {
            let Ok(new_job) = wait_for_auth(&mut session, msg).await else {
                break;
            };
            if let Some(new_job) = new_job {
                job = Some(new_job);
                break;
            }
        }
        let job = job.ok_or_else(|| anyhow::anyhow!("no job found"))?;
        log::info!("job found: {}", job.id());
        handle_job(job, session, stream).await?;
        Ok::<(), anyhow::Error>(())
    });
    Ok(response)
}

async fn handle_job(
    mut job: Job,
    mut session: actix_ws::Session,
    stream: actix_ws::AggregatedMessageStream,
) -> anyhow::Result<()> {
    if job.completed() {
        send_message!(
            session,
            AuthStateMessage::Error {
                message: "job already completed".to_string(),
            }
        )
        .await?;
        return Ok(());
    }
    job.handle_ws(session, stream).await?;
    // insert or replace the job in the app state
    let mut app_state = APP_STATE.lock().await;
    if app_state.jobs.contains_key(&job.id()) {
        app_state.jobs.remove(&job.id());
    }
    app_state.jobs.insert(job.id(), job);
    Ok(())
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(tag = "type")]
enum AuthStateMessage {
    Hello { auth: String },
    Error { message: String },
}

impl Into<String> for AuthStateMessage {
    fn into(self) -> String {
        serde_json::to_string(&self).unwrap()
    }
}

async fn wait_for_auth(
    session: &mut actix_ws::Session,
    msg: AggregatedMessage,
) -> anyhow::Result<Option<Job>> {
    match msg {
        AggregatedMessage::Close(_) => {
            return Ok(None);
        }

        AggregatedMessage::Text(text) => {
            let msg: AuthStateMessage = serde_json::from_str(&text)?;
            match msg {
                AuthStateMessage::Hello { auth } => {
                    let app_state = APP_STATE.lock().await;
                    let job = app_state
                        .jobs
                        .iter()
                        .find(|(_, v)| v.auth() == auth)
                        .map(|(_, v)| v);
                    let job = job.cloned();
                    if job.is_none() {
                        send_message!(
                            session,
                            AuthStateMessage::Error {
                                message: "invalid auth".to_string(),
                            }
                        )
                        .await?;
                    }

                    return Ok(job);
                }

                _ => Ok(None),
            }
        }

        AggregatedMessage::Ping(ref msg) => {
            session.pong(msg).await?;
            return Ok(None);
        }

        _ => Ok(None),
    }
}
