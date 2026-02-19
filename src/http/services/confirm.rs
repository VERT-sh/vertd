use crate::{http::response::ApiResponse, state::APP_STATE};
use actix_web::{get, web, HttpResponse, Responder, ResponseError};
use log::{info, warn};
use std::time::Duration;
use tokio::fs;

#[derive(Debug, thiserror::Error)]
pub enum ConfirmError {
    #[error("job not found")]
    JobNotFound,
    #[error("invalid token")]
    InvalidToken,
    #[error("job not completed")]
    JobNotCompleted,
}

impl ResponseError for ConfirmError {
    fn error_response(&self) -> HttpResponse {
        let status = match self {
            ConfirmError::JobNotFound => actix_web::http::StatusCode::NOT_FOUND,
            ConfirmError::InvalidToken => actix_web::http::StatusCode::UNAUTHORIZED,
            ConfirmError::JobNotCompleted => actix_web::http::StatusCode::BAD_REQUEST,
        };

        HttpResponse::build(status).json(ApiResponse::<()>::Error(self.to_string()))
    }
}

#[get("/confirm/{id}/{token}")]
pub async fn confirm(path: web::Path<(String, String)>) -> Result<impl Responder, ConfirmError> {
    let (id, token) = path.into_inner();
    let id = id.parse().map_err(|_| ConfirmError::JobNotFound)?;

    let mut app_state = APP_STATE.lock().await;
    let job = app_state
        .jobs
        .get(&id)
        .ok_or(ConfirmError::JobNotFound)?
        .clone();

    if job.auth != token {
        drop(app_state);
        return Err(ConfirmError::InvalidToken);
    }

    if !job.completed() {
        drop(app_state);
        return Err(ConfirmError::JobNotCompleted);
    }

    let to = job.to.as_ref().ok_or(ConfirmError::JobNotCompleted)?;
    let file_path = format!("output/{id}.{to}");

    app_state.jobs.remove(&id);
    drop(app_state);

    info!("job {id} confirmed, scheduling deletion in 10s");

    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(10)).await;
        match fs::remove_file(&file_path).await {
            Ok(_) => info!("deleted file {} after download confirmation", file_path),
            Err(e) => warn!("failed to delete file {}: {}", file_path, e),
        }
    });

    Ok(HttpResponse::Ok().json(ApiResponse::Success("download confirmed")))
}
