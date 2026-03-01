#[cfg(test)]
mod aex_tests {
    use aex::communicators::event::Event;
    use aex::connection::context::{ HTTPContext, TypeMapExt };
    use aex::http::meta::HttpMetadata;
    use aex::http::protocol::header::HeaderKey;
    use aex::http::protocol::status::StatusCode;
    use aex::server::{ AexServer, HTTPServer };
    use aex::tcp::types::{ Codec, Command, Frame };
    use futures::FutureExt;
    use std::sync::atomic::Ordering;
    use std::net::SocketAddr;
    use std::sync::Arc;
    use std::sync::atomic::AtomicUsize;
    use tokio::io::AsyncWriteExt;
    use tokio::net::{ TcpListener, TcpStream, UdpSocket };
    use tokio::time::{ Duration, sleep, timeout };

    // 确保引入了 bincode 的宏
    use bincode::{ Decode, Encode };

    use aex::http::router::{ NodeType, Router as HttpRouter };
    use aex::tcp::router::Router as TcpRouter;
    use aex::udp::router::Router as UdpRouter;

    // --- 1. 修正后的 Mock 协议 ---
    #[derive(serde::Serialize, serde::Deserialize, Encode, Decode, Clone, Debug)]
    struct MockProtocol(Vec<u8>);

    impl Frame for MockProtocol {
        fn validate(&self) -> bool {
            !self.0.is_empty()
        }
        fn command(&self) -> Option<&Vec<u8>> {
            Some(&self.0)
        }
        fn payload(&self) -> Option<Vec<u8>> {
            Some(self.0.clone())
        }
    }

    impl Command for MockProtocol {
        fn id(&self) -> u32 {
            self.0.first().cloned().unwrap_or(0) as u32
        }
                fn data(&self) -> &Vec<u8> {
            &self.0
        }
    }

    impl Codec for MockProtocol {
        fn decode(src: &[u8]) -> anyhow::Result<Self> {
            // 模拟异常：如果字节太长或特定字节则报错，验证服务器健壮性
            if src.len() > 1024 {
                return Err(anyhow::anyhow!("OOM Protected"));
            }
            if src == &[0xff, 0xff, 0, 0] {
                return Err(anyhow::anyhow!("Simulated Decode Error"));
            }
            Ok(Self(src.to_vec()))
        }
        fn encode(&self) -> Vec<u8> {
            self.0.clone()
        }
    }

    // --- 2. 统一测试套件 ---
    #[tokio::test]
    async fn test_aex_server_coverage() {
        // 使用 timeout 包裹整个测试，防止死锁导致 CI 卡死
        let test_result = timeout(Duration::from_secs(10), async {
            // 1. 自动分配可用端口
            let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
            let mut server = AexServer::<MockProtocol, MockProtocol, u32>::new(addr);

            // 2. 注册 HTTP 路由
            let mut hr = HttpRouter::new(NodeType::Static("root".into()));

            hr.insert(
                "/",
                Some("GET"),
                Arc::new(|ctx: &mut HTTPContext| {
                    Box::pin(async move {
                        let meta = &mut ctx.local.get_value::<HttpMetadata>().unwrap();
                        meta.status = StatusCode::Ok;
                        meta.headers.insert(HeaderKey::ContentType, "text/plain".to_string());
                        meta.body = b"Hello world!".to_vec();

                        println!("HTTP handler executed, meta prepared: {:?}", meta);

                        println!(
                            "Preparing to send response with local context: {:?}",
                            ctx.local.get_value::<HttpMetadata>().unwrap()
                        );
                        ctx.local.set_value(meta.clone()); // 同步更新回 local，确保后续中间件和处理器能访问到最新的 Metadata

                        true
                    }).boxed()
                }),
                None
            );

            // 3. 注册 TCP 路由 (ID 10)
            let mut tr = TcpRouter::new(|c: &MockProtocol| c.0[0] as u32);
            tr.on(10, |_, _, _| async move { Ok(true) });

            // 4. 注册 UDP 路由 (ID 20)
            let mut ur = UdpRouter::new(|c: &MockProtocol| c.0[0] as u32);
            ur.on(20, |_, addr, socket| async move {
                socket.send_to(b"udp_ok", addr).await?;
                Ok(true)
            });

            // 绑定实际端口
            let temp_listener = TcpListener::bind(addr).await.unwrap();
            let actual_addr = temp_listener.local_addr().unwrap();
            drop(temp_listener); // 释放临时绑定的端口以便 Server 使用

            server.addr = actual_addr;
            let server = server.http(hr).tcp(tr).udp(ur);

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
            let http_res = reqwest
                ::get(format!("http://{}", actual_addr)).await
                .expect("HTTP request failed");
            assert_eq!(http_res.status(), 200);
            assert_eq!(http_res.text().await.unwrap(), "Hello world!");

            // --- 分支测试 B: TCP 正常路由 ---
            println!("Testing TCP Normal...");
            let mut tcp_conn = TcpStream::connect(actual_addr).await.expect("TCP connect failed");
            tcp_conn.write_all(&[10, 0, 0, 1]).await.unwrap(); // 发送 ID 10
            sleep(Duration::from_millis(100)).await;

            // --- 分支测试 C: TCP 异常包 (触发解包失败但不崩溃) ---
            println!("Testing TCP Robustness...");
            let mut tcp_bad = TcpStream::connect(actual_addr).await.unwrap();
            tcp_bad.write_all(&[0xff, 0xff, 0, 0]).await.unwrap(); // 触发 MockProtocol::decode 中的错误
            sleep(Duration::from_millis(100)).await;

            // --- 分支测试 D: UDP 正常路由与回包 ---
            println!("Testing UDP...");
            let udp_client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
            udp_client.send_to(&[20, 0, 0, 2], actual_addr).await.unwrap(); // 发送 ID 20
            let mut buf = [0u8; 1024];
            let (len, _) = timeout(Duration::from_secs(2), udp_client.recv_from(&mut buf)).await
                .expect("UDP response timeout")
                .expect("UDP recv failed");
            assert_eq!(&buf[..len], b"udp_ok");

            // --- 分支测试 E: UDP 未匹配路由 ---
            println!("Testing UDP Mismatch...");
            udp_client.send_to(&[99, 0, 0, 0], actual_addr).await.unwrap(); // ID 99
            sleep(Duration::from_millis(100)).await;

            println!("✅ 所有覆盖率分支已跑通，服务器运行平稳。");
        }).await;

        if test_result.is_err() {
            panic!("Test timed out! Possible deadlock or server not responding.");
        }
    }

    #[tokio::test]
    async fn test_server_communication_bus() {
        let addr = "127.0.0.1:0".parse().unwrap(); // 自动分配可用端口
        let server = HTTPServer::new(addr);

        // 准备计数器
        let pipe_count = Arc::new(AtomicUsize::new(0));
        let spread_count = Arc::new(AtomicUsize::new(0));
        let event_count = Arc::new(AtomicUsize::new(0));

        // 1. 测试 Pipe (N:1)
        let p_c = Arc::clone(&pipe_count);
        server.pipe::<String>(
            "audit_log",
            Box::new(move |msg| {
                let c = Arc::clone(&p_c);
                (
                    async move {
                        println!("[Pipe Test] 收到日志: {}", msg);
                        c.fetch_add(1, Ordering::SeqCst);
                    }
                ).boxed()
            })
        ).await;

        // 2. 测试 Spread (1:N)
        let s_c = Arc::clone(&spread_count);
        server.spread::<i32>(
            "config_sync",
            Box::new(move |val| {
                let c = Arc::clone(&s_c);
                (
                    async move {
                        println!("[Spread Test] 收到配置版本: {}", val);
                        c.fetch_add(1, Ordering::SeqCst);
                    }
                ).boxed()
            })
        ).await;

        // 3. 测试 Event (M:N)
        let e_c = Arc::clone(&event_count);
        server.event::<u32>(
            "user_login",
            Arc::new(move |uid| {
                let c = Arc::clone(&e_c);
                (
                    async move {
                        println!("[Event Test] 用户 {} 登录", uid);
                        c.fetch_add(1, Ordering::SeqCst);
                    }
                ).boxed()
            })
        ).await;

        // --- 模拟业务触发 ---
        // 在实际运行中，这些触发通常发生在 Context 逻辑内
        {
            let globals = server.globals.write().await;

            // 触发 Pipe
            globals.pipe.send("audit_log", "Server started".to_string()).await.unwrap();

            // 触发 Spread
            globals.spread.publish("config_sync", 101).await.unwrap();

            // 触发 Event
            globals.event.notify("user_login".to_string(), 888_u32).await;
        }

        // 给异步任务一点点执行时间
        sleep(Duration::from_millis(100)).await;

        // 断言验证
        assert_eq!(pipe_count.load(Ordering::SeqCst), 1, "Pipe 回调应执行 1 次");
        assert_eq!(spread_count.load(Ordering::SeqCst), 1, "Spread 回调应执行 1 次");
        assert_eq!(event_count.load(Ordering::SeqCst), 1, "Event 回调应执行 1 次");

        println!("✅ AexServer 通讯总线功能验证通过！");
    }
}
