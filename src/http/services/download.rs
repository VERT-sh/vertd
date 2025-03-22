use actix_web::{get, web, HttpResponse, Responder, ResponseError};
use tokio::fs;

use crate::{http::response::ApiResponse, job::JobTrait as _};

#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
    #[error("job not found")]
    JobNotFound,
    #[error("incomplete websocket handshake")]
    IncompleteHandshake,
    #[error("filesystem error: {0}")]
    FilesystemError(#[from] std::io::Error),
}

impl ResponseError for DownloadError {
    fn error_response(&self) -> HttpResponse {
        let status = match self {
            DownloadError::JobNotFound => actix_web::http::StatusCode::NOT_FOUND,
            DownloadError::IncompleteHandshake => actix_web::http::StatusCode::BAD_REQUEST,
            DownloadError::FilesystemError(_) => actix_web::http::StatusCode::INTERNAL_SERVER_ERROR,
        };

        HttpResponse::build(status).json(ApiResponse::<()>::Error(self.to_string()))
    }
}

#[get("/download/{token}")]
pub async fn download(path: web::Path<String>) -> Result<impl Responder, DownloadError> {
    let token = path.into_inner();
    let app_state = crate::state::APP_STATE.lock().await;
    let job = app_state
        .jobs
        .iter()
        .find_map(|(_, job)| {
            if job.auth() == token {
                Some(job.clone())
            } else {
                None
            }
        })
        .ok_or(DownloadError::JobNotFound)?;
    drop(app_state);

    if !job.completed() {
        return Err(DownloadError::IncompleteHandshake);
    }

    let output_path = job
        .output_path()
        .ok_or(DownloadError::IncompleteHandshake)?;

    let bytes = fs::read(&output_path).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            DownloadError::JobNotFound
        } else {
            DownloadError::FilesystemError(e)
        }
    })?;

    let mime = mime_guess::from_path(&output_path)
        .first_or_octet_stream()
        .to_string();

    fs::remove_file(output_path)
        .await
        .map_err(|e| DownloadError::FilesystemError(e))?;

    Ok(HttpResponse::Ok()
        .insert_header(("Content-Type", mime))
        .insert_header(("Content-Length", bytes.len()))
        .body(bytes))
}
