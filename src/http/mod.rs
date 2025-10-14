use actix_cors::Cors;
use actix_web::{web, App, HttpServer};
use log::info;
use services::{download::download, upload::upload, version::version, websocket::websocket};

use crate::http::services::keep::keep;

mod response;
mod services;

pub async fn start_http() -> anyhow::Result<()> {
    let server = HttpServer::new(|| {
        App::new()
            .wrap(
                Cors::default()
                    .allow_any_origin()
                    .allow_any_method()
                    .allow_any_header(),
            )
            .service(
                web::scope("/api")
                    .service(upload)
                    .service(download)
                    .service(websocket)
                    .service(version)
                    .service(keep),
            )
    });
    let port = std::env::var("PORT").unwrap_or_else(|_| "24153".to_string());
    if !port.chars().all(char::is_numeric) {
        anyhow::bail!("PORT must be a number");
    }
    let ip = format!("0.0.0.0:{}", port);
    info!("http server listening on {}", ip);
    server.bind(ip)?.run().await?;
    Ok(())
}
