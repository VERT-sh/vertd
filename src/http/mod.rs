use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener};

use actix_cors::Cors;
use actix_web::{web, App, HttpServer};
use log::{info, warn};
use services::{
    codecs::{codec, codec_support, codecs},
    confirm::confirm,
    download::download,
    info::{size_limit, version},
    upload::upload,
    websocket::websocket,
};
use socket2::{Domain, Protocol, Socket, Type};

use crate::http::services::keep::keep;

mod response;
mod services;

/// Parsed form of the `CORS_ORIGINS` environment variable.
#[derive(Clone)]
enum CorsConfig {
    Any,
    Specific(Vec<String>),
}

/// Parses a comma-separated list of origins. An empty value or `*` means
/// "allow any origin".
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

/// Builds the actix-cors middleware from the parsed configuration.
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

/// Binds a dual-stack listener on `[::]:port` that accepts both IPv6 and IPv4
/// (via IPv4-mapped addresses). If IPv6 is unavailable (e.g. disabled in the
/// container), falls back to an IPv4-only listener on `0.0.0.0:port`.
fn bind_dual_stack(port: u16) -> std::io::Result<TcpListener> {
    let v6 = (|| -> std::io::Result<Socket> {
        let socket = Socket::new(Domain::IPV6, Type::STREAM, Some(Protocol::TCP))?;
        // Explicitly accept IPv4 too; the OS default differs (e.g. Windows/BSD).
        socket.set_only_v6(false)?;
        #[cfg(unix)]
        socket.set_reuse_address(true)?;
        socket.bind(&SocketAddr::from((Ipv6Addr::UNSPECIFIED, port)).into())?;
        socket.listen(1024)?;
        Ok(socket)
    })();

    let listener = match v6 {
        Ok(socket) => TcpListener::from(socket),
        Err(e) => {
            warn!("IPv6 dual-stack bind failed ({e}); falling back to 0.0.0.0:{port}");
            TcpListener::bind((Ipv4Addr::UNSPECIFIED, port))?
        }
    };
    listener.set_nonblocking(true)?;
    Ok(listener)
}

pub async fn start_http() -> anyhow::Result<actix_web::dev::Server> {
    let cors_origins = std::env::var("CORS_ORIGINS").unwrap_or_else(|_| "*".to_string());
    let cors_config = parse_cors(&cors_origins);

    match &cors_config {
        CorsConfig::Any => info!("CORS: allow any origin (*)"),
        CorsConfig::Specific(origins) => {
            info!("CORS: allowed origins:");
            for origin in origins {
                info!("  - {origin}");
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
                    .service(confirm)
                    .service(websocket)
                    .service(version)
                    .service(size_limit)
                    .service(codecs)
                    .service(codec)
                    .service(codec_support)
                    .service(keep),
            )
        }
    });

    let port = std::env::var("PORT").unwrap_or_else(|_| "24153".to_string());
    let port: u16 = port
        .parse()
        .map_err(|_| anyhow::anyhow!("PORT must be a number between 0 and 65535"))?;
    let listener = bind_dual_stack(port)?;
    info!("http server listening on {}", listener.local_addr()?);
    Ok(server.listen(listener)?.run())
}
