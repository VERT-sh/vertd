use actix_cors::Cors;
use actix_web::{web, App, HttpServer};
use log::info;
use services::{
    codecs::{codec, codec_support, codecs},
    confirm::confirm,
    download::download,
    info::{size_limit, version},
    upload::upload,
    websocket::websocket,
};

use crate::http::services::keep::keep;

mod response;
mod services;

pub async fn start_http() -> anyhow::Result<actix_web::dev::Server> {
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
                    .service(confirm)
                    .service(websocket)
                    .service(version)
                    .service(size_limit)
                    .service(codecs)
                    .service(codec)
                    .service(codec_support)
                    .service(keep),
            )
    });
    let port = std::env::var("PORT").unwrap_or_else(|_| "24153".to_string());
    if !port.chars().all(char::is_numeric) {
        anyhow::bail!("PORT must be a number");
    }
    let ip = format!("0.0.0.0:{}", port);
    info!("http server listening on {}", ip);
    let server = server.bind(ip)?;
    Ok(server.run())
}
