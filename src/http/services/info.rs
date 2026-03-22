use actix_web::{get, Responder};
use rbtag::BuildInfo;

use crate::{http::response::ApiResponse, MAX_UPLOAD_BYTES};

#[derive(BuildInfo)]
struct BuildTag;

#[get("/version")]
pub async fn version() -> impl Responder {
    let build_tag = BuildTag {}.get_build_commit();
    if build_tag.starts_with("-") {
        return ApiResponse::Success("latest");
    }

    ApiResponse::Success(build_tag)
}

#[get("/size_limit")]
pub async fn size_limit() -> impl Responder {
    ApiResponse::Success(*MAX_UPLOAD_BYTES)
}
