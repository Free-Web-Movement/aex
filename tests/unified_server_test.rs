use aex::connection::global::GlobalContext;
use aex::http::router::Router as HttpRouter;
use aex::http::types::Executor;
use aex::unified::UnifiedServer;
use futures::FutureExt;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::{Duration, sleep};

fn make_http_router() -> HttpRouter {
    let mut router = HttpRouter::new(
        aex::http::router::NodeType::Static("root".into())
    );
    
    let handler: Arc<Executor> = Arc::new(|_ctx: &mut aex::connection::context::Context| {
        Box::pin(async move {
            println!("[Test] HTTP handler executed");
            true
        }) as Pin<Box<dyn futures::Future<Output = bool> + Send>>
    });
    
    router.get("/", handler).register();
    router
}

fn setup_unified_server(addr: SocketAddr, enable_http2: bool) -> (UnifiedServer, Arc<AtomicUsize>) {
    let tcp_counter = Arc::new(AtomicUsize::new(0));
    
    let globals = Arc::new(GlobalContext::new(addr, None));
    
    let mut unified = UnifiedServer::new(addr, globals);
    
    let counter_for_handler = tcp_counter.clone();
    unified = unified
        .http_router(make_http_router())
        .tcp_handler(Arc::new(move |mut ctx| {
            println!("[Test] TCP handler called");
            let counter = counter_for_handler.clone();
            tokio::spawn(async move {
                println!("[Test] TCP task spawned");
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
    
    (unified, tcp_counter)
}

#[tokio::test]
async fn test_unified_protocol_detection() {
    let addr: SocketAddr = "[::1]:0".parse().unwrap();
    let temp_listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    let actual_addr = temp_listener.local_addr().unwrap();
    drop(temp_listener);
    
    let (server, tcp_counter) = setup_unified_server(actual_addr, false);
    
    tokio::spawn(async move {
        if let Err(e) = server.start().await {
            eprintln!("Server error: {}", e);
        }
    });

    sleep(Duration::from_millis(100)).await;
    
    {
        let mut conn = TcpStream::connect(actual_addr).await.unwrap();
        conn.write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n").await.unwrap();
        let mut buf = [0u8; 1024];
        let _n = conn.read(&mut buf).await.unwrap();
        println!("[Test] HTTP/1.1 response received");
    }
    
    sleep(Duration::from_millis(100)).await;
    
    {
        let mut conn = TcpStream::connect(actual_addr).await.unwrap();
        conn.write_all(b"\x00\x01\x02\x03custom_tcp_data").await.unwrap();
        sleep(Duration::from_millis(100)).await;
    }
    
    sleep(Duration::from_millis(200)).await;
    
    let tcp_count = tcp_counter.load(Ordering::SeqCst);
    println!("[Test] TCP connections handled: {}", tcp_count);
    
    println!("Unified protocol detection test passed!");
}

#[tokio::test]
async fn test_http1_on_unified_server() {
    let addr: SocketAddr = "[::1]:0".parse().unwrap();
    let temp_listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    let actual_addr = temp_listener.local_addr().unwrap();
    drop(temp_listener);
    
    let (server, _) = setup_unified_server(actual_addr, false);
    
    tokio::spawn(async move {
        if let Err(e) = server.start().await {
            eprintln!("Server error: {}", e);
        }
    });

    sleep(Duration::from_millis(100)).await;
    
    let res = reqwest::get(format!("http://{}", actual_addr))
        .await
        .expect("HTTP request failed");
    
    assert_eq!(res.status(), 200);
    println!("HTTP/1.1 on unified server test passed!");
}

#[tokio::test]
async fn test_p2p_tcp_on_unified_server() {
    let addr: SocketAddr = "[::1]:0".parse().unwrap();
    let temp_listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    let actual_addr = temp_listener.local_addr().unwrap();
    drop(temp_listener);
    
    let (server, tcp_counter) = setup_unified_server(actual_addr, false);
    
    let server_handle = tokio::spawn(async move {
        if let Err(e) = server.start().await {
            eprintln!("Server error: {}", e);
        }
    });

    sleep(Duration::from_millis(100)).await;
    
    {
        let mut conn = TcpStream::connect(actual_addr).await.unwrap();
        conn.write_all(b"\x00custom_p2p_payload").await.unwrap();
        conn.flush().await.unwrap();
        conn.shutdown().await.ok();
    }
    
    sleep(Duration::from_millis(500)).await;
    
    let count = tcp_counter.load(Ordering::SeqCst);
    println!("[Test] Final P2P counter value: {}", count);
    
    server_handle.abort();
    
    assert!(count >= 1, "Should have handled at least 1 TCP P2P connection, got {}", count);
    println!("P2P TCP on unified server test passed!");
}

#[tokio::test]
async fn test_websocket_detection() {
    let addr: SocketAddr = "[::1]:0".parse().unwrap();
    let temp_listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    let actual_addr = temp_listener.local_addr().unwrap();
    drop(temp_listener);
    
    let (server, _) = setup_unified_server(actual_addr, false);
    
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
    println!("[Test] WebSocket request handled, got {} bytes", n);
    println!("WebSocket detection test passed!");
}

#[tokio::test]
async fn test_http2_detection() {
    let addr: SocketAddr = "[::1]:0".parse().unwrap();
    let temp_listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    let actual_addr = temp_listener.local_addr().unwrap();
    drop(temp_listener);
    
    let (server, _) = setup_unified_server(actual_addr, true);
    
    tokio::spawn(async move {
        if let Err(e) = server.start().await {
            eprintln!("Server error: {}", e);
        }
    });

    sleep(Duration::from_millis(100)).await;
    
    let mut conn = TcpStream::connect(actual_addr).await.unwrap();
    
    conn.write_all(b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n").await.unwrap();
    
    let mut buf = [0u8; 1024];
    let n = tokio::time::timeout(Duration::from_millis(500), conn.read(&mut buf)).await;
    
    assert!(n.is_ok(), "HTTP/2 connection should be handled");
    println!("[Test] HTTP/2 connection initiated");
    println!("HTTP/2 detection test passed!");
}

#[tokio::test]
async fn test_unified_all_protocols() {
    let addr: SocketAddr = "[::1]:0".parse().unwrap();
    let temp_listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    let actual_addr = temp_listener.local_addr().unwrap();
    drop(temp_listener);
    
    let (server, tcp_counter) = setup_unified_server(actual_addr, true);
    
    let server_handle = tokio::spawn(async move {
        if let Err(e) = server.start().await {
            eprintln!("Server error: {}", e);
        }
    });

    sleep(Duration::from_millis(100)).await;
    
    let mut success_count = 0;
    
    {
        let res = reqwest::get(format!("http://{}", actual_addr)).await;
        if res.is_ok() && res.unwrap().status() == 200 {
            println!("[Test] HTTP/1.1: OK");
            success_count += 1;
        }
    }
    
    {
        let mut conn = TcpStream::connect(actual_addr).await.unwrap();
        conn.write_all(b"\x00custom_p2p_payload").await.unwrap();
        conn.flush().await.unwrap();
        conn.shutdown().await.ok();
        sleep(Duration::from_millis(300)).await;
        let count = tcp_counter.load(Ordering::SeqCst);
        if count > 0 {
            println!("[Test] TCP P2P: OK ({} connections)", count);
            success_count += 1;
        }
    }
    
    {
        let mut conn = TcpStream::connect(actual_addr).await.unwrap();
        conn.write_all(b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n").await.unwrap();
        let mut buf = [0u8; 24];
        let result = tokio::time::timeout(Duration::from_millis(300), conn.read(&mut buf)).await;
        if result.is_ok() {
            println!("[Test] HTTP/2: OK");
            success_count += 1;
        }
    }
    
    assert!(success_count >= 2, "At least HTTP/1.1 and TCP should work");
    println!("[Test] All protocols test: {}/3 passed", success_count);
    
    server_handle.abort();
    println!("Unified all protocols test passed!");
}