use aex::connection::global::GlobalContext;
use aex::http::meta::HttpMetadata;
use aex::http::middlewares::websocket::WebSocket;
use aex::http::router::{NodeType, Router as HttpRouter};
use aex::http::types::Executor;
use aex::http::websocket::WSFrame;
use aex::server::Server;
use aex::tcp::router::Router as TcpRouter;
use aex::tcp::types::{Command, RawCodec};
use aex::udp::router::Router as UdpRouter;
use aex::exe;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use tokio::net::UdpSocket;

static VISITOR_COUNT: once_cell::sync::Lazy<Mutex<HashMap<String, u64>>> =
    once_cell::sync::Lazy::new(|| Mutex::new(HashMap::new()));

fn increment_visitor(ip: &str) -> u64 {
    let mut visitors = VISITOR_COUNT.lock().unwrap();
    let count = visitors.entry(ip.to_string()).or_insert(0);
    *count += 1;
    *count
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let addr: SocketAddr = "0.0.0.0:9999".parse()?;

    println!();
    println!("╔══════════════════════════════════════════════════════════════════════╗");
    println!("║              AEX Multi-Protocol Demo Server (port 9999)           ║");
    println!("╠══════════════════════════════════════════════════════════════════════╣");
    println!("║  HTTP:  curl http://localhost:9999/whoami                         ║");
    println!("║  WS:    wscat -c ws://localhost:9999/ws  (then type: ping/time) ║");
    println!("║  TCP:   nc localhost 9999 (type: PING, INFO, COUNT)              ║");
    println!("║  UDP:   echo -n 'HELLO' | nc -u localhost 9999                   ║");
    println!("╚══════════════════════════════════════════════════════════════════════╝");
    println!();

    // ═══════════════════════════════════════════════════════════════════════
    // HTTP Router
    // ═══════════════════════════════════════════════════════════════════════
    let mut http_router = HttpRouter::new(NodeType::Static("root".into()));

    let root: Arc<Executor> = exe!(|ctx| {
        let count = increment_visitor(ctx.addr.ip().to_string().as_str());
        ctx.send(
            format!(
                r#"{{"msg":"Welcome to AEX","visits":{},"ip":"{}","try":"/whoami","ws":"/ws"}}"#,
                count, ctx.addr.ip()
            ),
            None,
        );
        true
    });

    let whoami: Arc<Executor> = exe!(|ctx| {
        let count = increment_visitor(ctx.addr.ip().to_string().as_str());
        ctx.send(
            format!(
                r#"{{"ip":"{}","visits":{},"protocol":"http"}}"#,
                ctx.addr.ip(),
                count
            ),
            None,
        );
        true
    });

    let health: Arc<Executor> = exe!(|ctx| {
        let visitors = VISITOR_COUNT.lock().unwrap();
        let total: u64 = visitors.values().sum();
        let unique = visitors.len();
        ctx.send(
            format!(r#"{{"status":"ok","total_visits":{},"unique_ips":{}}}"#, total, unique),
            None,
        );
        true
    });

    let ws_handler = WebSocket::new().set_handler(|_ws, _ctx, frame| {
        Box::pin(async move {
            match frame {
                WSFrame::Text(text) => {
                    println!("[WS] Text: {}", text);
                }
                WSFrame::Binary(data) => {
                    println!("[WS] Binary: {} bytes", data.len());
                }
                WSFrame::Close(code, reason) => {
                    println!("[WS] Closed: {} - {}", code, reason.unwrap_or_default());
                }
                _ => {}
            }
            true
        })
    });
    let ws_middleware: Arc<Executor> = Arc::from(WebSocket::to_middleware(ws_handler));

    http_router.get("/", root).register();
    http_router.get("/whoami", whoami).register();
    http_router.get("/health", health).register();
    http_router.get("/ws", exe!(|ctx| {
        let _meta = ctx.local.get_value::<HttpMetadata>().unwrap();
        true
    }))
    .middleware(ws_middleware)
    .register();

    // ═══════════════════════════════════════════════════════════════════════
    // TCP Router
    // ═══════════════════════════════════════════════════════════════════════
    let mut tcp_router = TcpRouter::new();

    tcp_router.on::<RawCodec, RawCodec>(
        0x01,
        Box::new(|_ctx, _frame: RawCodec, cmd: RawCodec| {
            Box::pin(async move {
                let msg = String::from_utf8_lossy(&cmd.data());
                println!("[TCP] PING: {}", msg);
                Ok(true)
            })
        }),
        vec![],
    );

    tcp_router.on::<RawCodec, RawCodec>(
        0xFF,
        Box::new(|_ctx, _frame: RawCodec, cmd: RawCodec| {
            Box::pin(async move {
                let msg = String::from_utf8_lossy(&cmd.data()).trim().to_uppercase();
                println!("[TCP] Command: {}", msg);
                Ok(true)
            })
        }),
        vec![],
    );

    // ═══════════════════════════════════════════════════════════════════════
    // UDP Router
    // ═══════════════════════════════════════════════════════════════════════
    let mut udp_router = UdpRouter::new();

    udp_router.on::<RawCodec, RawCodec, _, _>(
        0x01,
        move |_global: Arc<GlobalContext>,
               _frame: RawCodec,
               payload: RawCodec,
               peer: SocketAddr,
               socket: Arc<UdpSocket>| async move {
            let msg = String::from_utf8_lossy(&payload.data());
            let msg = msg.trim();
            println!("[UDP] {} -> {}", peer, msg);

            let response = format!("ACK: {}", msg);
            let _ = socket.send_to(response.as_bytes(), peer).await;
            Ok(true)
        },
    );

    udp_router.on::<RawCodec, RawCodec, _, _>(
        0x02,
        move |_global: Arc<GlobalContext>,
               _frame: RawCodec,
               _payload: RawCodec,
               peer: SocketAddr,
               socket: Arc<UdpSocket>| async move {
            let (total, unique) = {
                let visitors = VISITOR_COUNT.lock().unwrap();
                let total: u64 = visitors.values().sum();
                let unique = visitors.len();
                (total, unique)
            };
            let response = format!("VISITORS: total={}, unique={}", total, unique);
            let _ = socket.send_to(response.as_bytes(), peer).await;
            Ok(true)
        },
    );

    // ═══════════════════════════════════════════════════════════════════════
    // Start Server
    // ═══════════════════════════════════════════════════════════════════════
    Server::new(addr, None)
        .http(http_router)
        .tcp::<RawCodec>(tcp_router, Arc::new(|c: &RawCodec| c.id()))
        .udp::<RawCodec>(udp_router, Arc::new(|c: &RawCodec| c.id()))
        .start()
        .await?;

    Ok(())
}
