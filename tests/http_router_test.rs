#[cfg(test)]
mod tests {
    use std::{
        net::SocketAddr,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
        time::Duration,
    };

    use aex::{
        all,
        connection::context::{HTTPContext, TypeMapExt},
        exe, get,
        http::{
            meta::HttpMetadata,
            protocol::{header::HeaderKey, status::StatusCode},
            router::{NodeType, Router},
            types::{Executor, to_executor},
        },
        route,
        server::{AexServer, HTTPServer},
        tcp::types::{Codec, Command, Frame},
    };
    use bincode::{Decode, Encode};
    use futures::FutureExt;
    use tokio::time::sleep;

    #[derive(serde::Serialize, serde::Deserialize, Encode, Decode, Clone, Debug)]
    pub struct MockProtocol(Vec<u8>);

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

    #[test]
    fn test_header_key_standard_match() {
        // 测试标准 Header 是否能准确解析并还原
        let cases = [
            ("Content-Type", HeaderKey::ContentType),
            ("Host", HeaderKey::Host),
            ("Authorization", HeaderKey::Authorization),
        ];

        for (input, expected_variant) in cases {
            let parsed = HeaderKey::from_str(input).unwrap();
            assert_eq!(parsed, expected_variant);
            assert_eq!(parsed.as_str(), input);
        }
    }

    #[test]
    fn test_header_key_case_insensitivity() {
        // 验证大小写不敏感解析
        let mixed_cases = ["content-type", "CONTENT-TYPE", "CoNtEnT-TyPe"];
        for input in mixed_cases {
            let parsed = HeaderKey::from_str(input).unwrap();
            assert_eq!(parsed, HeaderKey::ContentType);
            // 无论输入如何，as_str() 应当返回宏定义的标准规范格式
            assert_eq!(parsed.as_str(), "Content-Type");
        }
    }

    #[test]
    fn test_header_key_custom_fallback() {
        // 验证非标准 Header 是否自动转为 Custom 变体
        let custom_name = "X-My-Custom-Header";
        let parsed = HeaderKey::from_str(custom_name).unwrap();

        match &parsed {
            HeaderKey::Custom(s) => assert_eq!(s, custom_name),
            _ => panic!("应当匹配为 Custom 变体"),
        }

        assert_eq!(parsed.as_str(), custom_name);
    }

    #[test]
    fn test_header_key_display_trait() {
        // 验证 Display 实现是否正确调用了 as_str
        let key = HeaderKey::UserAgent;
        assert_eq!(format!("{}", key), "User-Agent");

        let custom = HeaderKey::Custom("X-Foo".to_string());
        assert_eq!(format!("{}", custom), "X-Foo");
    }

    #[test]
    fn test_header_key_roundtrip() {
        // 验证：字符串 -> 枚举 -> 字符串 的完整性
        // 测试标准键（宏内部会进行 to_ascii_lowercase 匹配）
        let raw_standard = "Accept-Encoding";
        let key = HeaderKey::from_str(raw_standard).unwrap();
        assert_eq!(key.as_str(), raw_standard);

        // 测试自定义键
        let raw_custom = "X-Tracing-Id";
        let key_custom = HeaderKey::from_str(raw_custom).unwrap();
        assert_eq!(key_custom.as_str(), raw_custom);
    }

    #[test]
    fn test_header_key_trimming() {
        // 验证 from_str 是否正确处理空格
        let input = "  Content-Type  ";
        let parsed = HeaderKey::from_str(input).unwrap();
        assert_eq!(parsed, HeaderKey::ContentType);
    }

    #[tokio::test]
    async fn test_server_auto_parsing_and_routing() {
        // 1. 构建 Router 场景
        let mut hr = Router::new(NodeType::Static("root".into()));

        hr.insert(
            "/api/user/:id",
            Some("GET"),
            Arc::new(|ctx: &mut HTTPContext| {
                async move {
                    let mut meta = ctx.local.get_value::<HttpMetadata>().unwrap();
                    let user_id = meta
                        .params
                        .as_ref()
                        .and_then(|p| p.data.as_ref())
                        .and_then(|d| d.get("id"))
                        .cloned()
                        .unwrap_or_default();

                    let custom_key = HeaderKey::from_str("X-Aex-Auth").unwrap();
                    let auth_val = meta.headers.get(&custom_key).cloned().unwrap_or_default();

                    meta.status = StatusCode::Ok;
                    meta.body = format!("User:{}, Auth:{}", user_id, auth_val).into_bytes();

                    ctx.local.set_value(meta);
                    true
                }
                .boxed()
            }),
            None,
        );

        // 2. 初始化端口并启动 Server
        // 获取一个空闲端口
        let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
        let actual_addr = listener.local_addr().unwrap();
        drop(listener); // 释放端口给 AexServer

        let server = AexServer::<MockProtocol, MockProtocol, u32>::new(actual_addr);
        let server = server.http(hr);

        tokio::spawn(async move {
            if let Err(e) = server.start().await {
                eprintln!("Server exit: {}", e);
            }
        });

        // 3. 关键改进：使用 tokio::time::sleep 并增加连接重试逻辑
        let client = reqwest::Client::new();
        let mut res = None;
        let url = format!("http://{}/api/user/9527", actual_addr);

        // 最多等待 1 秒 (100ms * 10)
        for _ in 0..10 {
            tokio::time::sleep(Duration::from_millis(100)).await;
            match client
                .get(&url)
                .header("X-Aex-Auth", "secret-token-v3")
                .send()
                .await
            {
                Ok(response) => {
                    res = Some(response);
                    break;
                }
                Err(_) => continue, // Server 可能还没准备好
            }
        }

        let res = res.expect("服务器在重试多次后仍未就绪 (Connection Refused)");

        // 4. 最终验证
        assert_eq!(res.status().as_u16(), 200);
        let body = res.text().await.unwrap();
        assert_eq!(body, "User:9527, Auth:secret-token-v3");

        println!("✅ AexServer 全链路 Router 自动化集成测试通过！");
    }

    #[tokio::test]
    async fn test_full_stack_wildcard_and_middleware() {
        // --- 1. 准备共享状态用于验证 ---
        let mw_exec_order = Arc::new(std::sync::Mutex::new(Vec::new()));
        let wildcard_hit_count = Arc::new(AtomicUsize::new(0));

        // --- 2. 构建 Router 场景 ---
        let mut hr = Router::new(NodeType::Static("root".into()));

        // A. 注册中间件 A (记录轨迹)
        let t1 = mw_exec_order.clone();

        // B. 注册中间件 B (记录轨迹并允许通过)
        let t2 = mw_exec_order.clone();

        // 2. 在定义中间件时，显式指定其返回值类型
        let mw_a: Arc<Executor> = Arc::new(move |_ctx| {
            let t = t1.clone();
            async move {
                t.lock().unwrap().push("MW_A");
                true
            }
            .boxed()
        });

        let mw_b: Arc<Executor> = Arc::new(move |_ctx| {
            let t = t2.clone();
            async move {
                t.lock().unwrap().push("MW_B");
                true
            }
            .boxed()
        });

        // C. 注册通配符路由处理器
        let count = wildcard_hit_count.clone();
        hr.insert(
            "/assets/*",
            Some("GET"),
            Arc::new(move |ctx: &mut HTTPContext| {
                let c = count.clone();
                async move {
                    c.fetch_add(1, Ordering::SeqCst);
                    let mut meta = ctx.local.get_value::<HttpMetadata>().unwrap();
                    meta.status = StatusCode::Ok;
                    meta.body = b"Wildcard Matched".to_vec();
                    ctx.local.set_value(meta);
                    true
                }
                .boxed()
            }),
            Some(vec![mw_a, mw_b]), // 挂载两个中间件
        );

        // --- 3. 启动服务器 ---
        let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
        let actual_addr = listener.local_addr().unwrap();
        drop(listener);

        let server = AexServer::<MockProtocol, MockProtocol, u32>::new(actual_addr);
        let server = server.http(hr);

        tokio::spawn(async move {
            let _ = server.start().await;
        });

        // --- 4. 发起真实请求验证 ---
        let client = reqwest::Client::new();
        let mut res = None;

        // 尝试请求一个深层路径，验证通配符能否捕获
        for _ in 0..10 {
            sleep(Duration::from_millis(100)).await;
            if let Ok(r) = client
                .get(format!("http://{}/assets/css/main.css", actual_addr))
                .send()
                .await
            {
                res = Some(r);
                break;
            }
        }

        let response = res.expect("Server failed to respond");

        // --- 5. 断言验证 ---

        // 验证 1: 路径匹配
        assert_eq!(response.status().as_u16(), 200);
        assert_eq!(response.text().await.unwrap(), "Wildcard Matched");

        // 验证 2: 通配符命中计数
        assert_eq!(wildcard_hit_count.load(Ordering::SeqCst), 1);

        // 验证 3: 中间件执行顺序 (MW_A -> MW_B)
        let order = mw_exec_order.lock().unwrap();
        assert_eq!(*order, vec!["MW_A", "MW_B"]);
    }

    #[tokio::test]
    async fn test_middleware_interruption_full_stack() {
        // 验证中间件返回 false 时，处理器不应被执行
        let mut hr = Router::new(NodeType::Static("root".into()));
        let handler_executed = Arc::new(AtomicUsize::new(0));

        // 拦截中间件
        let mw_blocker: Arc<Executor> = Arc::new(|ctx: &mut HTTPContext| {
            async move {
                let mut meta = ctx.local.get_value::<HttpMetadata>().unwrap();
                meta.status = StatusCode::Forbidden; // 403
                meta.body = b"Blocked".to_vec();
                ctx.local.set_value(meta);
                false // 停止后续执行
            }
            .boxed()
        });

        let h_count = handler_executed.clone();
        hr.insert(
            "/admin",
            Some("GET"),
            Arc::new(move |_| {
                let c = h_count.clone();
                async move {
                    c.fetch_add(1, Ordering::SeqCst);
                    true
                }
                .boxed()
            }),
            Some(vec![mw_blocker]),
        );

        // 启动 Server (逻辑同上，略)
        let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let actual_addr = tokio::net::TcpListener::bind(addr)
            .await
            .unwrap()
            .local_addr()
            .unwrap();
        let server = AexServer::<MockProtocol, MockProtocol, u32>::new(actual_addr);
        let server = server.http(hr);
        tokio::spawn(async move {
            let _ = server.start().await;
        });

        sleep(Duration::from_millis(200)).await;
        let res = reqwest::get(format!("http://{}/admin", actual_addr))
            .await
            .unwrap();

        // 断言验证
        assert_eq!(res.status().as_u16(), 403);
        let text = res.text().await.unwrap();
        assert!(text.contains("Blocked"));
        assert_eq!(handler_executed.load(Ordering::SeqCst), 0); // 处理器不应运行
    }

    #[tokio::test]
    async fn test_router_404_not_found() {
        let hr = Router::new(NodeType::Static("root".into())); // 空路由

        let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let actual_addr = tokio::net::TcpListener::bind(addr)
            .await
            .unwrap()
            .local_addr()
            .unwrap();

        let server = AexServer::<MockProtocol, MockProtocol, u32>::new(actual_addr);
        let server = server.http(hr);
        tokio::spawn(async move {
            let _ = server.start().await;
        });

        tokio::time::sleep(Duration::from_millis(200)).await;

        let res = reqwest::get(format!("http://{}/undefined/path", actual_addr))
            .await
            .unwrap();

        // 验证 handle_request 最后的 else 分支是否正确设置了 StatusCode::NotFound
        assert_eq!(res.status().as_u16(), 404);
    }
    // #[tokio::test]
    //     async fn test_form_body_auto_parsing() {
    //         let mut hr = Router::new(NodeType::Static("root".into()));

    //         hr.insert(
    //             "/submit",
    //             Some("POST"),
    //             Arc::new(|ctx: &mut HTTPContext| {
    //                 async move {
    //                     let mut meta = ctx.local.get_value::<HttpMetadata>().unwrap();

    //                     // 逻辑验证：从解析后的 Params 中提取
    //                     let user = meta.params.as_ref()
    //                         .and_then(|p| p.form.as_ref())
    //                         .and_then(|f| f.get("user"))
    //                         .cloned()
    //                         .unwrap_or_default();

    //                     meta.status = StatusCode::Ok;
    //                     meta.body = format!("User:{}", user.join("")).into_bytes();
    //                     ctx.local.set_value(meta);
    //                     true
    //                 }.boxed()
    //             }),
    //             None,
    //         );

    //         // --- 启动服务器 ---
    //         let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
    //         let actual_addr = tokio::net::TcpListener::bind(addr).await.unwrap().local_addr().unwrap();
    //         let mut server = AexServer::<MockProtocol, MockProtocol, u32>::new(actual_addr);
    //         let server = server.http(hr);
    //         tokio::spawn(async move { let _ = server.start().await; });

    //         tokio::time::sleep(Duration::from_millis(200)).await;

    //         // --- 使用最基础的请求方式 ---
    //         let client = reqwest::Client::new();
    //         let body_str = "user=Gemini&age=20";

    //         let res = client
    //             .post(format!("http://{}/submit", actual_addr))
    //             // 显式设置这些 Header，触发你 handle_request 中的 if 条件
    //             .header("Content-Type", "application/x-www-form-urlencoded")
    //             .header("Content-Length", body_str.len().to_string())
    //             .body(body_str.to_string()) // 显式转为 String 以满足 RequestBuilder::body
    //             .send()
    //             .await
    //             .expect("Request failed");

    //         assert_eq!(res.status().as_u16(), 200);
    //         assert_eq!(res.text().await.unwrap(), "User:Gemini");
    //     }

    #[tokio::test]
    async fn test_router_404_fallback() {
        let hr = Router::new(NodeType::Static("root".into())); // 空路由

        let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let actual_addr = tokio::net::TcpListener::bind(addr)
            .await
            .unwrap()
            .local_addr()
            .unwrap();
        let server = AexServer::<MockProtocol, MockProtocol, u32>::new(actual_addr);
        let server = server.http(hr);
        tokio::spawn(async move {
            let _ = server.start().await;
        });

        tokio::time::sleep(Duration::from_millis(200)).await;

        let res = reqwest::get(format!("http://{}/not_exists", actual_addr))
            .await
            .unwrap();

        // 验证 handle_request 最后的 else 分支：设置了 404
        assert_eq!(res.status().as_u16(), 404);
    }

    #[tokio::test]
    async fn test_form_body_auto_parsing() {
        let mut hr = Router::new(NodeType::Static("root".into()));

        hr.insert(
            "/submit",
            Some("POST"),
            Arc::new(|ctx: &mut HTTPContext| {
                async move {
                    let mut meta = ctx.local.get_value::<HttpMetadata>().unwrap();

                    println!("meta.params = {:?}", meta.params);

                    // 验证 Params.form 是否被正确注入
                    let user = meta
                        .params
                        .as_ref()
                        .and_then(|p| p.form.as_ref())
                        .and_then(|f| f.get("user"))
                        .cloned()
                        .unwrap_or_default();

                    meta.status = StatusCode::Ok;
                    // 关键：必须设置 Body 响应，否则客户端会认为服务器没说完
                    meta.body = format!("User:{}", user.get(0).unwrap()).into_bytes();

                    println!("meta = {:?}", meta);

                    ctx.local.set_value(meta);
                    true
                }
                .boxed()
            }),
            None,
        );

        let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let actual_addr = tokio::net::TcpListener::bind(addr)
            .await
            .unwrap()
            .local_addr()
            .unwrap();
        let server = AexServer::<MockProtocol, MockProtocol, u32>::new(actual_addr);
        let server = server.http(hr);
        tokio::spawn(async move {
            let _ = server.start().await;
        });

        sleep(Duration::from_millis(200)).await;

        let client = reqwest::Client::new();
        let body_str = "user=Gemini&age=20";

        // 这里的请求必须完全匹配你 handle_request 里的条件
        let res = client
            .post(format!("http://{}/submit", actual_addr))
            .header("Content-Type", "application/x-www-form-urlencoded")
            // Content-Length 必须精确，否则 read_exact 会一直等待
            .header("Content-Length", body_str.len().to_string())
            .body(body_str)
            .send()
            .await
            .expect("Request failed");

        assert_eq!(res.status().as_u16(), 200);
        let text = res.text().await.unwrap();
        assert_eq!(text, "User:Gemini");
    }

    #[tokio::test]
    async fn test_wildcard_method_matching() {
        let mut hr = Router::new(NodeType::Static("root".into()));

        // 1. 注册一个通用处理器 (Method 为 None 或 "*")
        // 假设该处理器应处理除明确定义外的所有方法
        hr.insert(
            "/universal",
            None, // 内部会转为 "*"
            Arc::new(|ctx: &mut HTTPContext| {
                async move {
                    let mut meta = ctx.local.get_value::<HttpMetadata>().unwrap();
                    let method = meta.method.to_str().to_owned();

                    meta.status = StatusCode::Ok;
                    meta.body = format!("Method:{} handled by *", method).into_bytes();

                    // 记得更新 Content-Length 避免之前的 IncompleteBody 错误
                    let cl_key = HeaderKey::from_str("Content-Length").unwrap();
                    meta.headers.insert(cl_key, meta.body.len().to_string());

                    ctx.local.set_value(meta);
                    true
                }
                .boxed()
            }),
            None,
        );

        // --- 启动 Server ---
        let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let actual_addr = tokio::net::TcpListener::bind(addr)
            .await
            .unwrap()
            .local_addr()
            .unwrap();
        let server = AexServer::<MockProtocol, MockProtocol, u32>::new(actual_addr);
        let server = server.http(hr);
        tokio::spawn(async move {
            let _ = server.start().await;
        });

        tokio::time::sleep(Duration::from_millis(200)).await;
        let client = reqwest::Client::new();

        // 验证 1: 发送 PUT 请求（未精确定义）
        let res_put = client
            .put(format!("http://{}/universal", actual_addr))
            .send()
            .await
            .unwrap();
        assert_eq!(res_put.status().as_u16(), 200);
        assert_eq!(res_put.text().await.unwrap(), "Method:PUT handled by *");

        // 验证 2: 发送 DELETE 请求
        let res_delete = client
            .delete(format!("http://{}/universal", actual_addr))
            .send()
            .await
            .unwrap();
        assert_eq!(
            res_delete.text().await.unwrap(),
            "Method:DELETE handled by *"
        );
    }

    #[tokio::test]
    async fn test_wildcard_method_middleware() {
        let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();

        let actual_addr = tokio::net::TcpListener::bind(addr)
            .await
            .unwrap()
            .local_addr()
            .unwrap();

        let mut hr = Router::new(NodeType::Static("root".into()));
        let mw_hit_count = Arc::new(AtomicUsize::new(0));

        let server = AexServer::<MockProtocol, MockProtocol, u32>::new(actual_addr);

        let count = mw_hit_count.clone();
        let mw_any: Arc<Executor> = Arc::new(move |_| {
            let c = count.clone();
            async move {
                c.fetch_add(1, Ordering::SeqCst);
                true
            }
            .boxed()
        });

        // 在通配符方法上插入中间件
        hr.insert(
            "/api",
            None,
            Arc::new(|_| async { true }.boxed()),
            Some(vec![mw_any]),
        );

        // ... 启动 Server ...

        let server = server.http(hr);
        tokio::spawn(async move {
            let _ = server.start().await;
        });

        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

        let client = reqwest::Client::new();

        // 分别发送 GET 和 POST
        // let _ = client
        //     .get(format!("http://{}/api", actual_addr))
        //     .send()
        //     .await;

        let res_get = client
            .get(format!("http://{}/api", actual_addr))
            .send()
            .await;

        match res_get {
            Ok(_) => println!("GET request sent successfully"),
            Err(e) => println!("GET request failed: {:?}", e),
        }
        let _ = client
            .post(format!("http://{}/api", actual_addr))
            .send()
            .await;

        // 验证中间件被执行了 2 次
        assert_eq!(mw_hit_count.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn test_wildcard_method_middleware_with_executor() {
        let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let actual_addr = tokio::net::TcpListener::bind(addr)
            .await
            .unwrap()
            .local_addr()
            .unwrap();

        let mut hr = Router::new(NodeType::Static("root".into()));
        let mw_hit_count = Arc::new(AtomicUsize::new(0));

        // 1. 使用原本的闭包方式定义的中间件
        let count = mw_hit_count.clone();
        let mw_counter = to_executor(move |_| {
            let c = count.clone();
            async move {
                c.fetch_add(1, Ordering::SeqCst);
                true
            }
            .boxed()
        });

        // 2. 使用 to_executor 定义一个额外的中间件（比如检查特定的 Header）
        let mw_header_check = to_executor(|_ctx| {
            async move {
                // 这里可以做一些逻辑判断
                println!("Middleware 2: Checking request...");
                true
            }
            .boxed()
        });

        // 将两个中间件都放入列表
        hr.insert(
            "/api",
            None, // 匹配所有方法 (*)
            Arc::new(|_| async { true }.boxed()),
            Some(vec![mw_counter, mw_header_check]), // 顺序执行
        );

        let server = HTTPServer::new(actual_addr).http(hr);
        tokio::spawn(async move {
            let _ = server.start().await;
        });

        // 给服务器起步时间，避免 Connection Refused
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        let client = reqwest::Client::new();

        // 分别发送 GET 和 POST
        let _ = client
            .get(format!("http://{}/api", actual_addr))
            .send()
            .await;
        let _ = client
            .post(format!("http://{}/api", actual_addr))
            .send()
            .await;

        // 每个请求命中了 1 个 method_key 为 * 的位置，里面有 2 个中间件
        // 总计增加应该是 2 (requests) * 1 (matching node) = 2
        // 注意：如果你在 handle_request 逻辑里循环执行了 vec 里的所有 mw，
        // 那么 c.fetch_add 还是会被调用两次。
        assert_eq!(mw_hit_count.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn test_full_macros_suite() {
         // 使用标准库锁简化测试代码中的同步操作

        let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let actual_addr = tokio::net::TcpListener::bind(addr)
            .await
            .unwrap()
            .local_addr()
            .unwrap();

        let mut hr = Router::new(NodeType::Static("root".into()));
        // let trace = Arc::new(Mutex::new(Vec::<u8>::new()));

        // --- 1. 使用 exe! 定义带 Pre 处理的中间件 ---
        // let t1 = trace.clone();
        let mw_info = exe!(
            |ctx, info| {
                async move { true }.await;
                true
            },
            |_ctx| { "info".to_string() }
        );

        // --- 2. 使用 exe! 定义 Handler (修正版) ---
        let handler: Arc<Executor> = exe!(|ctx| {
            let mut meta = ctx.local.get_value::<HttpMetadata>().unwrap();
            meta.body = b"Macro OK".to_vec();
            ctx.local.set_value(meta);
            true
        });

        // --- 3. 使用 route! 配合不同方法宏注册 ---

        // 注册通配符方法路由到 /api/*
        route!(hr, all!("/api/all", handler.clone(), vec![mw_info.clone()]));

        // 注册特定 GET 路由
        route!(hr, get!("/api/specific", handler, vec![mw_info]));

        // --- 4. 启动服务器 ---
        let server = HTTPServer::new(actual_addr).http(hr);
        tokio::spawn(async move {
            let _ = server.start().await;
        });

        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
        let client = reqwest::Client::new();

        // --- 5. 验证请求 ---

        // 测试 all! 宏 (POST 请求)
        let _ = client
            .post(format!("http://{}/api/all", actual_addr))
            .send()
            .await;

        // 测试 get! 宏
        let _ = client
            .get(format!("http://{}/api/specific", actual_addr))
            .send()
            .await;

        // --- 6. 断言结果 ---
        // let results = trace.lock().unwrap();
        // assert_eq!(results.len(), 2);
        // assert_eq!(results[0], "POST:/api/all");
        // assert_eq!(results[1], "GET:/api/specific");

        // println!("Macro Test Trace: {:?}", *results);
    }

    #[tokio::test]
    async fn test_full_macros_suite1() {
        let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let actual_addr = tokio::net::TcpListener::bind(addr)
            .await
            .unwrap()
            .local_addr()
            .unwrap();

        let mut hr = Router::new(NodeType::Static("root".into()));

        // --- 1. 中间件：演示 pre 块提取数据并存入 context ---
        let mw_info = exe!(
            |ctx, info| {
                // 将 pre 块提取的 info 放入 Response Header 传回 client
                let mut meta = ctx.local.get_value::<HttpMetadata>().unwrap();
                meta.headers.insert(HeaderKey::from_str("X-Macro-Info").unwrap(), info);
                ctx.local.set_value(meta);
                
                async move { true }.await;
                true
            },
            |_ctx| { 
                // pre 块：这里可以根据不同请求生成动态数据
                "processed-by-macro".to_string() 
            }
        );

        // --- 2. Handler：修改 Body ---
        let handler: Arc<Executor> = exe!(|ctx| {
            let mut meta = ctx.local.get_value::<HttpMetadata>().unwrap();
            meta.body = b"Macro OK".to_vec();
            ctx.local.set_value(meta);
            true
        });

        // --- 3. 注册路由 ---
        route!(hr, all!("/api/all", handler.clone(), vec![mw_info.clone()]));
        route!(hr, get!("/api/specific", handler, vec![mw_info]));

        // --- 4. 启动服务器 ---
        let server = HTTPServer::new(actual_addr).http(hr);
        tokio::spawn(async move {
            let _ = server.start().await;
        });

        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
        let client = reqwest::Client::new();

        // --- 5. 验证请求 ---

        // 验证 POST 到 all! 路由
        let resp_all = client
            .post(format!("http://{}/api/all", actual_addr))
            .send()
            .await
            .expect("POST request failed");

        // 检查 Header 是否含有中间件注入的信息
        assert_eq!(resp_all.headers().get("X-Macro-Info").unwrap(), "processed-by-macro");
        // 检查 Body 是否由 Handler 正确设置
        assert_eq!(resp_all.text().await.unwrap(), "Macro OK");

        // 验证 GET 到 specific 路由
        let resp_get = client
            .get(format!("http://{}/api/specific", actual_addr))
            .send()
            .await
            .expect("GET request failed");

        assert_eq!(resp_get.status(), 200);
        assert_eq!(resp_get.text().await.unwrap(), "Macro OK");

        println!("Full macro suite test passed!");
    }
}
