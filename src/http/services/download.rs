// get /download/{id} where id is Uuid

use actix_web::{get, web, HttpResponse, Responder, ResponseError};
use tokio::{fs, time, time::Duration};
use tokio_util::io::ReaderStream;

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
        // prevent path traversal by checking if valid UUID
        let id_no_ext = id.split('.').next().unwrap_or(&id);
        if uuid::Uuid::parse_str(id_no_ext).is_err() {
            log::warn!("invalid UUID for download: {id}");
            return Err(DownloadError::JobNotFound);
        }
        format!("permanent/{id}")
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

    let file = fs::File::open(&file_path).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            DownloadError::JobNotFound
        } else {
            DownloadError::FilesystemError(e)
        }
    })?;

    let metadata = file.metadata().await.map_err(DownloadError::FilesystemError)?;
    let file_size = metadata.len();

    let stream = ReaderStream::new(file);

    let file_path_clone = file_path.clone();
    tokio::spawn(async move {
        time::sleep(Duration::from_secs(30)).await;
        if let Err(e) = fs::remove_file(file_path_clone).await {
            log::warn!("failed to delete file after 30s: {e}");
        }
    });

    Ok(HttpResponse::Ok()
        .insert_header(("Content-Type", "application/octet-stream"))
        .insert_header(("Content-Length", file_size))
        .streaming(stream))
}
