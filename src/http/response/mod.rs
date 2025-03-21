use actix_web::{HttpResponse, Responder};
use serde::Serialize;
use serde_with::skip_serializing_none;

#[derive(Serialize)]
#[serde(tag = "type", content = "data")]
#[serde(rename_all = "camelCase")]
#[skip_serializing_none]
pub enum ApiResponse<T> {
    #[serde(rename = "success")]
    Success(T),
    #[serde(rename = "error")]
    Error(String),
}

impl<T: Serialize> Responder for ApiResponse<T> {
    type Body = actix_web::body::BoxBody;
    fn respond_to(self, _req: &actix_web::HttpRequest) -> HttpResponse<Self::Body> {
        HttpResponse::Ok().json(self)
    }
}
