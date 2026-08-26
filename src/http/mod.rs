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
    let port_num: u16 = port.parse()?;
    let addr_v6: SocketAddr = format!("[::]:{}", port_num).parse()?;

    let listener = match Socket::new(Domain::IPV6, Type::STREAM, None)
        .and_then(|s| { s.set_only_v6(false)?; s.set_reuse_address(true)?; s.bind(&addr_v6.into())?; s.listen(1024)?; Ok(s) })
    {
        Ok(socket) => {
            info!("http server listening on {} (dual-stack v4/v6)", addr_v6);
            socket.into()
        }
        Err(e) => {
            warn!("dual-stack bind failed ({e}), falling back to IPv4-only 0.0.0.0:{port_num}");
            let addr_v4 = format!("0.0.0.0:{}", port_num);
            std::net::TcpListener::bind(&addr_v4)?
        }
    };

    server.listen(listener)?.run().await?;
    Ok(())
}



    let port_num: u16 = port.parse()?;
    let addr_v6: SocketAddr = format!("[::]:{}", port_num).parse()?;

    let listener = match Socket::new(Domain::IPV6, Type::STREAM, None)
        .and_then(|s| { s.set_only_v6(false)?; s.set_reuse_address(true)?; s.bind(&addr_v6.into())?; s.listen(1024)?; Ok(s) })
    {
        Ok(socket) => {
            info!("http server listening on {} (dual-stack v4/v6)", addr_v6);
            socket.into()
        }
        Err(e) => {
            warn!("dual-stack bind failed ({e}), falling back to IPv4-only 0.0.0.0:{port_num}");
            let addr_v4 = format!("0.0.0.0:{}", port_num);
            std::net::TcpListener::bind(&addr_v4)?
        }
    };

    server.listen(listener)?.run().await?;
    Ok(())
