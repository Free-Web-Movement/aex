use aex::connection::global::GlobalContext;
use aex::http::middlewares::websocket::WebSocket;
use aex::http::router::Router as HttpRouter;
use aex::http::types::Executor;
use aex::tcp::types::{Codec, Command, Frame};
use aex::unified::UnifiedServer;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Debug, Clone, bincode::Encode, bincode::Decode, serde::Serialize, serde::Deserialize)]
pub struct MyFrame {
    pub id: u32,
    pub data: Vec<u8>,
}

impl Codec for MyFrame {}
impl Frame for MyFrame {
    fn payload(&self) -> Option<Vec<u8>> {
        Some(self.data.clone())
    }
    fn validate(&self) -> bool {
        self.id != 0
    }
    fn command(&self) -> Option<&Vec<u8>> {
        Some(&self.data)
    }
}

#[derive(Debug, Clone, bincode::Encode, bincode::Decode, serde::Serialize, serde::Deserialize)]
pub struct MyCommand {
    pub id: u32,
    pub data: Vec<u8>,
}

impl Codec for MyCommand {}
impl Command for MyCommand {
    fn id(&self) -> u32 {
        self.id
    }
    fn data(&self) -> &Vec<u8> {
        &self.data
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    let addr: SocketAddr = "0.0.0.0:8080".parse()?;

    let globals = Arc::new(GlobalContext::new(addr, None));
    let mut unified = UnifiedServer::new(addr, globals);

    let mut router = HttpRouter::new(aex::http::router::NodeType::Static("root".into()));

    router
        .get(
            "/",
            aex::exe!(|ctx| {
                ctx.send("AEX Unified Server + TCP Frame!", None);
                true
            }),
        )
        .register();

    router
        .get(
            "/info",
            aex::exe!(|ctx| {
                let info = serde_json::json!({
                    "protocols": ["http1", "http2", "ws", "tcp_frame", "udp"],
                    "frame_format": "MyFrame (id:u32, data:Vec<u8>)"
                });
                ctx.send(info.to_string(), None);
                ctx.res().set_header("Content-Type", "application/json");
                true
            }),
        )
        .register();

    router
        .get(
            "/test",
            aex::exe!(|ctx| {
                ctx.send(
                    r#"<!DOCTYPE html>
<html><body>
<h1>TCP Frame Test</h1>
<p>Send binary frame with Python:</p>
<pre>python3 -c "
import struct, socket
s = socket.socket()
s.connect(('localhost', 8080))
s.send(struct.pack('<I', 1) + b'hello')
print(s.recv(1024))
s.close()
"</pre>
</body></html>"#,
                    None,
                );
                ctx.res().set_header("Content-Type", "text/html");
                true
            }),
        )
        .register();

    let ws_handler = WebSocket::new()
        .on_text(|_ws, _ctx, text| {
            Box::pin(async move {
                println!("[WebSocket] Received: {}", text);
                true
            })
        })
        .on_binary(|_ws, _ctx, data| {
            Box::pin(async move {
                println!("[WebSocket] Received binary: {} bytes", data.len());
                true
            })
        });

    let ws_middleware: Arc<Executor> = Arc::from(WebSocket::to_middleware(ws_handler));

    router
        .get("/ws", aex::exe!(|_ctx| { true }))
        .middleware(ws_middleware)
        .register();

    unified = unified
        .http_router(router)
        .enable_http2()
        .tcp_handler(Arc::new(move |mut ctx| {
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                let mut reader = match ctx.reader.take() {
                    Some(r) => r,
                    None => return,
                };
                match reader.read(&mut buf).await {
                    Ok(n) => {
                        if n > 0 {
                            let data = &buf[..n];
                            match MyFrame::decode(data) {
                                Ok(frame) => {
                                    println!(
                                        "[TCP Frame] id={}, data={:?}",
                                        frame.id,
                                        String::from_utf8_lossy(&frame.data)
                                    );

                                    let response = MyCommand {
                                        id: frame.id,
                                        data: b"ACK".to_vec(),
                                    };
                                    let encoded = Codec::encode(&response);

                                    if let Some(mut w) = ctx.writer.take() {
                                        let _ = w.write_all(&encoded).await;
                                        let _ = w.flush().await;
                                    }
                                }
                                Err(e) => {
                                    println!("[TCP Frame] Decode error: {}", e);
                                    if let Some(mut w) = ctx.writer.take() {
                                        let _ = w.write_all(b"[TCP] Invalid frame\n").await;
                                        let _ = w.flush().await;
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        println!("[TCP] Read error: {}", e);
                    }
                }
            })
        }))
        .udp_handler(Arc::new(|ctx| {
            tokio::spawn(async move {
                if let Some(data) = ctx.local.get_value::<Vec<u8>>() {
                    println!("[UDP] Received {} bytes", data.len());
                }
            })
        }));

    println!("===========================================");
    println!("  AEX Unified Server + TCP Frame");
    println!("  Listening on http://{}", addr);
    println!("===========================================");
    println!();
    println!("  HTTP/1.1:  curl http://{}/", addr);
    println!("  HTTP/2:     curl --http2 http://{}/", addr);
    println!("  WebSocket:   wscat ws://{}/ws", addr);
    println!();
    println!("  TCP Frame:  MyFrame with bincode codec");
    println!(
        "  Test:     python3 -c \"import struct,socket; s=socket.socket(); s.connect(('localhost',8080)); s.send(struct.pack('<I',1)+b'hello'); print(s.recv(1024))\""
    );
    println!("  UDP:      echo 'data' | nc -u {} 8080", addr.ip());
    println!();
    println!("  API:      curl http://{}/info", addr);
    println!();

    unified.start().await?;

    Ok(())
}
