use aex::connection::global::GlobalContext;
use aex::http::router::Router as HttpRouter;
use aex::http::middlewares::websocket::WebSocket;
use aex::http::types::Executor;
use aex::unified::UnifiedServer;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    let addr: SocketAddr = "0.0.0.0:8080".parse()?;

    let globals = Arc::new(GlobalContext::new(addr, None));
    let mut unified = UnifiedServer::new(addr, globals);

    let mut router = HttpRouter::new(aex::http::router::NodeType::Static("root".into()));

    router.get("/", aex::exe!(|ctx| {
        ctx.send("Hello from HTTP/1.1!", None);
        true
    })).register();

    router.get("/info", aex::exe!(|ctx| {
        ctx.send(r#"{"protocol":"HTTP/1.1","message":"Welcome to AEX Unified Server"}"#, None);
        ctx.res().set_header("Content-Type", "application/json");
        true
    })).register();

    let ws_handler = WebSocket::new()
        .on_text(|_ws, _ctx, text| {
            Box::pin(async move {
                println!("[WebSocket] Received text: {}", text);
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

    router.get("/ws", aex::exe!(|_ctx| { true }))
        .middleware(ws_middleware)
        .register();

    router.get("/test", aex::exe!(|ctx| {
        let html = r#"<!DOCTYPE html>
<html>
<head>
    <title>AEX Unified Server Test</title>
    <style>
        body { font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; max-width: 800px; margin: 50px auto; padding: 20px; }
        .card { border: 1px solid #ddd; border-radius: 8px; padding: 20px; margin: 10px 0; }
        .card h3 { margin-top: 0; color: #333; }
        .btn { background: #007bff; color: white; border: none; padding: 10px 20px; border-radius: 4px; cursor: pointer; margin: 5px; }
        input { width: 100%; padding: 8px; margin: 5px 0; }
        #messages { height: 200px; overflow-y: auto; border: 1px solid #ddd; padding: 10px; background: #f9f9f9; }
    </style>
</head>
<body>
    <h1>AEX Unified Server Test</h1>
    <div class="card">
        <h3>HTTP/1.1</h3>
        <button class="btn" onclick="testHttp()">Test GET /</button>
        <pre id="http-result"></pre>
    </div>
    <div class="card">
        <h3>WebSocket</h3>
        <input id="ws-msg" placeholder="Message...">
        <button class="btn" onclick="connectWs()">Connect</button>
        <button class="btn" onclick="sendWs()">Send</button>
        <button class="btn" onclick="closeWs()">Disconnect</button>
        <div id="ws-status">Disconnected</div>
        <div id="messages"></div>
    </div>
    <script>
        let ws = null;
        async function testHttp() {
            const res = await fetch('/');
            document.getElementById('http-result').textContent = await res.text();
        }
        function connectWs() {
            ws = new WebSocket('ws://' + location.host + '/ws');
            ws.onopen = () => document.getElementById('ws-status').textContent = 'Connected';
            ws.onmessage = e => { const d=document.createElement('div'); d.textContent='Recv: '+e.data; document.getElementById('messages').appendChild(d); };
            ws.onclose = () => document.getElementById('ws-status').textContent = 'Disconnected';
        }
        function sendWs() { if(ws) ws.send(document.getElementById('ws-msg').value); }
        function closeWs() { if(ws) ws.close(); }
    </script>
</body>
</html>"#;
        ctx.send(html, None);
        ctx.res().set_header("Content-Type", "text/html");
        true
    })).register();

    unified = unified
        .http_router(router)
        .enable_http2()
        .tcp_handler(Arc::new(|mut ctx| {
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                let reader = ctx.reader.as_mut().unwrap();
                match reader.read(&mut buf).await {
                    Ok(n) => {
                        let data = &buf[..n];
                        println!("[TCP] Received {} bytes: {:?}", n, String::from_utf8_lossy(data));
                        let writer = ctx.writer.as_mut().unwrap();
                        let _ = writer.write_all(b"[TCP] ACK\n").await;
                        let _ = writer.flush().await;
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
    println!("  AEX Unified Server");
    println!("  Listening on http://{}", addr);
    println!("===========================================");
    println!();
    println!("  HTTP/1.1: curl http://{}/", addr);
    println!("  HTTP/2:    curl --http2 http://{}/h2", addr);
    println!("  WebSocket:  wscat ws://{}/ws", addr);
    println!("  TCP:      echo 'hello' | nc {} 8080", addr.ip());
    println!("  UDP:      echo 'hello' | nc -u {} 8080", addr.ip());
    println!();
    println!("  API Info:  curl http://{}/info", addr);
    println!();

    unified.start().await?;

    Ok(())
}