// get /download/{id} where id is Uuid

use actix_web::{get, web, HttpResponse, Responder, ResponseError};
use tokio::fs;

use crate::{http::response::ApiResponse, state::APP_STATE};

#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
    #[error("job not found")]
    JobNotFound,
    #[error("incomplete websocket handshake")]
    IncompleteHandshake,
    #[error("invalid token")]
    InvalidToken,
    #[error("filesystem error: {0}")]
    FilesystemError(#[from] std::io::Error),
}

impl ResponseError for DownloadError {
    fn error_response(&self) -> HttpResponse {
        let status = match self {
            DownloadError::JobNotFound => actix_web::http::StatusCode::NOT_FOUND,
            DownloadError::IncompleteHandshake => actix_web::http::StatusCode::BAD_REQUEST,
            DownloadError::InvalidToken => actix_web::http::StatusCode::UNAUTHORIZED,
            DownloadError::FilesystemError(_) => actix_web::http::StatusCode::INTERNAL_SERVER_ERROR,
        };

        HttpResponse::build(status).json(ApiResponse::<()>::Error(self.to_string()))
    }
}

#[get("/download/{id}/{token}")]
pub async fn download(path: web::Path<(String, String)>) -> Result<impl Responder, DownloadError> {
    let (id, token) = path.into_inner();

    let is_admin = std::env::var("ADMIN_PASSWORD")
        .ok()
        .is_some_and(|p| p == token && !p.is_empty() && p != "supersecret"); // disable admin if password is empty or default

    let file_path = if is_admin {
        log::warn!("admin download used for id {id}");
        format!("permanent/{id}") // this would be vulnerable to path traversal but it's behind a password so who cares
    } else {
        let id = id.parse().map_err(|_| DownloadError::JobNotFound)?;
        let app_state = APP_STATE.lock().await;
        let job = app_state
            .jobs
            .get(&id)
            .ok_or(DownloadError::JobNotFound)?
            .clone();
        drop(app_state);

        if job.auth != token && !is_admin {
            return Err(DownloadError::InvalidToken);
        }

        let file_path = match job.to {
            Some(to) => format!("output/{id}.{to}"),
            None => return Err(DownloadError::IncompleteHandshake),
        };

        let mut app_state = APP_STATE.lock().await;
        app_state.jobs.remove(&id);
        drop(app_state);
        file_path
    };

    let bytes = fs::read(&file_path).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            DownloadError::JobNotFound
        } else {
            DownloadError::FilesystemError(e)
        }
    })?;

    let mime = mime_guess::from_path(&file_path)
        .first_or_octet_stream()
        .to_string();

    fs::remove_file(file_path)
        .await
        .map_err(DownloadError::FilesystemError)?;

    Ok(HttpResponse::Ok()
        .insert_header(("Content-Type", mime))
        .insert_header(("Content-Length", bytes.len()))
        .body(bytes))
}
