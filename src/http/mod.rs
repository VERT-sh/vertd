use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener};

use actix_cors::Cors;
use actix_web::{web, App, HttpServer};
use log::{info, warn};
use services::{download::download, upload::upload, version::version, websocket::websocket};
use socket2::{Domain, Protocol, Socket, Type};

use crate::http::services::keep::keep;

mod response;
mod services;

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
    let port: u16 = port
        .parse()
        .map_err(|_| anyhow::anyhow!("PORT must be a number between 0 and 65535"))?;
    let listener = bind_dual_stack(port)?;
    info!("http server listening on {}", listener.local_addr()?);
    server.listen(listener)?.run().await?;
    Ok(())
}
