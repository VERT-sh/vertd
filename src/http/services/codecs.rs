use actix_web::{get, web, Responder};
use std::str::FromStr;

use crate::converter::{
    codecs::{all_supported_codecs, container_support, containers_for_codec},
    format::ConverterFormat,
};
use crate::http::response::ApiResponse;

#[get("/codecs")]
pub async fn codecs() -> impl Responder {
    ApiResponse::Success(all_supported_codecs())
}

#[get("/codecs/{container}")]
pub async fn codec(container: web::Path<String>) -> impl Responder {
    let container = container.into_inner().to_lowercase();
    let Ok(format) = ConverterFormat::from_str(&container) else {
        return ApiResponse::Error(format!("unsupported container: {}", container));
    };

    if let Some(support) = container_support(format) {
        return ApiResponse::Success(support);
    }

    ApiResponse::Error(format!(
        "container exists but has no codec map yet: {}",
        container
    ))
}

#[get("/codecs/support/{codec}")]
pub async fn codec_support(codec_name: web::Path<String>) -> impl Responder {
    ApiResponse::Success(containers_for_codec(&codec_name.into_inner()))
}
