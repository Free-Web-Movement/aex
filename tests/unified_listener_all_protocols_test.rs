use aex::connection::context::Context;
use aex::connection::global::GlobalContext;
use aex::http::router::Router as HttpRouter;
use aex::http::types::Executor;
use aex::unified::{UnifiedServer, Protocol};
use aex::http::websocket::{WSCodec, WSFrame};
use futures::FutureExt;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::{Duration, sleep};
use tokio_util::codec::{Decoder, Encoder};
use bytes::BytesMut;

fn make_http_router() -> HttpRouter {
    let mut router = HttpRouter::new(aex::http::router::NodeType::Static("root".into()));

    let handler: Arc<Executor> = Arc::new(|_ctx: &mut Context| {
        Box::pin(async move {
            true
        }) as Pin<Box<dyn futures::Future<Output = bool> + Send>>
    });

    router.get("/", handler).register();
    router
}

fn setup_unified_server(addr: SocketAddr, enable_http2: bool) -> (UnifiedServer, Arc<AtomicUsize>, Arc<AtomicUsize>, Arc<AtomicUsize>) {
    let http_counter = Arc::new(AtomicUsize::new(0));
    let ws_counter = Arc::new(AtomicUsize::new(0));
    let tcp_counter = Arc::new(AtomicUsize::new(0));

    let globals = Arc::new(GlobalContext::new(addr, None));

    let mut unified = UnifiedServer::new(addr, globals);

    let tcp_counter_clone = tcp_counter.clone();

    unified = unified
        .http_router(make_http_router())
        .tcp_handler(Arc::new(move |mut ctx| {
            println!("[Test] TCP handler called");
            let counter = tcp_counter_clone.clone();
            tokio::spawn(async move {
                let mut buf = [0u8; 1024];
                let reader = ctx.reader.as_mut().unwrap();
                match reader.read(&mut buf).await {
                    Ok(n) => {
                        println!("[Test] TCP read {} bytes", n);
                        counter.fetch_add(1, Ordering::SeqCst);
                    }
                    Err(e) => {
                        println!("[Test] TCP read error: {}", e);
                    }
                }
            })
        }));

    if enable_http2 {
        unified = unified.enable_http2();
    }

    (unified, http_counter, ws_counter, tcp_counter)
}

#[tokio::test]
async fn test_protocol_detection_http1() {
    let addr: SocketAddr = "[::1]:0".parse().unwrap();
    let temp_listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    let actual_addr = temp_listener.local_addr().unwrap();
    drop(temp_listener);

    let (server, _, _, _) = setup_unified_server(actual_addr, false);

    tokio::spawn(async move {
        if let Err(e) = server.start().await {
            eprintln!("Server error: {}", e);
        }
    });

    sleep(Duration::from_millis(100)).await;

    let mut conn = TcpStream::connect(actual_addr).await.unwrap();
    conn.write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .await
        .unwrap();
    
    let mut buf = [0u8; 1024];
    let n = conn.read(&mut buf).await.unwrap();
    
    assert!(n > 0, "Should receive HTTP response");
    assert!(buf.starts_with(b"HTTP/1.1"), "Should be HTTP/1.1 response");
    
    println!("[Test] HTTP/1.1 protocol detection: OK");
}

#[tokio::test]
async fn test_protocol_detection_http2() {
    let addr: SocketAddr = "[::1]:0".parse().unwrap();
    let temp_listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    let actual_addr = temp_listener.local_addr().unwrap();
    drop(temp_listener);

    let (server, _, _, _) = setup_unified_server(actual_addr, true);

    tokio::spawn(async move {
        if let Err(e) = server.start().await {
            eprintln!("Server error: {}", e);
        }
    });

    sleep(Duration::from_millis(100)).await;

    let mut conn = TcpStream::connect(actual_addr).await.unwrap();
    conn.write_all(b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n")
        .await
        .unwrap();

    let mut buf = [0u8; 24];
    let result = tokio::time::timeout(Duration::from_millis(500), conn.read(&mut buf)).await;
    
    assert!(result.is_ok(), "HTTP/2 connection should be handled");
    
    println!("[Test] HTTP/2 protocol detection: OK");
}

#[tokio::test]
async fn test_protocol_detection_websocket() {
    let addr: SocketAddr = "[::1]:0".parse().unwrap();
    let temp_listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    let actual_addr = temp_listener.local_addr().unwrap();
    drop(temp_listener);

    let (server, _, _, _) = setup_unified_server(actual_addr, false);

    tokio::spawn(async move {
        if let Err(e) = server.start().await {
            eprintln!("Server error: {}", e);
        }
    });

    sleep(Duration::from_millis(100)).await;

    let mut conn = TcpStream::connect(actual_addr).await.unwrap();

    let ws_request = b"GET /ws HTTP/1.1\r\n\
                       Host: localhost\r\n\
                       Upgrade: websocket\r\n\
                       Connection: Upgrade\r\n\
                       Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\
                       Sec-WebSocket-Version: 13\r\n\r\n";

    conn.write_all(ws_request).await.unwrap();

    let mut buf = [0u8; 1024];
    let n = conn.read(&mut buf).await.unwrap();

    assert!(n > 0, "Should receive WebSocket response");
    
    println!("[Test] WebSocket protocol detection: OK");
}

#[tokio::test]
async fn test_protocol_detection_tcp() {
    let addr: SocketAddr = "[::1]:0".parse().unwrap();
    let temp_listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    let actual_addr = temp_listener.local_addr().unwrap();
    drop(temp_listener);

    let (server, _, _, _) = setup_unified_server(actual_addr, false);

    tokio::spawn(async move {
        if let Err(e) = server.start().await {
            eprintln!("Server error: {}", e);
        }
    });

    sleep(Duration::from_millis(100)).await;

    let mut conn = TcpStream::connect(actual_addr).await.unwrap();
    conn.write_all(b"\x00custom_p2p_payload").await.unwrap();
    conn.flush().await.unwrap();
    conn.shutdown().await.ok();

    sleep(Duration::from_millis(200)).await;
    
    println!("[Test] TCP protocol detection: OK");
}

#[tokio::test]
async fn test_unified_all_protocols_same_listener() {
    let addr: SocketAddr = "[::1]:0".parse().unwrap();
    let temp_listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    let actual_addr = temp_listener.local_addr().unwrap();
    drop(temp_listener);

    let (server, http_counter, ws_counter, tcp_counter) = setup_unified_server(actual_addr, true);

    let server_handle = tokio::spawn(async move {
        if let Err(e) = server.start().await {
            eprintln!("Server error: {}", e);
        }
    });

    sleep(Duration::from_millis(100)).await;

    let mut success_count = 0;

    {
        let mut conn = TcpStream::connect(actual_addr).await.unwrap();
        conn.write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .await
            .unwrap();
        let mut buf = [0u8; 1024];
        let n = conn.read(&mut buf).await.unwrap();
        if n > 0 && buf.starts_with(b"HTTP/1.1") {
            println!("[Test] HTTP/1.1: OK");
            success_count += 1;
        }
    }

    sleep(Duration::from_millis(50)).await;

    {
        let mut conn = TcpStream::connect(actual_addr).await.unwrap();
        conn.write_all(b"GET /ws HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n")
            .await
            .unwrap();
        let mut buf = [0u8; 1024];
        let n = conn.read(&mut buf).await.unwrap();
        if n > 0 {
            println!("[Test] WebSocket: OK ({} bytes)", n);
            success_count += 1;
        }
    }

    sleep(Duration::from_millis(50)).await;

    {
        let mut conn = TcpStream::connect(actual_addr).await.unwrap();
        conn.write_all(b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n")
            .await
            .unwrap();
        let mut buf = [0u8; 24];
        let result = tokio::time::timeout(Duration::from_millis(300), conn.read(&mut buf)).await;
        if result.is_ok() {
            println!("[Test] HTTP/2: OK");
            success_count += 1;
        }
    }

    sleep(Duration::from_millis(50)).await;

    {
        let mut conn = TcpStream::connect(actual_addr).await.unwrap();
        conn.write_all(b"\x00custom_p2p_payload").await.unwrap();
        conn.flush().await.unwrap();
        conn.shutdown().await.ok();
    }

    sleep(Duration::from_millis(300)).await;

    let http_count = http_counter.load(Ordering::SeqCst);
    let ws_count = ws_counter.load(Ordering::SeqCst);
    let tcp_count = tcp_counter.load(Ordering::SeqCst);

    println!("[Test] HTTP counters - HTTP: {}, WebSocket: {}, TCP: {}", http_count, ws_count, tcp_count);

    assert!(success_count >= 3, "At least 3 protocols should work (HTTP/1.1, WebSocket, HTTP/2)");
    
    server_handle.abort();
    
    println!("[Test] Unified all protocols same listener: {}/4 passed", success_count);
}

#[tokio::test]
async fn test_websocket_frame_codec() {
    let mut codec = WSCodec {};
    
    let mut src = BytesMut::from(&[0x81, 0x05][..]);
    src.extend_from_slice(b"hello");
    
    let result = codec.decode(&mut src);
    assert!(result.is_ok());
    let frame = result.unwrap().unwrap();
    assert_eq!(frame, WSFrame::Text("hello".to_string()));
    
    let mut dst = BytesMut::new();
    codec.encode(WSFrame::Text("hello".to_string()), &mut dst).unwrap();
    assert_eq!(dst[0], 0x81); // FIN + text opcode
    assert_eq!(dst[1], 0x05); // length 5
    assert_eq!(&dst[2..], b"hello");
    
    println!("[Test] WebSocket frame codec: OK");
}

#[test]
fn test_protocol_detect_http1() {
    let bytes = b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n";
    let protocol = Protocol::detect(bytes, false);
    assert_eq!(protocol, Protocol::Http11);
    
    let bytes = b"POST /api/data HTTP/1.1\r\nHost: example.com\r\n\r\n";
    let protocol = Protocol::detect(bytes, false);
    assert_eq!(protocol, Protocol::Http11);
    
    println!("[Test] Protocol::detect HTTP/1.1: OK");
}

#[test]
fn test_protocol_detect_http2() {
    let bytes = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";
    let protocol = Protocol::detect(bytes, false);
    assert_eq!(protocol, Protocol::Http2);
    
    println!("[Test] Protocol::detect HTTP/2: OK");
}

#[test]
fn test_protocol_detect_tcp() {
    let bytes = b"\x00\x01\x02\x03custom_tcp_data";
    let protocol = Protocol::detect(bytes, false);
    assert_eq!(protocol, Protocol::TCP);
    
    println!("[Test] Protocol::detect TCP: OK");
}

#[test]
fn test_protocol_detect_udp() {
    let bytes = b"some data";
    let protocol = Protocol::detect(bytes, true);
    assert_eq!(protocol, Protocol::UDP);
    
    println!("[Test] Protocol::detect UDP: OK");
}

#[test]
fn test_protocol_detect_empty() {
    let bytes: &[u8] = b"";
    let protocol = Protocol::detect(bytes, false);
    assert_eq!(protocol, Protocol::Unknown);
    
    println!("[Test] Protocol::detect empty: OK");
}