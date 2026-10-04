// get /download/{id} where id is Uuid

use actix_web::{post, web::Json, HttpResponse, Responder, ResponseError};
use discord_webhook2::{message, webhook::DiscordWebhook};
use serde::Deserialize;
use tokio::fs;
use uuid::Uuid;

use crate::{http::response::ApiResponse, state::APP_STATE};

#[derive(Debug, thiserror::Error)]
pub enum KeepError {
    #[error("job not found")]
    JobNotFound,
    #[error("invalid token")]
    InvalidToken,
    #[error("job is not in an error state")]
    NotErrored,
    #[error(
        "keeping files is not configured (missing WEBHOOK_URL, PUBLIC_URL, or ADMIN_PASSWORD)"
    )]
    NotConfigured,
    #[error("filesystem error: {0}")]
    FilesystemError(#[from] std::io::Error),
}

#[derive(Debug, Deserialize)]
pub struct KeepRequest {
    pub id: Uuid,
    pub token: String,
}

impl ResponseError for KeepError {
    fn error_response(&self) -> HttpResponse {
        let status = match self {
            KeepError::JobNotFound => actix_web::http::StatusCode::NOT_FOUND,
            KeepError::NotErrored => actix_web::http::StatusCode::BAD_REQUEST,
            KeepError::InvalidToken => actix_web::http::StatusCode::UNAUTHORIZED,
            KeepError::NotConfigured => actix_web::http::StatusCode::SERVICE_UNAVAILABLE,
            KeepError::FilesystemError(_) => actix_web::http::StatusCode::INTERNAL_SERVER_ERROR,
        };

        HttpResponse::build(status).json(ApiResponse::<()>::Error(self.to_string()))
    }
}

fn keep_configuration() -> Result<(String, String, String), KeepError> {
    let webhook_url = std::env::var("WEBHOOK_URL")
        .ok()
        .filter(|v| !v.is_empty())
        .ok_or(KeepError::NotConfigured)?;
    let public_url = std::env::var("PUBLIC_URL")
        .ok()
        .filter(|v| !v.is_empty())
        .ok_or(KeepError::NotConfigured)?;
    // empty/default admin password disables download (see download.rs), so reject it
    let admin_password = std::env::var("ADMIN_PASSWORD")
        .ok()
        .filter(|v| !v.is_empty() && v != "supersecret")
        .ok_or(KeepError::NotConfigured)?;
    Ok((webhook_url, public_url, admin_password))
}

// i am only now starting to realise how poorly designed i made this api
#[post("/keep")]
pub async fn keep(body: Json<KeepRequest>) -> Result<impl Responder, KeepError> {
    let body = body.into_inner();
    let (job_id, job_from, job_auth, job_errored) = {
        let app_state = APP_STATE.lock().await;
        let job = app_state.jobs.get(&body.id).ok_or(KeepError::JobNotFound)?;
        (job.id, job.from.clone(), job.auth.clone(), job.errored())
    };

    if !job_errored {
        return Err(KeepError::NotErrored);
    }

    if job_auth != body.token {
        return Err(KeepError::InvalidToken);
    }

    // fail before moving the file if we can't build a usable download link
    let (webhook_url, public_url, admin_password) = keep_configuration()?;

    // move the file from temp to permanent storage
    let current_path = format!("input/{}.{}", job_id, job_from);
    let permanent_path = format!("permanent/{}.{}", job_id, job_from);
    log::debug!(
        "moving file to permanent storage: {} -> {}",
        current_path,
        permanent_path
    );
    fs::rename(&current_path, &permanent_path).await?;
    log::info!("moved file to permanent storage: {}", permanent_path);

    let id = job_id;
    let from = job_from;

    tokio::spawn(async move {
        if let Err(e) = webhook_permanent(id, from, webhook_url, public_url, admin_password).await {
            log::error!("failed to send permanent webhook: {}", e);
        }
    });

    Ok("{}")
}

async fn webhook_permanent(
    id: Uuid,
    from: String,
    webhook_url: String,
    public_url: String,
    admin_password: String,
) -> anyhow::Result<()> {
    let webhook_pings = std::env::var("WEBHOOK_PINGS").unwrap_or_default();

    let file_url = format!("{public_url}/api/download/{id}.{from}/{admin_password}");

    let client = DiscordWebhook::new(webhook_url)?;

    let message = message::Message::new(|m| {
        m.content(format!("🚨🚨🚨 {webhook_pings}")).embed(|e| {
            e.title("a file has been kept permanently!")
                .description(format!("download it [here]({file_url}). please note that the link contains a secret token, and also that the file is deleted upon first (successful full) download, so please agree on whoever downloads it first."))
                .color(0xff83fa)
        })
    });

    client.send(&message).await?;

    Ok(())
}
