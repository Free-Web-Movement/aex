//!
//! HTTP/1.1 + HTTP/2 + WebSocket + Framed TCP + Framed UDP
//! 所有协议共用8080端口
//! 运行: cargo run --example http_ws_framed_tcp_framed_udp
//!

use std::net::SocketAddr;
use std::sync::Arc;

use aex::exe;
use aex::http::middlewares::websocket::WebSocket;
use aex::http::router::{NodeType, Router as HttpRouter};
use aex::http::types::Executor;
use aex::server::Server;
use aex::tcp::router::Router as TcpRouter;
use aex::tcp::types::{Codec, RawCodec};
use aex::udp::router::Router as UdpRouter;
use anyhow::Result;

fn setup_http() -> HttpRouter {
    let mut router = HttpRouter::new(NodeType::Static("root".into()));

    router
        .get(
            "/",
            exe!(|ctx| {
                ctx.send("{\"combo\":\"http_ws_framed_tcp_framed_udp\",\"protocols\":[\"http1\",\"http2\",\"ws\",\"framed_tcp\",\"framed_udp\"]}", None);
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

fn setup_framed_tcp() -> TcpRouter<RawCodec, RawCodec> {
    let mut router = TcpRouter::new();
    router = router.extractor(|cmd: &RawCodec| -> u32 {
        if cmd.0.len() >= 4 {
            let mut arr = [0u8; 4];
            arr.copy_from_slice(&cmd.0[0..4]);
            u32::from_le_bytes(arr)
        } else {
            0
        }
    });
    router.on(
        1,
        Box::new(|ctx, _frame: RawCodec, _cmd: RawCodec| {
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

fn setup_framed_udp() -> UdpRouter<RawCodec, RawCodec> {
    let mut router = UdpRouter::new();
    router = router.extractor(|cmd: &RawCodec| -> u32 {
        if cmd.0.len() >= 4 {
            let mut arr = [0u8; 4];
            arr.copy_from_slice(&cmd.0[0..4]);
            u32::from_le_bytes(arr)
        } else {
            0
        }
    });
    router.on(1, |_g, _f, _c, addr, sock| {
        Box::pin(async move {
            let _ = sock
                .send_to(&Codec::encode(&RawCodec(b"PONG".to_vec())), addr)
                .await;
            Ok(true)
        })
    });
    router
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("\n=== http_ws_framed_tcp_framed_udp (port 8080) ===");
    println!("HTTP/1.1+2: curl http://localhost:8080/");
    println!("WebSocket:   wscat -c ws://localhost:8080/ws");
    println!("Framed TCP: nc localhost 8080");
    println!("Framed UDP:  echo test | nc -u localhost:8080\n");

    let addr: SocketAddr = "0.0.0.0:8080".parse()?;

    let mut http = setup_http();
    http.get("/ws", exe!(|_ctx| { true }))
        .middleware(setup_ws())
        .register();

    let srv = Server::new(addr, None)
        .http(http)
        .http2()
        .tcp(setup_framed_tcp());
    tokio::spawn(async move {
        let _ = srv.start_with_protocols::<RawCodec, RawCodec>().await;
    });

    // Framed UDP
    let udp_srv = Server::new(addr, None).udp(setup_framed_udp());
    tokio::spawn(async move {
        let _ = udp_srv.start_with_protocols::<RawCodec, RawCodec>().await;
    });

    println!("Started. Ctrl+C to stop.\n");
    tokio::signal::ctrl_c().await?;
    Ok(())
}
