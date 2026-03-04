#[cfg(test)]
mod websocket_tests {
    use aex::{
        connection::context::TypeMapExt,
        exe, get,
        http::{
            meta::HttpMetadata,
            middlewares::websocket::WebSocket,
            protocol::{header::HeaderKey, method::HttpMethod},
            router::{NodeType, Router},
        },
        post, route,
        server::HTTPServer, tcp::types::{Command, RawCodec},
    };
    use std::{collections::HashMap, net::SocketAddr, sync::Arc};
    use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader, BufWriter};

    // 辅助工具：生成合法的 WebSocket 帧
    fn create_ws_frame(opcode: u8, payload: &[u8], masked: bool) -> Vec<u8> {
        let mut frame = Vec::new();
        frame.push(0x80 | opcode);
        let mask_bit = if masked { 0x80 } else { 0x00 };

        if payload.len() < 126 {
            frame.push(mask_bit | (payload.len() as u8));
        } else {
            frame.push(mask_bit | 126);
            frame.extend_from_slice(&(payload.len() as u16).to_be_bytes());
        }

        if masked {
            let mask = [1, 2, 3, 4];
            frame.extend_from_slice(&mask);
            let mut masked_payload = payload.to_vec();
            for i in 0..masked_payload.len() {
                masked_payload[i] ^= mask[i % 4];
            }
            frame.extend_from_slice(&masked_payload);
        } else {
            frame.extend_from_slice(payload);
        }
        frame
    }

    #[test]
    fn test_check_handshake_logic() {
        let mut headers = HashMap::new();
        headers.insert(HeaderKey::Upgrade, "websocket".to_string());
        headers.insert(HeaderKey::Connection, "Upgrade".to_string());

        // 正常情况
        assert!(WebSocket::check(HttpMethod::GET, &headers));

        // 错误的 Method
        assert!(!WebSocket::check(HttpMethod::POST, &headers));

        // 缺失 Header
        let mut h2 = headers.clone();
        h2.remove(&HeaderKey::Upgrade);
        assert!(!WebSocket::check(HttpMethod::GET, &h2));

        // Connection 不包含 Upgrade
        let mut h3 = headers.clone();
        h3.insert(HeaderKey::Connection, "keep-alive".to_string());
        assert!(!WebSocket::check(HttpMethod::GET, &h3));
    }

    #[tokio::test]
    async fn test_handshake_success() {
        let (_client, _server_read) = tokio::io::duplex(1024);
        let (_server_read, server_write) = tokio::io::duplex(1024);
        let mut writer = BufWriter::new(server_write);

        let mut headers = HashMap::new();
        headers.insert(
            HeaderKey::SecWebSocketKey,
            "dGhlIHNhbXBsZSBub25jZQ==".to_string(),
        );

        WebSocket::handshake(&mut writer, &headers).await.unwrap();
        // 验证逻辑是否执行（这里通常会通过拦截写入的数据验证 accept_key 是否为 s3pPLMBiTxaQ9kYGzzhZRbK+xOo=）
    }

    #[tokio::test]
    async fn test_read_full_data_frames() {
        use tokio::time::{Duration, timeout};

        // 1. 正确创建一对双工流：client <-> server
        let (client, server) = tokio::io::duplex(1024);

        // 将 server 端拆分为 reader 和 writer
        let (server_read, server_write) = tokio::io::split(server);
        let mut reader = BufReader::new(server_read);
        let mut writer = BufWriter::new(server_write);

        // 2. 准备数据
        let payload = b"hello";
        let frame = create_ws_frame(0x1, payload, true); // 发送一个 Text 帧

        // 3. 客户端发送数据
        tokio::spawn(async move {
            let mut client_handle = client;
            // 写入一帧数据
            if let Err(e) = client_handle.write_all(&frame).await {
                eprintln!("Client write error: {:?}", e);
            }
            // 💡 保持连接直到测试完成或由服务端关闭
        });

        // 4. 服务端读取：增加超时控制防止卡死
        let res = timeout(Duration::from_secs(2), async {
            WebSocket::read_full(&mut reader, &mut writer).await
        })
        .await;

        // 5. 验证结果
        match res {
            Ok(Ok((opcode, data))) => {
                assert_eq!(opcode, 0x1);
                assert_eq!(data, payload);
            }
            Ok(Err(e)) => panic!("读取失败: {:?}", e),
            Err(_) => panic!(
                "测试超时：read_full 可能在读取完第一帧后没有 return，而是继续 loop 等待下一帧"
            ),
        }
    }

    #[tokio::test]
    async fn test_read_full_ping_pong() {
        let (server_in, client_out) = tokio::io::duplex(1024);
        let (client_in, server_out) = tokio::io::duplex(1024);

        let mut reader = BufReader::new(server_in);
        let mut writer = BufWriter::new(server_out);
        let mut client_reader = client_in;
        let mut client_writer = client_out;

        // 1. 发送 Ping，预期 read_full 会自动回复 Pong 并继续 loop 读取下一帧
        let ping_frame = create_ws_frame(0x9, b"ping", true);
        let text_frame = create_ws_frame(0x1, b"end", true);

        client_writer.write_all(&ping_frame).await.unwrap();
        client_writer.write_all(&text_frame).await.unwrap();

        let (opcode, data) = WebSocket::read_full(&mut reader, &mut writer)
            .await
            .unwrap();

        // 验证收到了文本帧
        assert_eq!(opcode, 0x1);
        assert_eq!(data, b"end");

        // 验证客户端收到了自动回复的 Pong (0x8a)
        let mut resp = [0u8; 2];
        client_reader.read_exact(&mut resp).await.unwrap();
        assert_eq!(resp[0], 0x8a);
    }

    #[tokio::test]
    async fn test_read_full_error_unmasked() {
        // 创建双工流
        let (client, server) = tokio::io::duplex(1024);
        let mut client_writer = client;
        let (server_read, server_write) = tokio::io::split(server);

        let mut reader = BufReader::new(server_read);
        let mut writer = BufWriter::new(server_write);

        // 构造一个明确未掩码的帧: masked = false
        // create_ws_frame(opcode, payload, masked)
        let frame = create_ws_frame(0x1, b"fail", false);

        // 发送数据
        tokio::spawn(async move {
            if let Err(e) = client_writer.write_all(&frame).await {
                eprintln!("client write error: {:?}", e);
            }
            // 💡 重点：保持 client 存活一会儿，防止过早关闭导致 Broken Pipe 或 EOF 覆盖了协议错误
            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        });

        let res = WebSocket::read_full(&mut reader, &mut writer).await;

        // 验证结果
        match res {
            Err(e) => {
                let err_msg = e.to_string();
                // 打印出来方便调试，万一以后改了错误文案
                assert!(
                    err_msg.contains("protocol error"),
                    "Expected 'protocol error', but got: '{}'",
                    err_msg
                );
            }
            Ok((op, payload)) => {
                panic!(
                    "Should have failed with protocol error, but got Ok(({:0x}, {:?}))",
                    op, payload
                );
            }
        }
    }

    #[test]
    fn test_parse_close_payload() {
        // 正常
        let p1 = vec![0x03, 0xE8]; // 1000
        assert_eq!(WebSocket::parse_close_payload(&p1).unwrap().0, 1000);

        // 带 Reason
        let mut p2 = vec![0x03, 0xE8];
        p2.extend_from_slice(b"bye");
        let (code, reason) = WebSocket::parse_close_payload(&p2).unwrap();
        assert_eq!(code, 1000);
        assert_eq!(reason, Some("bye"));

        // 空 Payload (合法)
        assert_eq!(WebSocket::parse_close_payload(&[]).unwrap().0, 1000);

        // 非法 Code
        let p3 = vec![0x00, 0x00];
        assert!(WebSocket::parse_close_payload(&p3).is_err());

        // 非法长度
        assert!(WebSocket::parse_close_payload(&[0x03]).is_err());
    }

    #[tokio::test]
    async fn test_send_frame_large_payload() {
        let (_s_r, s_w) = tokio::io::duplex(65536);
        let mut writer = BufWriter::new(s_w);

        // 测试 126 模式 (长度 > 125)
        let medium_payload = vec![0u8; 200];
        WebSocket::send_frame(&mut writer, 0x2, &medium_payload)
            .await
            .unwrap();

        // 测试 127 模式 (长度 > 65535)
        // 实际上可以用较小的模拟，只要逻辑走到那
    }

    #[tokio::test]
    async fn test_send_text_frame() {
        let (client, server) = tokio::io::duplex(1024);
        let mut client_reader = client;
        let (_, server_write) = tokio::io::split(server);
        let mut writer = BufWriter::new(server_write);

        let msg = "hello_world";
        // 调用发送文本方法
        WebSocket::send_text(&mut writer, msg).await.unwrap();

        // 客户端验证
        let mut header = [0u8; 2];
        client_reader.read_exact(&mut header).await.unwrap();

        // 验证 FIN(0x80) + Opcode(0x01 = Text)
        assert_eq!(header[0], 0x81);
        // 验证长度（server 发送给 client 通常不带 mask，所以 mask 位应为 0）
        assert_eq!(header[1], msg.len() as u8);

        let mut payload = vec![0u8; msg.len()];
        client_reader.read_exact(&mut payload).await.unwrap();
        assert_eq!(payload, msg.as_bytes());
    }

    #[tokio::test]
    async fn test_send_binary_frame() {
        let (client, server) = tokio::io::duplex(1024);
        let mut client_reader = client;
        let (_, server_write) = tokio::io::split(server);
        let mut writer = BufWriter::new(server_write);

        let data = vec![0xDE, 0xAD, 0xBE, 0xEF];
        // 调用发送二进制方法
        WebSocket::send_binary(&mut writer, &data).await.unwrap();

        let mut header = [0u8; 2];
        client_reader.read_exact(&mut header).await.unwrap();

        // 验证 FIN(0x80) + Opcode(0x02 = Binary)
        assert_eq!(header[0], 0x82);
        assert_eq!(header[1], data.len() as u8);

        let mut payload = vec![0u8; data.len()];
        client_reader.read_exact(&mut payload).await.unwrap();
        assert_eq!(payload, data);
    }

    #[tokio::test]
    async fn test_send_ping_frame() {
        let (client, server) = tokio::io::duplex(1024);
        let mut client_reader = client;
        let (_, server_write) = tokio::io::split(server);
        let mut writer = BufWriter::new(server_write);

        // 调用发送 Ping 方法
        WebSocket::send_ping(&mut writer).await.unwrap();

        let mut header = [0u8; 2];
        client_reader.read_exact(&mut header).await.unwrap();

        // 验证 FIN(0x80) + Opcode(0x09 = Ping)
        assert_eq!(header[0], 0x89);
        // Ping 通常不带 payload，长度应为 0
        assert_eq!(header[1], 0);
    }

    #[tokio::test]
    async fn test_send_large_text_frame_126() {
        let (client, server) = tokio::io::duplex(2048);
        let mut client_reader = client;
        let (_, server_write) = tokio::io::split(server);
        let mut writer = BufWriter::new(server_write);

        // 构造一个长度为 200 的字符串 (超过 125，应使用 126 模式)
        let large_msg = "a".repeat(200);
        WebSocket::send_text(&mut writer, &large_msg).await.unwrap();

        let mut header = [0u8; 4]; // 2字节头 + 2字节扩展长度
        client_reader.read_exact(&mut header).await.unwrap();

        assert_eq!(header[0], 0x81);
        assert_eq!(header[1], 126); // 长度标识位为 126

        let extended_len = u16::from_be_bytes([header[2], header[3]]);
        assert_eq!(extended_len, 200);

        let mut payload = vec![0u8; 200];
        client_reader.read_exact(&mut payload).await.unwrap();
        assert_eq!(payload, large_msg.as_bytes());
    }

    #[tokio::test]
    async fn test_websocket_full_integration_via_router() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpStream;

        // --- 1. 服务器配置 ---
        let addr: std::net::SocketAddr = "127.0.0.1:0".parse().unwrap();
        let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
        let actual_addr = listener.local_addr().unwrap();
        drop(listener); // 释放 listener 以便 HTTPServer 重新绑定

        let mut hr = Router::new(NodeType::Static("root".into()));

        // --- 2. 构造 WebSocket 业务逻辑 ---
        let ws_logic = WebSocket {
            on_text: Some(Arc::new(|_ws, ctx, text| {
                Box::pin(async move {
                    let mut writer = ctx.writer.lock().await;
                    // 收到消息转为大写并回传
                    WebSocket::send_text(&mut writer, &format!("ACK: {}", text))
                        .await
                        .unwrap();
                    true
                })
            })),
            on_binary: None,
        };

        // 将 WebSocket 转化为中间件
        let ws_middleware = WebSocket::to_middleware(ws_logic);

        // 定义一个空 Handler（WebSocket 中间件会拦截并返回 false，所以这个 handler 永远不会执行）
        let ws_handler = exe!(|_ctx| { false });

        // 挂载到路由：GET /ws
        route!(hr, get!("/ws", ws_handler, vec![ws_middleware.into()]));

        // --- 3. 启动服务器 ---
        let server = HTTPServer::new(actual_addr).http(hr).clone();
        tokio::spawn(async move {
            let _ = server.start::<RawCodec, RawCodec>(Arc::new(|c: &RawCodec| c.id())).await;
        });

        // 等待服务器启动
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

        // --- 4. 客户端模拟 (真实 TCP 交互) ---
        let mut stream = TcpStream::connect(actual_addr).await.unwrap();

        // 发起 WebSocket 握手请求
        let handshake_req = format!(
            "GET /ws HTTP/1.1\r\n\
        Host: {}\r\n\
        Upgrade: websocket\r\n\
        Connection: Upgrade\r\n\
        Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\
        Sec-WebSocket-Version: 13\r\n\r\n",
            actual_addr
        );
        stream.write_all(handshake_req.as_bytes()).await.unwrap();

        // 验证握手响应
        let mut response = [0u8; 1024];
        let n = stream.read(&mut response).await.unwrap();
        let resp_str = String::from_utf8_lossy(&response[..n]);
        assert!(resp_str.contains("101 Switching Protocols"));
        assert!(resp_str.contains("Sec-WebSocket-Accept: s3pPLMBiTxaQ9kYGzzhZRbK+xOo="));

        // --- 5. 数据交换测试 ---
        // 客户端发送 Mask 过的文本帧: "hello"
        // 使用你之前定义的 create_ws_frame
        let frame = create_ws_frame(0x1, b"hello", true);
        stream.write_all(&frame).await.unwrap();

        // 接收服务端回传：预期 "ACK: hello"
        let mut head = [0u8; 2];
        stream.read_exact(&mut head).await.unwrap();
        assert_eq!(head[0], 0x81); // FIN + Text

        let payload_len = (head[1] & 0x7f) as usize;
        let mut payload = vec![0u8; payload_len];
        stream.read_exact(&mut payload).await.unwrap();

        assert_eq!(String::from_utf8(payload).unwrap(), "ACK: hello");

        println!("WebSocket 全链路集成测试通过！");
    }

    #[tokio::test]
    async fn test_websocket_middleware_return_logic() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpStream;

        // --- 1. 服务器配置 ---
        let addr: std::net::SocketAddr = "127.0.0.1:0".parse().unwrap();
        let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
        let actual_addr = listener.local_addr().unwrap();
        drop(listener);

        let mut hr = Router::new(NodeType::Static("root".into()));

        // WebSocket 中间件：收到 text 就关闭
        let ws_middleware = WebSocket::to_middleware(WebSocket {
            on_text: Some(Arc::new(|_, _, _| Box::pin(async { false }))),
            on_binary: None,
        });

        // 哨兵 Handler：如果中间件返回 true，就会执行到这里
        let sentinel_handler = exe!(|ctx| {
            let mut meta = ctx.local.get_value::<HttpMetadata>().unwrap();
            meta.body = b"Sentinel Hit".to_vec(); // 标记 Handler 被触发
            ctx.local.set_value(meta);
            true
        });

        // 挂载：GET /ws -> [ws_middleware] -> sentinel_handler
        route!(
            hr,
            get!("/ws", sentinel_handler, vec![ws_middleware.into()])
        );

        let server = HTTPServer::new(actual_addr).http(hr).clone();
        tokio::spawn(async move {
            let _ = server.start::<RawCodec, RawCodec>(Arc::new(|c: &RawCodec| c.id())).await;
        });
        tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;

        // --- 场景 A: 升级请求 (应该被拦截，返回 false) ---
        {
            let mut stream = TcpStream::connect(actual_addr).await.unwrap();
            let upgrade_req = format!(
                "GET /ws HTTP/1.1\r\n\
            Host: {}\r\n\
            Upgrade: websocket\r\n\
            Connection: Upgrade\r\n\
            Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n",
                actual_addr
            );
            stream.write_all(upgrade_req.as_bytes()).await.unwrap();

            let mut buf = [0u8; 1024];
            let n = stream.read(&mut buf).await.unwrap();
            let resp = String::from_utf8_lossy(&buf[..n]);

            // 1. 验证握手成功
            assert!(resp.contains("101 Switching Protocols"));
            // 2. 验证哨兵逻辑没有执行 (不应该包含 "Sentinel Hit")
            assert!(
                !resp.contains("Sentinel Hit"),
                "Middleware should have intercepted the request"
            );
        }

        // --- 场景 B: 普通请求 (应该穿透，返回 true) ---
        {
            let mut stream = TcpStream::connect(actual_addr).await.unwrap();
            let normal_req = format!(
                "GET /ws HTTP/1.1\r\n\
            Host: {}\r\n\r\n",
                actual_addr
            );
            stream.write_all(normal_req.as_bytes()).await.unwrap();

            let mut buf = [0u8; 1024];
            let n = stream.read(&mut buf).await.unwrap();
            let resp = String::from_utf8_lossy(&buf[..n]);

            // 1. 验证没有执行握手 (应该是 200 OK 或由 Handler 产生的响应)
            assert!(!resp.contains("101 Switching Protocols"));
            // 2. 验证穿透到了哨兵 Handler
            assert!(
                resp.contains("Sentinel Hit"),
                "Middleware should have allowed the request to pass through"
            );
        }
    }

    #[tokio::test]
    async fn test_websocket_middleware_non_get_method() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpStream;

        // --- 1. 服务器配置 ---
        let addr: std::net::SocketAddr = "127.0.0.1:0".parse().unwrap();
        let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
        let actual_addr = listener.local_addr().unwrap();
        drop(listener);

        let mut hr = Router::new(NodeType::Static("root".into()));

        // WebSocket 中间件
        let ws_middleware = WebSocket::to_middleware(WebSocket {
            on_text: None,
            on_binary: None,
        });

        // 哨兵 Handler
        let sentinel_handler = exe!(|ctx| {
            let mut meta = ctx.local.get_value::<HttpMetadata>().unwrap();
            meta.body = b"Post Handler Hit".to_vec();
            ctx.local.set_value(meta);
            true
        });

        // 挂载一个支持 POST 的路由
        // 注意：即使这里有 WebSocket 中间件，非 GET 请求也应该穿透
        route!(
            hr,
            post!("/ws", sentinel_handler, vec![ws_middleware.into()])
        );

        let server = HTTPServer::new(actual_addr).http(hr).clone();
        tokio::spawn(async move {
            let _ = server.start::<RawCodec, RawCodec>(Arc::new(|c: &RawCodec| c.id())).await;
        });
        tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;

        // --- 场景: 使用 POST 发起升级请求 ---
        {
            let mut stream = TcpStream::connect(actual_addr).await.unwrap();
            // 构造一个合法的握手头，但方法使用 POST
            let post_upgrade_req = format!(
                "POST /ws HTTP/1.1\r\n\
            Host: {}\r\n\
            Upgrade: websocket\r\n\
            Connection: Upgrade\r\n\
            Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n",
                actual_addr
            );
            stream.write_all(post_upgrade_req.as_bytes()).await.unwrap();

            let mut buf = [0u8; 1024];
            let n = stream.read(&mut buf).await.unwrap();
            let resp = String::from_utf8_lossy(&buf[..n]);

            // 1. 验证没有执行握手 (不应包含 101 Switching Protocols)
            assert!(
                !resp.contains("101 Switching Protocols"),
                "POST method should not trigger WebSocket handshake"
            );

            // 2. 验证穿透到了 POST Handler
            assert!(
                resp.contains("Post Handler Hit"),
                "Request should have passed through to the next handler"
            );
        }
    }

    #[tokio::test]
    async fn test_websocket_handshake_failed_missing_key() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpStream;

        // --- 1. 服务器配置 ---
        let addr: std::net::SocketAddr = "127.0.0.1:0".parse().unwrap();
        let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
        let actual_addr = listener.local_addr().unwrap();
        drop(listener);

        let mut hr = Router::new(NodeType::Static("root".into()));

        let ws_middleware = WebSocket::to_middleware(WebSocket {
            on_text: None,
            on_binary: None,
        });

        let sentinel_handler = exe!(|ctx| {
            let mut meta = ctx.local.get_value::<HttpMetadata>().unwrap();
            meta.body = b"Should Not Reach Here".to_vec();
            ctx.local.set_value(meta);
            true
        });

        // 挂载路由
        route!(
            hr,
            get!("/ws", sentinel_handler, vec![ws_middleware.into()])
        );

        let server = HTTPServer::new(actual_addr).http(hr).clone();
        tokio::spawn(async move {
            let _ = server.start::<RawCodec, RawCodec>(Arc::new(|c: &RawCodec| c.id())).await;
        });
        tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;

        // --- 2. 场景: 发送 Upgrade 请求但故意缺失 Sec-WebSocket-Key ---
        {
            let mut stream = TcpStream::connect(actual_addr).await.unwrap();
            let bad_req = format!(
                "GET /ws HTTP/1.1\r\n\
            Host: {}\r\n\
            Upgrade: websocket\r\n\
            Connection: Upgrade\r\n\r\n", // ❌ 缺少 Sec-WebSocket-Key
                actual_addr
            );
            stream.write_all(bad_req.as_bytes()).await.unwrap();

            // 3. 验证结果
            // 因为 handshake 失败，中间件会 println! 并返回 false
            // 客户端通常会收到一个空响应或者连接被关闭（取决于 AexServer 对中间件返回 false 且未写响应的处理）
            let mut buf = [0u8; 1024];
            let n = stream.read(&mut buf).await.unwrap();

            let resp = String::from_utf8_lossy(&buf[..n]);

            // 验证没有成功握手
            assert!(!resp.contains("101 Switching Protocols"));
            // 验证没有穿透到 Handler
            assert!(!resp.contains("Should Not Reach Here"));
        }
    }
    #[tokio::test]
    async fn test_websocket_run_error_path_execution() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::{TcpListener, TcpStream};

        // 1. 启动服务器
        let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let listener = TcpListener::bind(addr).await.unwrap();
        let actual_addr = listener.local_addr().unwrap();
        drop(listener);

        let mut hr = Router::new(NodeType::Static("root".into()));
        let ws_mw = WebSocket::to_middleware(WebSocket {
            on_text: None,
            on_binary: None,
        });

        // 修正后的语法：使用 ctx 闭包参数，并对中间件调用 .into()
        route!(
            hr,
            get!("/trigger", exe!(|_ctx| { true }), vec![ws_mw.into()])
        );

        let server = HTTPServer::new(actual_addr).http(hr).clone();
        tokio::spawn(async move {
            let _ = server.start::<RawCodec, RawCodec>(Arc::new(|c: &RawCodec| c.id())).await;
        });
        tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;

        // 2. 客户端连接并完成握手
        let mut stream = TcpStream::connect(actual_addr).await.unwrap();
        let handshake = format!(
            "GET /trigger HTTP/1.1\r\n\
        Upgrade: websocket\r\n\
        Connection: Upgrade\r\n\
        Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n"
        );
        stream.write_all(handshake.as_bytes()).await.unwrap();

        let mut buf = [0u8; 1024];
        let _ = stream.read(&mut buf).await.unwrap(); // 消耗握手响应

        // 3. 构造触发异常：发送一个“未掩码”的 Text 帧
        // 根据 RFC 6455，客户端发给服务端的帧必须带 Mask，否则服务端必须断开连接
        // [0x81 (FIN+Text), 0x05 (PayloadLen 5, Mask bit 为 0)]
        let illegal_frame = vec![0x81, 0x05, b'h', b'e', b'l', b'l', b'o'];
        stream.write_all(&illegal_frame).await.unwrap();

        // 4. 验证副作用确认路径执行
        // 副作用 A: read_full 抛出 Err 前会调用 Self::close，客户端应收到 Close 控制帧 (0x88)
        let n1 = stream.read(&mut buf).await.unwrap();
        assert!(n1 > 0, "服务端应返回 Close 帧数据");
        assert_eq!(
            buf[0], 0x88,
            "必须收到 Close 帧 (Opcode 8)，证明 read_full 识别了协议错误并准备报错"
        );

        // 副作用 B: run 循环接收到错误并退出，命中 eprintln! 分支，随后 ctx 销毁导致 TCP 关闭
        let n2 = stream.read(&mut buf).await.unwrap();
        assert_eq!(n2, 0, "连接必须在错误发生后彻底关闭");
    }

    #[tokio::test]
    async fn test_read_full_logic_branches() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::{TcpListener, TcpStream};

        // --- 1. 服务器启动 ---
        let addr: std::net::SocketAddr = "127.0.0.1:0".parse().unwrap();
        let listener = TcpListener::bind(addr).await.unwrap();
        let actual_addr = listener.local_addr().unwrap();
        drop(listener);

        let mut hr = Router::new(NodeType::Static("root".into()));

        // 我们让 WebSocket 收到 Text 时回传 "ok"，用于验证逻辑通畅
        let ws = WebSocket {
            on_text: Some(Arc::new(|_, ctx, _| {
                Box::pin(async move {
                    let mut w = ctx.writer.lock().await;
                    let _ = WebSocket::send_text(&mut w, "ok").await;
                    true
                })
            })),
            on_binary: None,
        };
        let ws_mw = WebSocket::to_middleware(ws);
        route!(
            hr,
            get!("/read_test", exe!(|_ctx| { true }), vec![ws_mw.into()])
        );

        let server = HTTPServer::new(actual_addr).http(hr).clone();
        tokio::spawn(async move {
            let _ = server.start::<RawCodec, RawCodec>(Arc::new(|c: &RawCodec| c.id())).await;
        });
        tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;

        // --- 2. 测试函数：用于快速发送帧并接收 Close 帧状态码 ---
        async fn expect_close_code(
            addr: std::net::SocketAddr,
            raw_frame: Vec<u8>,
            expected_code: u16,
        ) {
            let mut stream = TcpStream::connect(addr).await.unwrap();
            // 握手
            stream.write_all(b"GET /read_test HTTP/1.1\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n").await.unwrap();
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf).await.unwrap();

            // 发送测试帧
            stream.write_all(&raw_frame).await.unwrap();

            // 读取 Close 帧
            let n = stream.read(&mut buf).await.unwrap();
            if n >= 4 && buf[0] == 0x88 {
                let code = u16::from_be_bytes([buf[2], buf[3]]);
                assert_eq!(code, expected_code);
            } else {
                panic!(
                    "Expected Close frame with code {}, but got {:?}",
                    expected_code,
                    &buf[..n]
                );
            }
        }

        // --- 3. 开始测试各个分支 ---

        // A. 测试 !fin (分片不支持) -> 预期 1002
        // 0x01 (Text but FIN=0)
        expect_close_code(actual_addr, vec![0x01, 0x80, 0, 0, 0, 0], 1002).await;

        // B. 测试 !masked (协议错误) -> 预期 1002
        // 0x81 (FIN Text), 0x05 (Payload 5, Mask=0)
        expect_close_code(actual_addr, vec![0x81, 0x05, 1, 2, 3, 4, 5], 1002).await;

        // C. 测试控制帧长度 > 125 -> 预期 1002
        // 0x89 (Ping), 0xfe (Mask=1, Len=126)
        expect_close_code(actual_addr, vec![0x89, 0xfe, 0, 126, 0, 0, 0, 0], 1002).await;

        // D. 测试未知 Opcode -> 预期 1002
        // 0x83 (Opcode 3 is Reserved)
        expect_close_code(actual_addr, vec![0x83, 0x80, 0, 0, 0, 0], 1002).await;

        // E. 测试 Ping 自动响应 Pong (match 0x9)
        {
            let mut stream = TcpStream::connect(actual_addr).await.unwrap();
            stream.write_all(b"GET /read_test HTTP/1.1\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n").await.unwrap();
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf).await.unwrap();

            // 发送 Ping: 0x89, Mask=1, Len=0, MaskKey=[1,1,1,1]
            stream.write_all(&[0x89, 0x80, 1, 1, 1, 1]).await.unwrap();

            // 服务端应自动回传 Pong: 0x8a, Len=0 (不带 Mask)
            stream.read_exact(&mut buf[..2]).await.unwrap();
            assert_eq!(buf[0], 0x8a);
            assert_eq!(buf[1], 0x00);
        }
    }

    #[tokio::test]
    async fn test_read_full_advanced_logic() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::{TcpListener, TcpStream};

        // --- 1. 服务器启动 ---
        let addr: std::net::SocketAddr = "127.0.0.1:0".parse().unwrap();
        let listener = TcpListener::bind(addr).await.unwrap();
        let actual_addr = listener.local_addr().unwrap();
        drop(listener);

        let mut hr = Router::new(NodeType::Static("root".into()));
        let ws_mw = WebSocket::to_middleware(WebSocket {
            on_text: None,
            on_binary: None,
        });
        route!(
            hr,
            get!("/protocol", exe!(|_ctx| { true }), vec![ws_mw.into()])
        );

        let server = HTTPServer::new(actual_addr).http(hr).clone();
        tokio::spawn(async move {
            let _ = server.start::<RawCodec, RawCodec>(Arc::new(|c: &RawCodec| c.id())).await;
        });
        tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;

        // --- 2. 测试场景 A: 扩展长度位 (len == 126) ---
        // 验证 read_full 能正确读取 2 字节的扩展长度
        {
            let mut stream = TcpStream::connect(actual_addr).await.unwrap();
            stream.write_all(b"GET /protocol HTTP/1.1\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n").await.unwrap();
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf).await.unwrap();

            let payload_size: u16 = 200;
            let mut frame = vec![0x81, 0xFE]; // FIN+Text, Mask=1, Len=126
            frame.extend_from_slice(&payload_size.to_be_bytes()); // 扩展长度: 200
            frame.extend_from_slice(&[0, 0, 0, 0]); // Mask Key
            frame.extend_from_slice(&vec![b'a'; 200]); // 200字节负载

            stream.write_all(&frame).await.unwrap();
            // 如果逻辑正确，服务端会处理这 200 字节。由于我们没设 handler，它会继续 loop。
            // 这里我们可以发一个关闭帧来结束这次连接验证。
            stream.write_all(&[0x88, 0x80, 0, 0, 0, 0]).await.unwrap();
        }

        // --- 3. 测试场景 B: 收到关闭帧 (Opcode 0x8) ---
        // 验证逻辑：客户端发 1000 Close -> 服务端回 1000 Close -> run 退出
        {
            let mut stream = TcpStream::connect(actual_addr).await.unwrap();
            stream.write_all(b"GET /protocol HTTP/1.1\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n").await.unwrap();
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf).await.unwrap();

            // 发送关闭帧：Code 1000
            let mut close_payload = 1000u16.to_be_bytes().to_vec();
            let mut frame = vec![0x88, 0x82, 1, 2, 3, 4]; // Opcode 8, Mask=1, Len=2
            for i in 0..2 {
                close_payload[i] ^= [1, 2, 3, 4][i];
            }
            frame.extend_from_slice(&close_payload);

            stream.write_all(&frame).await.unwrap();

            // 验证服务端回传了关闭帧
            stream.read_exact(&mut buf[..4]).await.unwrap();
            assert_eq!(buf[0], 0x88);
            assert_eq!(u16::from_be_bytes([buf[2], buf[3]]), 1000);
        }

        // --- 4. 测试场景 C: 非法关闭码 (parse_close_payload 异常) ---
        // 验证逻辑：发送 0 字节 payload 或 非法 Code -> 预期 1002 错误
        {
            let mut stream = TcpStream::connect(actual_addr).await.unwrap();
            stream.write_all(b"GET /protocol HTTP/1.1\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n").await.unwrap();
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf).await.unwrap();

            // 发送 Code 1005 (RFC 规定不能在关闭帧里显式发送 1005)
            let close_payload = 1005u16.to_be_bytes().to_vec();
            let mut frame = vec![0x88, 0x82, 0, 0, 0, 0];
            frame.extend_from_slice(&close_payload);

            stream.write_all(&frame).await.unwrap();

            // 预期收到 1002 (Protocol Error)
            stream.read_exact(&mut buf[..4]).await.unwrap();
            assert_eq!(u16::from_be_bytes([buf[2], buf[3]]), 1002);
        }
    }

    #[tokio::test]
    async fn test_read_full_len127_extended_payload() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::{TcpListener, TcpStream};

        // 1. 启动服务器逻辑 (复用之前的配置)
        let addr: std::net::SocketAddr = "127.0.0.1:0".parse().unwrap();
        let listener = TcpListener::bind(addr).await.unwrap();
        let actual_addr = listener.local_addr().unwrap();
        drop(listener);

        let mut hr = Router::new(NodeType::Static("root".into()));

        // 设置一个 Handler 验证接收到的超长数据内容
        let ws = WebSocket {
            on_text: Some(Arc::new(|_, ctx, text| {
                Box::pin(async move {
                    if text.len() == 200 {
                        let mut w = ctx.writer.lock().await;
                        let _ = WebSocket::send_text(&mut w, "len_ok").await;
                    }
                    true
                })
            })),
            on_binary: None,
        };

        let ws_mw = WebSocket::to_middleware(ws);
        route!(
            hr,
            get!("/len127", exe!(|_ctx| { true }), vec![ws_mw.into()])
        );

        let server = HTTPServer::new(actual_addr).http(hr).clone();
        tokio::spawn(async move {
            let _ = server.start::<RawCodec, RawCodec>(Arc::new(|c: &RawCodec| c.id())).await;
        });
        tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;

        // 2. 客户端构造 64-bit Length 帧
        let mut stream = TcpStream::connect(actual_addr).await.unwrap();

        // 握手
        stream.write_all(b"GET /len127 HTTP/1.1\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n").await.unwrap();
        let mut buf = [0u8; 1024];
        let _ = stream.read(&mut buf).await.unwrap();

        // 构造帧：虽然数据只有 200 字节，但我们强制使用 8 字节长度格式 (0x7F)
        let payload_len: u64 = 200;
        let mut frame = Vec::new();
        frame.push(0x81); // FIN + Text
        frame.push(0xFF); // Mask=1, Payload Len=127 (指示 64-bit 扩展长度)

        // 写入 8 字节长度 (Big Endian)
        frame.extend_from_slice(&payload_len.to_be_bytes());

        // 写入 4 字节 Mask Key
        let mask = [0x12, 0x34, 0x56, 0x78];
        frame.extend_from_slice(&mask);

        // 写入 200 字节并进行掩码处理
        let raw_data = vec![b'a'; 200];
        let masked_data: Vec<u8> = raw_data
            .iter()
            .enumerate()
            .map(|(i, &b)| b ^ mask[i % 4])
            .collect();
        frame.extend_from_slice(&masked_data);

        // 发送
        stream.write_all(&frame).await.unwrap();

        // 3. 验证服务端响应
        // 如果 read_full 正确解析了 8 字节长度并读取了 payload，handler 会回传 "len_ok"
        let n = stream.read(&mut buf).await.unwrap();
        let resp = String::from_utf8_lossy(&buf[..n]);
        assert!(
            resp.contains("len_ok"),
            "服务端应正确识别 64-bit 长度声明的 200 字节数据"
        );
    }

    #[tokio::test]
    async fn test_read_full_opcode_pong_logic_final() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::{TcpListener, TcpStream};

        let addr: std::net::SocketAddr = "127.0.0.1:0".parse().unwrap();
        let listener = TcpListener::bind(addr).await.unwrap();
        let actual_addr = listener.local_addr().unwrap();
        drop(listener);

        let mut hr = Router::new(NodeType::Static("root".into()));
        let ws = WebSocket {
            on_text: Some(Arc::new(|_, ctx, _text| {
                Box::pin(async move {
                    let mut w = ctx.writer.lock().await;
                    let _ = WebSocket::send_text(&mut w, "ack").await;
                    true
                })
            })),
            on_binary: None,
        };

        let ws_mw = WebSocket::to_middleware(ws);
        // 换一个不带 pong 字样的路径，彻底消除干扰
        route!(hr, get!("/t", exe!(|_ctx| { true }), vec![ws_mw.into()]));

        let server = HTTPServer::new(actual_addr).http(hr).clone();
        tokio::spawn(async move {
            let _ = server.start::<RawCodec, RawCodec>(Arc::new(|c: &RawCodec| c.id())).await;
        });
        tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;

        let mut stream = TcpStream::connect(actual_addr).await.unwrap();

        // 1. 握手
        stream.write_all(b"GET /t HTTP/1.1\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n").await.unwrap();
        let mut buf = [0u8; 1024];
        let n = stream.read(&mut buf).await.unwrap();
        assert!(String::from_utf8_lossy(&buf[..n]).contains("101"));

        // 2. 发送 Pong 帧 (0x8A)
        // 即使服务器逻辑写错回发了数据，它也会先排在缓冲区里
        stream.write_all(&[0x8A, 0x80, 0, 0, 0, 0]).await.unwrap();

        // 3. 紧接着发送一个 Text 帧
        stream
            .write_all(&[0x81, 0x83, 0, 0, 0, 0, b'y', b'e', b's'])
            .await
            .unwrap();

        // 4. 读取结果
        let _n = stream.read(&mut buf).await.unwrap();

        // 关键点：如果服务器正确静默处理了 Pong，那么我们读到的第一个字节必须是 Text 响应 (0x81)
        // 如果服务器错误地回发了 Pong，那么读到的第一个字节会是 0x8a
        assert_eq!(
            buf[0], 0x81,
            "第一个字节应该是 Text 响应 (0x81)，而不是 Pong 响应 (0x8a)"
        );

        // 验证内容确实是 "ack" (Text 响应的内容)
        let payload_len = (buf[1] & 0x7f) as usize;
        assert_eq!(&buf[2..2 + payload_len], b"ack");
    }

    #[tokio::test]
    async fn test_send_frame_push127_large_payload_fixed() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::{TcpListener, TcpStream};

        let addr: std::net::SocketAddr = "127.0.0.1:0".parse().unwrap();
        let listener = TcpListener::bind(addr).await.unwrap();
        let actual_addr = listener.local_addr().unwrap();
        drop(listener);

        let mut hr = Router::new(NodeType::Static("root".into()));
        let ws = WebSocket {
            on_text: Some(Arc::new(|_, ctx, text| {
                Box::pin(async move {
                    if text == "send_large" {
                        let large_data = vec![b'A'; 65537];
                        let mut w = ctx.writer.lock().await;
                        let _ = WebSocket::send_binary(&mut w, &large_data).await;
                    }
                    true
                })
            })),
            on_binary: None,
        };

        let ws_mw = WebSocket::to_middleware(ws);
        route!(
            hr,
            get!("/large_send", exe!(|_ctx| { true }), vec![ws_mw.into()])
        );

        let server = HTTPServer::new(actual_addr).http(hr).clone();
        tokio::spawn(async move {
            let _ = server.start::<RawCodec, RawCodec>(Arc::new(|c: &RawCodec| c.id())).await;
        });
        tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;

        let mut stream = TcpStream::connect(actual_addr).await.unwrap();

        // 1. 握手
        stream.write_all(b"GET /large_send HTTP/1.1\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n").await.unwrap();
        let mut buf = [0u8; 4096];
        let _ = stream.read(&mut buf).await.unwrap();

        // 2. 发送指令 (必须带 Mask！)
        // [0x81 (FIN+Text), 0x8A (Mask=1, Len=10)]
        let mask = [0x11, 0x22, 0x33, 0x44];
        let payload = "send_large";
        let masked_payload: Vec<u8> = payload
            .as_bytes()
            .iter()
            .enumerate()
            .map(|(i, &b)| b ^ mask[i % 4])
            .collect();

        let mut cmd_frame = vec![0x81, 0x8A];
        cmd_frame.extend_from_slice(&mask);
        cmd_frame.extend_from_slice(&masked_payload);
        stream.write_all(&cmd_frame).await.unwrap();

        // 3. 验证服务端发回的帧
        let mut frame_head = [0u8; 10];
        stream.read_exact(&mut frame_head).await.unwrap();

        assert_eq!(frame_head[0], 0x82, "期望收到二进制帧 (0x82)");
        assert_eq!(frame_head[1], 127, "第二字节应为 127");

        let mut len_bytes = [0u8; 8];
        len_bytes.copy_from_slice(&frame_head[2..10]);
        let actual_len = u64::from_be_bytes(len_bytes);

        assert_eq!(actual_len, 65537);
    }

    #[tokio::test]
    async fn test_websocket_all_handler_paths_combined() {
        use std::sync::atomic::AtomicU8;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::{TcpListener, TcpStream};

        // --- 1. 服务器环境准备 ---
        let addr: std::net::SocketAddr = "127.0.0.1:0".parse().unwrap();
        let listener = TcpListener::bind(addr).await.unwrap();
        let actual_addr = listener.local_addr().unwrap();
        drop(listener);

        // 使用计数器来动态控制 Handler 的行为
        let test_stage = Arc::new(AtomicU8::new(0));
        let stage_clone = test_stage.clone();

        let ws = WebSocket {
            on_text: Some(Arc::new(move |_, _, text| {
                let _s = stage_clone.clone();
                Box::pin(async move {
                    if text == "trigger_fail" {
                        return false; // 👈 触发 0x1 分支的 break
                    }
                    true
                })
            })),
            on_binary: Some(Arc::new(move |_, _, data| {
                Box::pin(async move {
                    if data == vec![0xDE, 0xAD] {
                        return false; // 👈 触发 0x2 分支的 break
                    }
                    true
                })
            })),
        };

        let mut hr = Router::new(NodeType::Static("root".into()));
        let ws_mw = WebSocket::to_middleware(ws);
        route!(hr, get!("/ws", exe!(|_ctx| { true }), vec![ws_mw.into()]));

        let server = HTTPServer::new(actual_addr).http(hr).clone();
        tokio::spawn(async move {
            let _ = server.start::<RawCodec, RawCodec>(Arc::new(|c: &RawCodec| c.id())).await;
        });
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // --- 场景 A: 验证 0x1 (Text) 分支的逻辑与 break ---
        {
            let mut stream = TcpStream::connect(actual_addr).await.unwrap();
            // 握手
            stream.write_all(b"GET /ws HTTP/1.1\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n").await.unwrap();
            let mut buf = [0u8; 1024];
            let n = stream.read(&mut buf).await.unwrap();
            assert!(String::from_utf8_lossy(&buf[..n]).contains("101"));

            // 发送 Text 消息 "trigger_fail" (带全零 Mask)
            // 0x81 (FIN+Text), 0x8C (Mask=1, Len=12)
            let mut frame = vec![0x81, 0x8C, 0, 0, 0, 0];
            frame.extend_from_slice(b"trigger_fail");
            stream.write_all(&frame).await.unwrap();

            // 验证: 1. 执行了 from_utf8_lossy 2. 执行了 close 3. 执行了 break
            let _n = stream.read(&mut buf).await.unwrap();
            assert_eq!(buf[0], 0x88, "0x1 拒绝应返回 Close 帧");
            assert_eq!(u16::from_be_bytes([buf[2], buf[3]]), 1000);

            let n_end = stream.read(&mut buf).await.unwrap();
            assert_eq!(n_end, 0, "0x1 拒绝后 TCP 应关闭");
        }

        // --- 场景 B: 验证 0x2 (Binary) 分支的逻辑与 break ---
        {
            let mut stream = TcpStream::connect(actual_addr).await.unwrap();
            // 握手
            stream.write_all(b"GET /ws HTTP/1.1\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n").await.unwrap();
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf).await.unwrap();

            // 发送 Binary 消息 [0xDE, 0xAD]
            // 0x82 (FIN+Binary), 0x82 (Mask=1, Len=2)
            let frame = vec![0x82, 0x82, 0, 0, 0, 0, 0xDE, 0xAD];
            stream.write_all(&frame).await.unwrap();

            // 验证: 1. 调用 on_binary 2. 执行了 close 3. 执行了 break
            let _n = stream.read(&mut buf).await.unwrap();
            assert_eq!(buf[0], 0x88, "0x2 拒绝应返回 Close 帧");
            assert_eq!(u16::from_be_bytes([buf[2], buf[3]]), 1000);

            let n_end = stream.read(&mut buf).await.unwrap();
            assert_eq!(n_end, 0, "0x2 拒绝后 TCP 应关闭");
        }

        // --- 场景 C: 验证未知 Opcode 拦截 (防止进入 run 里的 unreachable) ---
        // 逻辑：read_full 应该先于 run 捕获到非法 Opcode 并返回 1002，而不是让 run 崩溃。
        {
            let mut stream = TcpStream::connect(actual_addr).await.unwrap();
            let _ = stream.write_all(b"GET /ws HTTP/1.1\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n").await.unwrap();
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf).await.unwrap();

            // 发送非法 Opcode 0x3 (Reserved)
            // read_full 会命中 _ => anyhow::bail!("unknown opcode")
            stream.write_all(&[0x83, 0x80, 0, 0, 0, 0]).await.unwrap();

            let _n = stream.read(&mut buf).await.unwrap();
            assert_eq!(buf[0], 0x88, "遇到未知 Opcode 应发送 Close 帧");
            assert_eq!(
                u16::from_be_bytes([buf[2], buf[3]]),
                1002,
                "协议错误码应为 1002"
            );
        }
    }

    #[tokio::test]
async fn test_websocket_custom_close_codes_range() {
    use tokio::net::{TcpListener, TcpStream};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    // 1. 启动服务器
    let addr: std::net::SocketAddr = "127.0.0.1:0".parse().unwrap();
    let listener = TcpListener::bind(addr).await.unwrap();
    let actual_addr = listener.local_addr().unwrap();
    drop(listener);

    let mut hr = Router::new(NodeType::Static("root".into()));
    let ws_mw = WebSocket::to_middleware(WebSocket { on_text: None, on_binary: None });
    route!(hr, get!("/custom_code", exe!(|_ctx| { true }), vec![ws_mw.into()]));

    let server = HTTPServer::new(actual_addr).http(hr).clone();
    tokio::spawn(async move { let _ = server.start::<RawCodec, RawCodec>(Arc::new(|c: &RawCodec| c.id())).await; });
    tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;

    // 2. 客户端连接并握手
    let mut stream = TcpStream::connect(actual_addr).await.unwrap();
    stream.write_all(b"GET /custom_code HTTP/1.1\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n").await.unwrap();
    let mut buf = [0u8; 1024];
    let _ = stream.read(&mut buf).await.unwrap();

    // --- 测试目标：自定义状态码 4000 (命中 3000..=4999 分支) ---
    // 构造 Payload: [0x0F, 0xA0] (即 4000 的大端序)
    let custom_code: u16 = 4000;
    let code_bytes = custom_code.to_be_bytes();
    let mask = [0x11, 0x22, 0x33, 0x44];
    
    let masked_payload = [
        code_bytes[0] ^ mask[0],
        code_bytes[1] ^ mask[1],
    ];

    // 发送关闭帧: FIN+Close(0x88), Masked(0x82), MaskKey, Payload
    let mut frame = vec![0x88, 0x82];
    frame.extend_from_slice(&mask);
    frame.extend_from_slice(&masked_payload);
    
    stream.write_all(&frame).await.unwrap();

    // 3. 验证服务端响应
    // 源码逻辑：parse_close_payload 成功返回 Ok((4000, None))
    // 随后 run 会调用 Self::close(writer, 4000, None) 并 bail
    let _n = stream.read(&mut buf).await.unwrap();
    
    assert_eq!(buf[0], 0x88, "应响应关闭帧");
    let received_code = u16::from_be_bytes([buf[2], buf[3]]);
    
    // 如果该分支没测到（即报错了），received_code 会是 1002
    // 如果测试通过，received_code 应该是我们发送的 4000
    assert_eq!(received_code, 4000, "服务端应允许并原样回传 3000-4999 范围内的自定义状态码");
}

#[tokio::test]
async fn test_websocket_strict_protocol_validation() {
    use tokio::net::{TcpListener, TcpStream};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    // 1. 启动服务器
    let addr: std::net::SocketAddr = "127.0.0.1:0".parse().unwrap();
    let listener = TcpListener::bind(addr).await.unwrap();
    let actual_addr = listener.local_addr().unwrap();
    drop(listener);

    let mut hr = Router::new(NodeType::Static("root".into()));
    // 提供一个正常的 Handler 供 UTF-8 测试使用
    let ws = WebSocket {
        on_text: Some(Arc::new(|_, _, _| Box::pin(async move { true }))),
        on_binary: None,
    };
    let ws_mw = WebSocket::to_middleware(ws);
    route!(hr, get!("/strict", exe!(|_ctx| { true }), vec![ws_mw.into()]));

    let server = HTTPServer::new(actual_addr).http(hr).clone();
    tokio::spawn(async move { let _ = server.start::<RawCodec, RawCodec>(Arc::new(|c: &RawCodec| c.id())).await; });
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // 辅助闭包：快速发起连接并完成握手
    let connect_ws = || async {
        let mut stream = TcpStream::connect(actual_addr).await.unwrap();
        stream.write_all(b"GET /strict HTTP/1.1\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n").await.unwrap();
        let mut buf = [0u8; 1024];
        let _ = stream.read(&mut buf).await.unwrap();
        stream
    };

    // --- 分支 1: 测试保留位校验 (RSV1 set) ---
    {
        let mut stream = connect_ws().await;
        // 0xF1 = FIN(1), RSV1(1), RSV2(0), RSV3(0), Opcode(1 - Text)
        // 按照代码应触发 1002
        stream.write_all(&[0xF1, 0x80, 0, 0, 0, 0]).await.unwrap();
        let mut buf = [0u8; 10];
        stream.read_exact(&mut buf[..4]).await.unwrap();
        assert_eq!(u16::from_be_bytes([buf[2], buf[3]]), 1002, "设置RSV位应返回1002");
    }

    // --- 分支 2: 测试分片拦截 (!fin) ---
    {
        let mut stream = connect_ws().await;
        // 0x01 = FIN(0), Opcode(1) -> 分片起始帧
        stream.write_all(&[0x01, 0x80, 0, 0, 0, 0]).await.unwrap();
        let mut buf = [0u8; 10];
        stream.read_exact(&mut buf[..4]).await.unwrap();
        assert_eq!(u16::from_be_bytes([buf[2], buf[3]]), 1002, "不支持分片应返回1002");
    }

    // --- 分支 3: 测试连续帧拦截 (opcode 0) ---
    {
        let mut stream = connect_ws().await;
        // 0x80 = FIN(1), Opcode(0) -> 无起始帧的连续帧
        stream.write_all(&[0x80, 0x80, 0, 0, 0, 0]).await.unwrap();
        let mut buf = [0u8; 10];
        stream.read_exact(&mut buf[..4]).await.unwrap();
        assert_eq!(u16::from_be_bytes([buf[2], buf[3]]), 1002, "孤立的连续帧应返回1002");
    }

    // --- 分支 4: 测试严格 UTF-8 校验 (1007) ---
    {
        let mut stream = connect_ws().await;
        // 构造非法 UTF-8 负载 (0xFF 在 UTF-8 中无效)
        let invalid_utf8 = [0xFF, 0xFE];
        let mask = [0x11, 0x22, 0x33, 0x44];
        let masked_payload = [invalid_utf8[0] ^ mask[0], invalid_utf8[1] ^ mask[1]];
        
        let mut frame = vec![0x81, 0x82]; // FIN, Text, Masked, Len 2
        frame.extend_from_slice(&mask);
        frame.extend_from_slice(&masked_payload);
        
        stream.write_all(&frame).await.unwrap();
        
        let mut buf = [0u8; 10];
        stream.read_exact(&mut buf[..4]).await.unwrap();
        assert_eq!(u16::from_be_bytes([buf[2], buf[3]]), 1007, "非法UTF-8文本帧应返回1007");
    }
}
}
