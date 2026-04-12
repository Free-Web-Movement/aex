use aex::http::router::{NodeType, Router as HttpRouter};
use aex::http::middlewares::websocket::WebSocket;
use aex::http::types::Executor;
use aex::http::websocket::WSFrame;
use aex::server::HTTPServer;
use aex::tcp::types::{Command, RawCodec};
use aex::{exe, get, route};
use std::net::SocketAddr;
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let addr: SocketAddr = "0.0.0.0:8080".parse()?;
    let mut router = HttpRouter::new(NodeType::Static("root".into()));

    let ws_handler = WebSocket::new().set_handler(|_ws, _ctx, frame| {
        Box::pin(async move {
            match frame {
                WSFrame::Text(text) => {
                    println!("Received text: {}", text);
                    let response = if text.contains("ping") {
                        WSFrame::Text("pong".to_string())
                    } else if text.contains("echo") {
                        WSFrame::Text(text.replace("echo", "echoed"))
                    } else {
                        WSFrame::Text(format!("Echo: {}", text))
                    };
                    println!("Sending response: {:?}", response);
                }
                WSFrame::Binary(data) => {
                    println!("Received {} bytes", data.len());
                }
                WSFrame::Ping(payload) => {
                    println!("Ping received: {:?}", payload);
                }
                _ => {}
            }
            true
        })
    });

    let ws_middleware: Arc<Executor> = Arc::from(WebSocket::to_middleware(ws_handler));

    route!(router, get!(
        "/",
        exe!(|ctx| {
            ctx.send("WebSocket server. Connect to /ws");
            true
        })
    ));

    route!(router, get!(
        "/ws",
        exe!(|_ctx| { true }),
        vec![ws_middleware]
    ));

    println!("Server running at http://{}", addr);
    println!("WebSocket endpoint: ws://{}/ws", addr);
    println!("Test with: wscat -c ws://127.0.0.1:8080/ws");

    HTTPServer::new(addr, None)
        .http(router)
        .start::<RawCodec, RawCodec>(Arc::new(|c| c.id()))
        .await?;
    Ok(())
}
