// get /download/{id} where id is Uuid

use crate::{http::response::ApiResponse, state::APP_STATE};
use actix_web::{get, web, HttpResponse, Responder, ResponseError};
use futures_util::StreamExt;
use tokio::fs;
use tokio_util::io::ReaderStream;

#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
    #[error("job not found")]
    JobNotFound,
    #[error("incomplete websocket handshake")]
    IncompleteHandshake,
    #[error("invalid token")]
    InvalidToken,
    #[error("job is not completed")]
    JobNotCompleted,
    #[error("filesystem error: {0}")]
    FilesystemError(#[from] std::io::Error),
}

impl ResponseError for DownloadError {
    fn error_response(&self) -> HttpResponse {
        let status = match self {
            DownloadError::JobNotFound => actix_web::http::StatusCode::NOT_FOUND,
            DownloadError::IncompleteHandshake => actix_web::http::StatusCode::BAD_REQUEST,
            DownloadError::InvalidToken => actix_web::http::StatusCode::UNAUTHORIZED,
            DownloadError::JobNotCompleted => actix_web::http::StatusCode::CONFLICT,
            DownloadError::FilesystemError(_) => actix_web::http::StatusCode::INTERNAL_SERVER_ERROR,
        };

        HttpResponse::build(status).json(ApiResponse::<()>::Error(self.to_string()))
    }
}

struct AdminDownloadGuard {
    path: String,
    size: u64,
    sent: u64,
    failed: bool,
}

impl AdminDownloadGuard {
    fn should_delete(&self) -> bool {
        !self.failed && self.sent == self.size
    }
}

impl Drop for AdminDownloadGuard {
    fn drop(&mut self) {
        if self.should_delete() {
            let path = self.path.clone();
            log::info!(
                "permanent file {} scheduled for deletion in 10 seconds",
                path
            );
            tokio::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                if let Err(e) = fs::remove_file(&path).await {
                    if e.kind() != std::io::ErrorKind::NotFound {
                        log::error!("failed to remove downloaded permanent file {}: {}", path, e);
                    }
                } else {
                    log::info!("removed downloaded permanent file {}", path);
                }
            });
        }
    }
}

#[get("/download/{id}/{token}")]
pub async fn download(path: web::Path<(String, String)>) -> Result<impl Responder, DownloadError> {
    let (id, token) = path.into_inner();

    let is_admin = std::env::var("ADMIN_PASSWORD")
        .ok()
        .is_some_and(|p| p == token && !p.is_empty() && p != "supersecret"); // disable admin if password is empty or default

    let file_path = if is_admin {
        let (raw_uuid, raw_ext) = id.split_once('.').ok_or_else(|| {
            log::warn!("invalid UUID for download: {id}");
            DownloadError::JobNotFound
        })?;

        if raw_uuid.contains('/')
            || raw_uuid.contains('\\')
            || raw_ext.contains('/')
            || raw_ext.contains('\\')
            || raw_ext.is_empty()
            || !raw_ext.chars().all(|c| c.is_ascii_alphanumeric())
        {
            log::warn!("invalid admin filename for download: {id}");
            return Err(DownloadError::JobNotFound);
        }

        let parsed_uuid = uuid::Uuid::parse_str(raw_uuid).map_err(|_| {
            log::warn!("invalid UUID for download: {id}");
            DownloadError::JobNotFound
        })?;

        let sanitized_name = format!("{}.{}", parsed_uuid, raw_ext);
        format!("permanent/{sanitized_name}")
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

        if !job.completed() {
            return Err(DownloadError::JobNotCompleted);
        }

        match job.to {
            Some(to) => format!("output/{id}.{to}"),
            None => return Err(DownloadError::IncompleteHandshake),
        }
    };

    let file = fs::File::open(&file_path).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            DownloadError::JobNotFound
        } else {
            DownloadError::FilesystemError(e)
        }
    })?;

    if is_admin {
        log::warn!("admin download used for id {id}");
    }

    let metadata = file
        .metadata()
        .await
        .map_err(DownloadError::FilesystemError)?;
    let file_size = metadata.len();

    // guard ONLY for admin downloads, deleting after all bytes sent sucessfully
    // regular (user) downloads deleted after confirmed by vert web ui (/confirm/{id})
    let mut guard = is_admin.then(|| AdminDownloadGuard {
        path: file_path,
        size: file_size,
        sent: 0,
        failed: false,
    });
    let stream = ReaderStream::new(file).map(move |chunk| {
        if let Some(guard) = guard.as_mut() {
            match &chunk {
                Ok(bytes) => guard.sent += bytes.len() as u64,
                Err(_) => guard.failed = true,
            }
        }
        chunk
    });

    Ok(HttpResponse::Ok()
        .insert_header(("Content-Type", "application/octet-stream"))
        .insert_header(("Content-Length", file_size))
        .streaming(stream))
}
