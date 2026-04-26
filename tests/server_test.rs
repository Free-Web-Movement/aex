use aex::communicators::event::Event;
use aex::connection::context::TypeMapExt;
use aex::http::router::Router as HttpRouter;
use aex::http::types::Executor;
use aex::server::Server;
use futures::FutureExt;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::{Arc, atomic::AtomicUsize, atomic::Ordering};
use tokio::time::{Duration, sleep, timeout};

#[tokio::test]
async fn test_server_http() {
    let addr: SocketAddr = "[::1]:0".parse().unwrap();
    let server = Server::new(addr, None);

    let mut http_router = HttpRouter::default();
    let handler: Arc<Executor> = Arc::new(|_ctx: &mut aex::connection::context::Context| {
        Box::pin(async move {
            println!("HTTP handler executed");
            true
        }) as Pin<Box<dyn futures::Future<Output = bool> + Send>>
    });
    http_router.get("/", handler).register();

    let temp_listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    let actual_addr = temp_listener.local_addr().unwrap();
    drop(temp_listener);

    let server = Server::new(actual_addr, None).http(http_router);

    tokio::spawn(async move {
        if let Err(e) = server.start().await {
            eprintln!("Server error: {}", e);
        }
    });

    sleep(Duration::from_millis(300)).await;

    let http_res = reqwest::get(format!("http://{}", actual_addr))
        .await
        .expect("HTTP request failed");
    assert_eq!(http_res.status(), 200);

    println!("HTTP server test passed!");
}

#[tokio::test]
async fn test_server_communication_bus() {
    let addr: SocketAddr = "[::1]:0".parse().unwrap();
    let server = Server::new(addr, None);

    let pipe_count = Arc::new(AtomicUsize::new(0));
    let spread_count = Arc::new(AtomicUsize::new(0));
    let event_count = Arc::new(AtomicUsize::new(0));

    let p_c = Arc::clone(&pipe_count);
    server
        .globals
        .pipe::<String>(
            "audit_log",
            Box::new(move |msg| {
                let c = Arc::clone(&p_c);
                async move {
                    println!("[Pipe Test] 收到日志: {}", msg);
                    c.fetch_add(1, Ordering::SeqCst);
                }
                .boxed()
            }),
        )
        .await;

    let s_c = Arc::clone(&spread_count);
    server
        .globals
        .spread::<i32>(
            "config_sync",
            Box::new(move |val| {
                let c = Arc::clone(&s_c);
                async move {
                    println!("[Spread Test] 收到配置版本: {}", val);
                    c.fetch_add(1, Ordering::SeqCst);
                }
                .boxed()
            }),
        )
        .await;

    let e_c = Arc::clone(&event_count);
    server
        .globals
        .event::<u32>(
            "user_login",
            Arc::new(move |uid| {
                let c = Arc::clone(&e_c);
                async move {
                    println!("[Event Test] 用户 {} 登录", uid);
                    c.fetch_add(1, Ordering::SeqCst);
                }
                .boxed()
            }),
        )
        .await;

    let globals = server.globals;

    globals
        .pipe
        .send("audit_log", "Server started".to_string())
        .await
        .unwrap();
    globals.spread.publish("config_sync", 101).await.unwrap();
    globals
        .event
        .notify("user_login".to_string(), 888_u32)
        .await;

    sleep(Duration::from_millis(100)).await;

    assert_eq!(
        pipe_count.load(Ordering::SeqCst),
        1,
        "Pipe callback should execute 1 time"
    );
    assert_eq!(
        spread_count.load(Ordering::SeqCst),
        1,
        "Spread callback should execute 1 time"
    );
    assert_eq!(
        event_count.load(Ordering::SeqCst),
        1,
        "Event callback should execute 1 time"
    );

    println!("Server communication bus test passed!");
}
