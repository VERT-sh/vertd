use crate::{
    http::response::ApiResponse,
    job::{
        types::{CompressionJob, ConversionJob, ConverterFormat},
        Job, JobTrait as _, JobType,
    },
    state::APP_STATE,
};
use actix_multipart::form::{json::Json as MpJson, tempfile::TempFile, MultipartForm};
use actix_web::{post, HttpResponse, Responder, ResponseError};
use serde::Deserialize;
use std::io::Read;
use tokio::fs;

#[derive(Debug, thiserror::Error)]
pub enum UploadError {
    #[error("failed to get field")]
    GetField(#[from] actix_multipart::MultipartError),
    #[error("no filename provided")]
    NoFilename,
    #[error("missing file extension")]
    NoExtension,
    #[error("invalid file extension: {0}. allowed: jpg, png, gif")]
    InvalidExtension(String),
    #[error("failed to read file data")]
    GetChunk,
    #[error("internal server error while writing file")]
    WriteFile(#[from] std::io::Error),
    #[error("ffprobe failed to read file: {0}")]
    ParseFile(#[from] anyhow::Error),
}

impl ResponseError for UploadError {
    fn error_response(&self) -> HttpResponse {
        // change these status codes as needed
        let status = match self {
            UploadError::GetField(_) => actix_web::http::StatusCode::BAD_REQUEST,
            UploadError::GetChunk => actix_web::http::StatusCode::INTERNAL_SERVER_ERROR,
            UploadError::WriteFile(_) => actix_web::http::StatusCode::INTERNAL_SERVER_ERROR,
            _ => actix_web::http::StatusCode::BAD_REQUEST,
        };

        HttpResponse::build(status).json(ApiResponse::<()>::Error(self.to_string()))
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FileMetadata {
    job_type: JobType,
}

#[derive(Debug, MultipartForm)]
struct UploadForm {
    #[multipart]
    file: TempFile,
    json: MpJson<FileMetadata>,
}

#[post("/upload")]
pub async fn upload(
    MultipartForm(form): MultipartForm<UploadForm>,
) -> Result<impl Responder, UploadError> {
    let mut app_state = APP_STATE.lock().await;

    let (id, job): (_, Job) = match form.json.job_type {
        JobType::Conversion => {
            let filename = form.file.file_name.ok_or_else(|| UploadError::NoFilename)?;
            let ext = filename
                .split('.')
                .last()
                .and_then(|ext| {
                    Some(
                        ext.chars()
                            .filter(|c| c.is_alphanumeric())
                            .collect::<String>(),
                    )
                })
                .ok_or_else(|| UploadError::NoExtension)?;
            let ext = ext
                .parse::<ConverterFormat>()
                .map_err(|_| UploadError::InvalidExtension(ext))?;

            let rand: [u8; 64] = rand::random();
            let token = hex::encode(rand);
            let mut job = ConversionJob::new(token, ext.to_string());
            let buf = tokio::task::spawn_blocking(async move || {
                let mut buf = Vec::with_capacity(form.file.size);
                let mut reader = form.file.file;
                reader
                    .read_to_end(&mut buf)
                    .expect("failed to read file data");
                buf
            })
            .await
            .map_err(|_| UploadError::GetChunk)?
            .await;
            fs::write(format!("input/{}.{}", job.id, ext), &buf).await?;
            job.total_frames().await?;
            (job.id, job.into())
        }

        JobType::Compression => {
            let rand: [u8; 64] = rand::random();
            let token = hex::encode(rand);
            let filename = form.file.file_name.ok_or_else(|| UploadError::NoFilename)?;
            let ext = filename
                .split('.')
                .last()
                .and_then(|ext| {
                    Some(
                        ext.chars()
                            .filter(|c| c.is_alphanumeric())
                            .collect::<String>(),
                    )
                })
                .ok_or_else(|| UploadError::NoExtension)?;
            let job = CompressionJob::new(token, ext);
            (job.id, job.into())
        }
    };

    let job_type = job.as_ref().to_lowercase();
    log::info!("uploaded {} job {}", job_type, job.id());

    app_state.jobs.insert(id, job.clone());

    Ok(ApiResponse::Success(job))
}
