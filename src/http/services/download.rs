use actix_web::{get, web, HttpResponse, Responder, ResponseError};
use uuid::Uuid;

use crate::http::response::ApiResponse;

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
pub async fn download(path: web::Path<(Uuid, String)>) -> Result<impl Responder, DownloadError> {
    Ok(ApiResponse::Success(()))
}
