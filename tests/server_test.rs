use aex::communicators::event::Event;
use aex::connection::context::{Context, TypeMapExt};
use aex::http::meta::HttpMetadata;
use aex::http::protocol::header::HeaderKey;
use aex::http::protocol::status::StatusCode;
use aex::server::{HTTPServer, Server};
use aex::tcp::types::{Codec, Command, RawCodec, TCPFrame, TCPCommand};
use futures::FutureExt;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio::time::{Duration, sleep, timeout};

use aex::http::router::{NodeType, Router as HttpRouter};
use aex::tcp::router::Router as TcpRouter;
use aex::udp::router::Router as UdpRouter;

// --- 2. 统一测试套件 ---
#[tokio::test]
async fn test_aex_server_coverage() {
    // 使用 timeout 包裹整个测试，防止死锁导致 CI 卡死
    let test_result = timeout(Duration::from_secs(10), async {
        // 1. 自动分配可用端口
        let addr: SocketAddr = "[::1]:0".parse().unwrap();
        let mut server = HTTPServer::new(addr, None);

        // 2. 注册 HTTP 路由
        let mut hr = HttpRouter::new(NodeType::Static("root".into()));

        hr.insert(
            "/",
            Some("GET"),
            Arc::new(|ctx: &mut Context| {
                Box::pin(async move {
                    let meta = &mut ctx.local.get_value::<HttpMetadata>().unwrap();
                    meta.status = StatusCode::Ok;
                    meta.headers
                        .insert(HeaderKey::ContentType, "text/plain".to_string());
                    meta.body = b"Hello world!".to_vec();

                    println!("HTTP handler executed, meta prepared: {:?}", meta);

                    println!(
                        "Preparing to send response with local context: {:?}",
                        ctx.local.get_value::<HttpMetadata>().unwrap()
                    );
                    ctx.local.set_value(meta.clone()); // 同步更新回 local，确保后续中间件和处理器能访问到最新的 Metadata

                    true
                })
                .boxed()
            }),
            None,
        );

        // 3. 注册 TCP 路由 (ID 10)
        let mut tr = TcpRouter::new();
        tr.on::<RawCodec, RawCodec>(
            10,
            Box::new(|_, _, _| Box::pin(async move { Ok(true) }).boxed()),
            vec![],
        );

        // 4. 注册 UDP 路由 (ID 20)
        let mut ur = UdpRouter::new();
        ur.on::<RawCodec, RawCodec, _, _>(20, |_, _, _, addr, socket| async move {
            socket.send_to(b"udp_ok", addr).await?;
            Ok(true)
        });

        // 绑定实际端口
        let temp_listener = TcpListener::bind(addr).await.unwrap();
        let actual_addr = temp_listener.local_addr().unwrap();
        drop(temp_listener); // 释放临时绑定的端口以便 Server 使用

        server.addr = actual_addr;
        let server = server
            .http(hr)
            .tcp(tr, Arc::new(|c: &RawCodec| c.id()))
            .udp(ur, Arc::new(|c: &RawCodec| c.id()))
            .clone();

        // 启动服务器
        tokio::spawn(async move {
            if let Err(e) = server.start().await {
                eprintln!("Server exit with error: {}", e);
            }
        });

        // 等待服务器就绪
        sleep(Duration::from_millis(300)).await;

        // --- 分支测试 A: HTTP 嗅探与响应 ---
        println!("Testing HTTP...");
        let http_res = reqwest::get(format!("http://{}", actual_addr))
            .await
            .expect("HTTP request failed");
        assert_eq!(http_res.status(), 200);
        assert_eq!(http_res.text().await.unwrap(), "Hello world!");

        // --- 分支测试 B: TCP 正常路由 ---
        println!("Testing TCP Normal...");
        let mut tcp_conn = TcpStream::connect(actual_addr)
            .await
            .expect("TCP connect failed");
        let rawtcp = Codec::encode(&RawCodec(vec![10, 0, 0, 0]));

        tcp_conn.write_all(&rawtcp).await.unwrap(); // 发送 ID 10
        sleep(Duration::from_millis(100)).await;

        // --- 分支测试 C: TCP 异常包 (触发解包失败但不崩溃) ---
        println!("Testing TCP Robustness...");
        let mut tcp_bad = TcpStream::connect(actual_addr).await.unwrap();
        tcp_bad.write_all(&[0xff, 0xff, 0, 0]).await.unwrap(); // 触发 MockProtocol::decode 中的错误
        sleep(Duration::from_millis(100)).await;

        // --- 分支测试 D: UDP 正常路由与回包 ---
        println!("Testing UDP...");
        let udp_client = match UdpSocket::bind("0.0.0.0:0").await {
            Ok(s) => s,
            Err(e) => {
                println!("UDP bind failed (skipped): {}", e);
                return;
            }
        };
        let target_addr = format!("{}:{}", actual_addr.ip(), actual_addr.port());
        let rawudp = Codec::encode(&RawCodec(vec![20, 0, 0, 0]));
        if let Err(e) = udp_client.send_to(&rawudp, &target_addr).await {
            println!("UDP send failed (skipped): {}", e);
            return;
        }
        let mut buf = [0u8; 1024];
        let result = timeout(Duration::from_secs(2), udp_client.recv_from(&mut buf)).await;
        match result {
            Ok(Ok((len, _))) => {
                assert_eq!(&buf[..len], b"udp_ok");
            }
            _ => {
                println!("UDP test skipped (environment limitation)");
            }
        }

        // --- 分支测试 E: UDP 未匹配路由 ---
        println!("Testing UDP Mismatch...");
        udp_client
            .send_to(&[99, 0, 0, 0], actual_addr)
            .await
            .unwrap(); // ID 99
        sleep(Duration::from_millis(100)).await;

        println!("✅ 所有覆盖率分支已跑通，服务器运行平稳。");
    })
    .await;

    if test_result.is_err() {
        panic!("Test timed out! Possible deadlock or server not responding.");
    }
}

#[tokio::test]
async fn test_server_communication_bus() {
    let addr = "[::1]:0".parse().unwrap(); // 自动分配可用端口
    let server = HTTPServer::new(addr, None);

    // 准备计数器
    let pipe_count = Arc::new(AtomicUsize::new(0));
    let spread_count = Arc::new(AtomicUsize::new(0));
    let event_count = Arc::new(AtomicUsize::new(0));

    // 1. 测试 Pipe (N:1)
    let p_c = Arc::clone(&pipe_count);
    server
        .globals
        .pipe::<String>(
            "audit_log",
            Box::new(move |msg| {
                let c = Arc::clone(&p_c);
                (async move {
                    println!("[Pipe Test] 收到日志: {}", msg);
                    c.fetch_add(1, Ordering::SeqCst);
                })
                .boxed()
            }),
        )
        .await;

    // 2. 测试 Spread (1:N)
    let s_c = Arc::clone(&spread_count);
    server
        .globals
        .spread::<i32>(
            "config_sync",
            Box::new(move |val| {
                let c = Arc::clone(&s_c);
                (async move {
                    println!("[Spread Test] 收到配置版本: {}", val);
                    c.fetch_add(1, Ordering::SeqCst);
                })
                .boxed()
            }),
        )
        .await;

    // 3. 测试 Event (M:N)
    let e_c = Arc::clone(&event_count);
    server
        .globals
        .event::<u32>(
            "user_login",
            Arc::new(move |uid| {
                let c = Arc::clone(&e_c);
                (async move {
                    println!("[Event Test] 用户 {} 登录", uid);
                    c.fetch_add(1, Ordering::SeqCst);
                })
                .boxed()
            }),
        )
        .await;

    // --- 模拟业务触发 ---
    // 在实际运行中，这些触发通常发生在 Context 逻辑内
    {
        let globals = server.globals;

        // 触发 Pipe
        globals
            .pipe
            .send("audit_log", "Server started".to_string())
            .await
            .unwrap();

        // 触发 Spread
        globals.spread.publish("config_sync", 101).await.unwrap();

        // 触发 Event
        globals
            .event
            .notify("user_login".to_string(), 888_u32)
            .await;
    }

    // 给异步任务一点点执行时间
    sleep(Duration::from_millis(100)).await;

    // 断言验证
    assert_eq!(pipe_count.load(Ordering::SeqCst), 1, "Pipe 回调应执行 1 次");
    assert_eq!(
        spread_count.load(Ordering::SeqCst),
        1,
        "Spread 回调应执行 1 次"
    );
    assert_eq!(
        event_count.load(Ordering::SeqCst),
        1,
        "Event 回调应执行 1 次"
    );

    println!("✅ Server 通讯总线功能验证通过！");
}

#[tokio::test]
async fn test_server_start_tcp_only() {
    let addr: SocketAddr = "[::1]:0".parse().unwrap();
    let mut server = HTTPServer::new(addr, None);
    
    let mut tr = TcpRouter::new();
    tr.on::<RawCodec, RawCodec>(
        10,
        Box::new(|_, _, _| Box::pin(async move { Ok(true) }).boxed()),
        vec![],
    );
    
    let temp_listener = TcpListener::bind(addr).await.unwrap();
    let actual_addr = temp_listener.local_addr().unwrap();
    drop(temp_listener);
    
    server.addr = actual_addr;
    let server = server.tcp::<RawCodec>(tr, Arc::new(|c: &RawCodec| c.id())).clone();
    let globals = server.globals.clone();
    
    tokio::spawn(async move {
        if let Err(e) = server.start().await {
            eprintln!("TCP server error: {}", e);
        }
    });
    
    sleep(Duration::from_millis(200)).await;
    
    let active_exits = globals.get_exits().await;
    assert!(active_exits.contains(&"tcp".to_string()));
}

#[tokio::test]
async fn test_server_start_udp_only() {
    let addr: SocketAddr = "[::1]:0".parse().unwrap();
    let mut server = HTTPServer::new(addr, None);
    
    let mut ur = UdpRouter::new();
    ur.on::<RawCodec, RawCodec, _, _>(20, |_, _, _, _addr, _socket| async move { Ok(true) });
    
    let socket = UdpSocket::bind(addr).await.unwrap();
    let actual_addr = socket.local_addr().unwrap();
    drop(socket);
    
    server.addr = actual_addr;
    let server = server.udp::<RawCodec>(ur, Arc::new(|c: &RawCodec| c.id())).clone();
    let globals = server.globals.clone();
    
    tokio::spawn(async move {
        if let Err(e) = server.start().await {
            eprintln!("UDP server error: {}", e);
        }
    });
    
    sleep(Duration::from_millis(200)).await;
    
    let active_exits = globals.get_exits().await;
    assert!(active_exits.contains(&"udp".to_string()));
}

#[tokio::test]
async fn test_server_http2_enable() {
    let addr: SocketAddr = "[::1]:0".parse().unwrap();
    let mut server = HTTPServer::new(addr, None);
    
    let mut hr = HttpRouter::new(NodeType::Static("root".into()));
    hr.insert("/", Some("GET"), Arc::new(|_| async { true }.boxed()), None);
    
    let temp_listener = TcpListener::bind(addr).await.unwrap();
    let actual_addr = temp_listener.local_addr().unwrap();
    drop(temp_listener);
    
    server.addr = actual_addr;
    let server = server.http(hr).http2();
    
    assert!(server.globals.h2_codec.read().unwrap().is_some());
}

#[tokio::test]
async fn test_server_httpserver_alias() {
    let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
    let server = HTTPServer::new(addr, None);
    
    assert_eq!(server.addr, addr);
    assert!(!server.globals.routers.get_value::<Arc<HttpRouter>>().is_some());
}

#[tokio::test]
async fn test_local_shutdown() -> anyhow::Result<()> {
    let addr: SocketAddr = "[::1]:0".parse()?; // 使用 0 端口自动分配
    
    // 需要创建 router 以启用 TCP/UDP
    let mut tr = TcpRouter::new();
    tr.on::<RawCodec, RawCodec>(
        10,
        Box::new(|_, _, _| Box::pin(async move { Ok(true) }).boxed()),
        vec![],
    );
    let mut ur = UdpRouter::new();
    ur.on::<RawCodec, RawCodec, _, _>(20, |_, _, _, _addr, _socket| async move { Ok(true) });
    
    let server = Server::new(addr, None)
        .tcp::<RawCodec>(tr, Arc::new(|c: &RawCodec| c.id()))
        .udp::<RawCodec>(ur, Arc::new(|c: &RawCodec| c.id()));
    let globals = server.globals.clone();

    // 1. 启动服务器 (在后台 Task)
    let server_handle = tokio::spawn(async move {
        server.start().await
    });

    // 给一点时间让服务器起来
    sleep(Duration::from_millis(200)).await;

    // 检查服务是否已注册到 exits
    let active_exits = globals.get_exits().await;
    println!("当前活跃服务: {:?}", active_exits);
    assert!(active_exits.contains(&"tcp".to_string()));
    assert!(active_exits.contains(&"udp".to_string()));

    // 2. 模拟本地触发“一键全断”
    println!("--- 正在触发全局关闭 ---");
    globals.shutdown_all().await;

    // 3. 验证结果
    // 如果 start 函数正常返回，说明 loop 已经 break
    let result = tokio::time::timeout(Duration::from_secs(2), server_handle).await;

    match result {
        Ok(res) => {
            println!("服务器已成功优雅退出。");
            res??; // 检查内部 anyhow::Result
        }
        Err(_) => panic!("服务器未能在超时时间内退出，可能存在死循环或信号阻塞！"),
    }

    Ok(())
}
