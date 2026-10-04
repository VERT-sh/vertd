use crate::{
    converter::{format::ConverterFormat, job::Job},
    http::response::ApiResponse,
    state::APP_STATE,
    MAX_UPLOAD_BYTES,
};
use actix_multipart::Multipart;
use actix_web::{post, HttpResponse, Responder, ResponseError};
use futures_util::StreamExt as _;
use log::{info, warn};
use tokio::{
    fs::{self, File},
    io::AsyncWriteExt,
};

#[derive(Debug, thiserror::Error)]
pub enum UploadError {
    #[error("no file uploaded")]
    NoFile,
    #[error("failed to get field")]
    GetField(#[from] actix_multipart::MultipartError),
    #[error("no filename provided")]
    NoFilename,
    #[error("missing file extension")]
    NoExtension,
    #[error("invalid file extension: {0}. allowed: jpg, png, gif")]
    InvalidExtension(String),
    #[error("failed to read file data")]
    GetChunk(#[from] actix_web::Error),
    #[error("internal server error while writing file")]
    WriteFile(#[from] std::io::Error),
    #[error("ffprobe failed to read file: {0}")]
    ParseFile(#[from] anyhow::Error),
    #[error("uploaded file exceeds the maximum allowed size of {limit} bytes")]
    PayloadTooLarge { limit: usize },
}

impl ResponseError for UploadError {
    fn error_response(&self) -> HttpResponse {
        // change these status codes as needed
        let status = match self {
            UploadError::GetField(_) => actix_web::http::StatusCode::BAD_REQUEST,
            UploadError::GetChunk(_) => actix_web::http::StatusCode::INTERNAL_SERVER_ERROR,
            UploadError::WriteFile(_) => actix_web::http::StatusCode::INTERNAL_SERVER_ERROR,
            UploadError::PayloadTooLarge { .. } => actix_web::http::StatusCode::PAYLOAD_TOO_LARGE,
            _ => actix_web::http::StatusCode::BAD_REQUEST,
        };

        HttpResponse::build(status).json(ApiResponse::<()>::Error(self.to_string()))
    }
}

// deletes the incomplete upload file if the request is dropped before the upload is complete
struct PendingUpload {
    path: String,
    file: Option<File>,
    registered: bool,
}

impl Drop for PendingUpload {
    fn drop(&mut self) {
        let file = self.file.take();
        if !self.registered {
            let path = self.path.clone();
            tokio::spawn(async move {
                if let Some(file) = file {
                    drop(file.into_std().await);
                    log::warn!(
                        "file upload interrupted, deleting incomplete upload: {}",
                        path
                    );
                }
                if let Err(e) = fs::remove_file(&path).await {
                    if e.kind() != std::io::ErrorKind::NotFound {
                        log::error!("failed to remove incomplete upload {}: {}", path, e);
                    }
                }
            });
        }
    }
}

#[post("/upload")]
pub async fn upload(mut payload: Multipart) -> Result<impl Responder, UploadError> {
    let mut job: Option<Job> = None;
    while let Some(item) = payload.next().await {
        let mut field = item?;

        if field.content_disposition().is_none() {
            continue;
        }

        let content_disposition = field.content_disposition().unwrap();
        if content_disposition.get_name() != Some("file") {
            continue;
        }

        // get file name
        let filename = content_disposition
            .get_filename()
            .ok_or_else(|| UploadError::NoFilename)?
            .to_owned();

        let ext = filename
            .split('.')
            .next_back()
            .map(|ext| {
                ext.chars()
                    .filter(|c| c.is_alphanumeric())
                    .collect::<String>()
            })
            .ok_or_else(|| UploadError::NoExtension)?;

        if let Err(e) = ext.parse::<ConverterFormat>() {
            log::error!("failed to parse file extension: {}", e);
            return Err(UploadError::InvalidExtension(ext));
        }

        let rand: [u8; 64] = rand::random();
        let token = hex::encode(rand);
        let our_job = Job::new(token, ext.to_string());

        info!("new file upload: {}.{}", our_job.id, ext);

        let input_path = format!("input/{}.{}", our_job.id, ext);
        let mut pending = PendingUpload {
            path: input_path.clone(),
            file: None,
            registered: false,
        };
        pending.file = Some(File::from_std(std::fs::File::create(&input_path)?));
        let mut uploaded_bytes = 0usize;
        while let Some(chunk) = field.next().await {
            let data = chunk?;
            uploaded_bytes += data.len();
            if let Some(limit) = *MAX_UPLOAD_BYTES {
                if uploaded_bytes > limit {
                    warn!(
                        "uploaded file {} exceeded max size limit ({} bytes), rejecting upload",
                        our_job.id, limit
                    );
                    return Err(UploadError::PayloadTooLarge { limit });
                }
            }

            pending.file.as_mut().unwrap().write_all(&data).await?;
        }

        pending.file.as_mut().unwrap().flush().await?;
        drop(pending.file.take());

        info!(
            "file uploaded successfully ({} bytes): {}",
            uploaded_bytes, our_job.id
        );

        let mut validated = our_job.clone();
        if let Err(e) = validated.total_frames().await {
            warn!("failed to validate uploaded file {}: {}", our_job.id, e);
            return Err(UploadError::ParseFile(e));
        }
        job = Some(validated);

        {
            let mut app_state = APP_STATE.lock().await;
            app_state.jobs.insert(our_job.id, our_job.clone());
        }
        // spawn a new task which waits an hour before removing the job
        tokio::spawn(async move {
            tokio::time::sleep(crate::INPUT_LIFETIME).await;
            info!(
                "{:?} elapsed, removing {}",
                crate::INPUT_LIFETIME,
                our_job.id
            );
            let removed = {
                let mut app_state = APP_STATE.lock().await;
                app_state.jobs.remove(&our_job.id)
            };
            if removed.is_some() {
                if let Err(e) = fs::remove_file(format!("input/{}.{}", our_job.id, ext)).await {
                    if e.kind() != std::io::ErrorKind::NotFound {
                        log::error!("failed to remove expired input {}: {}", our_job.id, e);
                    }
                }
            }
        });
        pending.registered = true; // file fully uploaded, so marked as registered to avoid deletion
        break;
    }
    let job = job.ok_or_else(|| UploadError::NoFile)?;
    Ok(ApiResponse::Success(job))
}
