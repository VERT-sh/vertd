// get /download/{id} where id is Uuid

use actix_web::{get, web, HttpResponse, Responder, ResponseError};
use futures_util::stream::StreamExt;
use tokio::{fs, time, time::Duration};
use tokio_util::io::ReaderStream;
use std::sync::{Arc, atomic};

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

struct StreamGuard {
    file_path: String,
    bytes_sent: Arc<atomic::AtomicU64>,
    file_size: u64,
}

impl Drop for StreamGuard {
    fn drop(&mut self) {
        let total_sent = self.bytes_sent.load(atomic::Ordering::Relaxed);
        let file_path = self.file_path.clone();
        let file_size = self.file_size;

        tokio::spawn(async move {
            if total_sent == file_size {
                log::info!("all bytes successfully sent for {}", file_path);
                time::sleep(Duration::from_secs(30)).await;
                log::info!("removing file after successful download: {}", file_path);
                if let Err(e) = fs::remove_file(&file_path).await {
                    log::error!("failed to remove file: {}", e);
                }
            }
        });
    }
}

#[get("/download/{id}/{token}")]
pub async fn download(path: web::Path<(String, String)>) -> Result<impl Responder, DownloadError> {
    let (id, token) = path.into_inner();

    let is_admin = std::env::var("ADMIN_PASSWORD")
        .ok()
        .is_some_and(|p| p == token && !p.is_empty() && p != "supersecret"); // disable admin if password is empty or default

    let file_path = if is_admin {
        // prevent path traversal by checking if valid UUID
        let id_no_ext = id.split('.').next().unwrap_or(&id);
        if uuid::Uuid::parse_str(id_no_ext).is_err() {
            log::warn!("invalid UUID for download: {id}");
            return Err(DownloadError::JobNotFound);
        }
        log::warn!("admin download used for id {id}");
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

    let metadata = file
        .metadata()
        .await
        .map_err(DownloadError::FilesystemError)?;
    let file_size = metadata.len();
    let bytes_sent = Arc::new(atomic::AtomicU64::new(0));
    let bytes_sent_clone = bytes_sent.clone();

    let file_stream = ReaderStream::new(file);
    let tracked_stream = file_stream.map(move |chunk| {
        if let Ok(ref bytes) = chunk {
            bytes_sent_clone.fetch_add(bytes.len() as u64, atomic::Ordering::Relaxed);
        }
        chunk
    });

    // remove file when stream is dropped
    let guard = StreamGuard {
        file_path: file_path.clone(),
        bytes_sent: bytes_sent.clone(),
        file_size,
    };

    // keep guard alive while streaming
    let http_stream = tracked_stream.inspect(move |_| {
        let _ = &guard;
    });

    Ok(HttpResponse::Ok()
        .insert_header(("Content-Type", "application/octet-stream"))
        .insert_header(("Content-Disposition", format!("attachment; filename=\"{}\"", id)))
        .insert_header(("Content-Length", file_size))
        .streaming(http_stream))
}
