use actix_cors::Cors;
use actix_web::{web, App, HttpServer};
use log::info;
use services::{download::download, upload::upload, version::version, websocket::websocket};

use crate::http::services::keep::keep;

mod response;
mod services;

#[derive(Clone, Debug)]
enum CorsConfig {
    Any,
    Specific(Vec<String>),
}

fn parse_cors(origins_raw: &str) -> CorsConfig {
    let raw = origins_raw.trim();

    if raw.is_empty() || raw == "*" {
        return CorsConfig::Any;
    }

    let origins = raw
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect::<Vec<_>>();

    CorsConfig::Specific(origins)
}

fn build_cors(config: &CorsConfig) -> Cors {
    match config {
        CorsConfig::Any => Cors::default()
            .allow_any_origin()
            .allow_any_method()
            .allow_any_header(),

        CorsConfig::Specific(origins) => {
            let mut cors = Cors::default().allow_any_method().allow_any_header();

            for origin in origins {
                cors = cors.allowed_origin(origin);
            }

            cors
        }
    }
}

pub async fn start_http() -> anyhow::Result<()> {
    let cors_origins = std::env::var("CORS_ORIGINS").unwrap_or_else(|_| "*".to_string());
    let cors_config = parse_cors(&cors_origins);

    match &cors_config {
        CorsConfig::Any => info!("CORS: allow any origin (*)"),
        CorsConfig::Specific(origins) => {
            info!("CORS: allowed origins:");
            for origin in origins {
                info!("  - {}", origin);
            }
        }
    }

    let server = HttpServer::new({
        let cors_config = cors_config.clone();
        move || {
            let cors = build_cors(&cors_config);

            App::new().wrap(cors).service(
                web::scope("/api")
                    .service(upload)
                    .service(download)
                    .service(websocket)
                    .service(version)
                    .service(keep),
            )
        }
    });

    let port = std::env::var("PORT").unwrap_or_else(|_| "24153".to_string());
    if !port.chars().all(char::is_numeric) {
        anyhow::bail!("PORT must be a number");
    }
    let ip = format!("0.0.0.0:{port}");
    info!("http server listening on {}", ip);
    server.bind(ip)?.run().await?;
    Ok(())
}
