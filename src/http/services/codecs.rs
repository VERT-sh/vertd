use actix_web::{get, web, HttpResponse, Responder, ResponseError};
use std::str::FromStr;

use crate::converter::{
    codecs::{all_supported_codecs, format_support, formats_for_codec},
    format::ConverterFormat,
};
use crate::http::response::ApiResponse;

#[derive(Debug, thiserror::Error)]
pub enum CodecError {
    #[error("unsupported format: {0}")]
    UnsupportedFormat(String),
    #[error("format exists but has no codec map yet: {0}")]
    MissingCodecMap(String),
}

impl ResponseError for CodecError {
    fn error_response(&self) -> HttpResponse {
        HttpResponse::BadRequest().json(ApiResponse::<()>::Error(self.to_string()))
    }
}

#[get("/codecs")]
pub async fn codecs() -> impl Responder {
    ApiResponse::Success(all_supported_codecs())
}

#[get("/codecs/{format}")]
pub async fn codec(format: web::Path<String>) -> Result<impl Responder, CodecError> {
    let format = format.into_inner().to_lowercase();
    let Ok(format) = ConverterFormat::from_str(&format) else {
        return Err(CodecError::UnsupportedFormat(format));
    };

    if let Some(support) = format_support(format) {
        return Ok(ApiResponse::Success(support));
    }

    Err(CodecError::MissingCodecMap(format.to_string()))
}

#[get("/codecs/support/{codec}")]
pub async fn codec_support(codec_name: web::Path<String>) -> impl Responder {
    ApiResponse::Success(formats_for_codec(&codec_name.into_inner()))
}
