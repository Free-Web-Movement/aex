//!
//! HTTP/1.1 + HTTP/2 + WebSocket + Raw TCP + Raw UDP
//! 所有协议共用8080端口
//! 运行: cargo run --example http_ws_raw_tcp_raw_udp
//!

use std::net::SocketAddr;
use std::sync::Arc;

use aex::exe;
use aex::http::middlewares::websocket::WebSocket;
use aex::http::router::{NodeType, Router as HttpRouter};
use aex::http::types::Executor;
use aex::server::Server;
use anyhow::Result;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn setup_http() -> HttpRouter {
    let mut router = HttpRouter::new(NodeType::Static("root".into()));
    
    router
        .get(
            "/",
            exe!(|ctx| {
                ctx.send("{\"combo\":\"http_ws_raw_tcp_raw_udp\",\"protocols\":[\"http1\",\"http2\",\"ws\",\"raw_tcp\",\"raw_udp\"]}", None);
                true
            }),
        )
        .register();
    
    router
        .get(
            "/health",
            exe!(|ctx| {
                ctx.send("{\"status\":\"ok\"}", None);
                true
            }),
        )
        .register();
    
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

#[tokio::main]
async fn main() -> Result<()> {
    println!("\n=== http_ws_raw_tcp_raw_udp (port 8080) ===");
    println!("HTTP/1.1+2: curl http://localhost:8080/");
    println!("WebSocket:   wscat -c ws://localhost:8080/ws");
    println!("Raw TCP:     nc localhost 8080");
    println!("Raw UDP:     echo test | nc -u localhost:8080\n");

    let addr: SocketAddr = "0.0.0.0:8080".parse()?;

    let mut http = setup_http();
    http.get("/ws", exe!(|_ctx| { true }))
        .middleware(setup_ws())
        .register();

    let srv = Server::new(addr, None).http(http).http2();
    tokio::spawn(async move { let _ = srv.start().await; });

    // Raw TCP
    tokio::spawn(async move {
        let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
        println!("[Raw TCP] Listening on 8080");
        loop {
            if let Ok((mut sock, peer)) = listener.accept().await {
                tokio::spawn(async move {
                    println!("[Raw TCP] {}", peer);
                    let mut buf = [0u8; 4096];
                    loop {
                        match sock.read(&mut buf).await {
                            Ok(0) => break,
                            Ok(n) => { let _ = sock.write_all(&buf[..n]).await; }
                            Err(_) => break,
                        }
                    }
                });
            }
        }
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
