use aex::http::router::{NodeType, Router as HttpRouter};
use aex::http::middlewares::websocket::WebSocket;
use aex::http::types::Executor;
use aex::http::websocket::WSFrame;
use aex::server::HTTPServer;
use aex::tcp::types::{Command, RawCodec};
use aex::exe;
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
                }
                WSFrame::Binary(data) => {
                    println!("Received {} bytes", data.len());
                }
                _ => {}
            }
            true
        })
    });

    let ws_middleware: Arc<Executor> = Arc::from(WebSocket::to_middleware(ws_handler));

    router.get("/", exe!(|ctx| {
        ctx.send("WebSocket server. Connect to /ws", None);
        true
    })).register();

    router.get("/ws", exe!(|_ctx| { true }))
        .middleware(ws_middleware)
        .register();

    println!("Server running at http://{}", addr);
    println!("WebSocket endpoint: ws://{}/ws", addr);

    HTTPServer::new(addr, None)
        .http(router)
        .start::<RawCodec, RawCodec>(Arc::new(|c| c.id()))
        .await?;
    Ok(())
}
