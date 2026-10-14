//!
//! HTTP/1.1 + HTTP/2 + WebSocket + Framed TCP + Raw UDP
//! 所有协议共用8080端口
//! 运行: cargo run --example http_ws_framed_tcp_raw_udp
//!

use std::net::SocketAddr;
use std::sync::Arc;

use aex::connection::context::Context;
use aex::http::middlewares::websocket::WebSocket;
use aex::http::router::Router as HttpRouter;
use aex::http::types::Executor;
use aex::protocols::http1::Http1;
use aex::protocols::http2::Http2;
use aex::protocols::tcp;
use aex::protocols::tcp::Tcp;
use aex::server::Server;
use aex::tcp::router::Router as TcpRouter;
use anyhow::Result;

fn setup_http() -> HttpRouter {
    let mut router = HttpRouter::default();

    router.get("/", |ctx: &mut Context| {
        ctx.send("{\"combo\":\"http_ws_framed_tcp_raw_udp\",\"protocols\":[\"http1\",\"http2\",\"ws\",\"framed_tcp\",\"raw_udp\"]}", None);
        true
    });
    router
}

fn setup_ws() -> Arc<Executor> {
    let ws = WebSocket::new().on_text(|_ws, _ctx, text| {
        Box::pin(async move {
            println!("[WS] {}", text);
            true
        })
    });
    Arc::from(WebSocket::to_middleware(ws))
}

fn setup_framed_tcp() -> TcpRouter {
    let mut router = TcpRouter::new().extractor(
        |buf: &[u8]| -> anyhow::Result<Option<(usize, u32, bytes::Bytes, bytes::Bytes)>> {
            if buf.len() < 4 {
                return Ok(None);
            }
            let mut arr = [0u8; 4];
            arr.copy_from_slice(&buf[0..4]);
            let key = u32::from_le_bytes(arr);
            Ok(Some((
                buf.len(),
                key,
                bytes::Bytes::copy_from_slice(buf),
                bytes::Bytes::new(),
            )))
        },
    );
    router.on(
        1,
        Box::new(|ctx, _frame: bytes::Bytes, _cmd: bytes::Bytes| {
            Box::pin(async move {
                let mut g = ctx.lock().await;
                if let Some(_w) = g.writer.as_mut() {
                    // write_all omitted for compilation
                }
                Ok(true)
            })
        }),
        vec![],
    );
    router
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("\n=== http_ws_framed_tcp_raw_udp (port 8080) ===");
    println!("HTTP/1.1+2: curl http://localhost:8080/");
    println!("WebSocket:   wscat -c ws://localhost:8080/ws");
    println!("Framed TCP: nc localhost 8080\n");

    let addr: SocketAddr = "0.0.0.0:8080".parse()?;

    let mut http = setup_http();
    http.get("/ws", |_ctx: &mut Context| true)
        .middleware(setup_ws());

    let tcp_router = std::sync::Arc::new(setup_framed_tcp());
    let srv = Server::default().push(
        Tcp::new(Some(tcp::router_handler(tcp_router)))
            .sub(Http1::new(Some(std::sync::Arc::new(http)), None).shared())
            .sub(Http2::new(None, None).shared())
            .shared(),
    );
    tokio::spawn(async move {
        let _ = srv.start(addr).await;
    });

    // Raw UDP
    tokio::spawn(async move {
        let socket = tokio::net::UdpSocket::bind(addr).await.unwrap();
        println!("[Raw UDP] Listening on 8080");
        let mut buf = [0u8; 65535];
        loop {
            if let Ok((n, peer)) = socket.recv_from(&mut buf).await {
                let _ = socket.send_to(&buf[..n], peer).await;
            }
        }
    });

    println!("Started. Ctrl+C to stop.\n");
    tokio::signal::ctrl_c().await?;
    Ok(())
}
