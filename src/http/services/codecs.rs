use actix_web::{get, web, Responder};
use std::str::FromStr;

use crate::converter::{
    codecs::{all_supported_codecs, format_support, formats_for_codec},
    format::ConverterFormat,
};
use crate::http::response::ApiResponse;

#[get("/codecs")]
pub async fn codecs() -> impl Responder {
    ApiResponse::Success(all_supported_codecs())
}

#[get("/codecs/{format}")]
pub async fn codec(format: web::Path<String>) -> impl Responder {
    let format = format.into_inner().to_lowercase();
    let Ok(format) = ConverterFormat::from_str(&format) else {
        return ApiResponse::Error(format!("unsupported format: {}", format));
    };

    if let Some(support) = format_support(format) {
        return ApiResponse::Success(support);
    }

    ApiResponse::Error(format!(
        "format exists but has no codec map yet: {}",
        format.to_string()
    ))
}

#[get("/codecs/support/{codec}")]
pub async fn codec_support(codec_name: web::Path<String>) -> impl Responder {
    ApiResponse::Success(formats_for_codec(&codec_name.into_inner()))
}
