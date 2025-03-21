use actix_web::{get, Responder};

#[get("/ws")]
pub async fn websocket() -> impl Responder {
    ""
}
