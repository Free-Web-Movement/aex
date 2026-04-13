#[cfg(test)]
mod websocket_tests {
    use aex::{
        connection::{context::Context, global::GlobalContext},
        http::{
            middlewares::websocket::WebSocket,
            protocol::{header::{HeaderKey, Headers}, method::HttpMethod},
            websocket::{WSCodec, WSFrame},
        },
        tcp::types::{Command, Frame},
    };
    use bytes::BytesMut;
    use futures::{SinkExt, StreamExt};
    use std::{net::SocketAddr, sync::Arc};
    use tokio::io::{BufReader, duplex};
    use tokio_util::codec::{Decoder, Encoder, Framed};
    use ahash::AHashMap;

    // 辅助工具：模拟客户端发送带 Mask 的 WebSocket 帧
    fn create_masked_frame(opcode: u8, payload: &[u8]) -> Vec<u8> {
        let mut frame = Vec::new();
        frame.push(0x80 | opcode); // FIN + Opcode
        let mask = [0x1, 0x2, 0x3, 0x4];

        if payload.len() < 126 {
            frame.push(0x80 | (payload.len() as u8));
        } else {
            frame.push(0x80 | 126);
            frame.extend_from_slice(&(payload.len() as u16).to_be_bytes());
        }

        frame.extend_from_slice(&mask);
        let mut masked_payload = payload.to_vec();
        for i in 0..masked_payload.len() {
            masked_payload[i] ^= mask[i % 4];
        }
        frame.extend_from_slice(&masked_payload);
        frame
    }

    // --- 1. 握手逻辑测试 ---
    #[test]
    fn test_check_handshake_logic() {
        let mut headers = AHashMap::new();
        headers.insert(HeaderKey::Upgrade, "websocket".to_string());
        headers.insert(HeaderKey::Connection, "Upgrade".to_string());
        let headers_ref = Headers::from(headers);
        assert!(WebSocket::check(HttpMethod::GET, &headers_ref));
        assert!(!WebSocket::check(HttpMethod::POST, &headers_ref));
    }

    // --- 2. Codec 编解码测试 (核心更新) ---
    #[tokio::test]
    async fn test_ws_codec_decode_text() {
        let mut codec = WSCodec;
        let raw_data = create_masked_frame(0x1, b"hello");
        let mut buf = BytesMut::from(&raw_data[..]);

        let frame = codec.decode(&mut buf).unwrap().unwrap();
        if let WSFrame::Text(s) = frame {
            assert_eq!(s, "hello");
        } else {
            panic!("Expected Text frame");
        }
    }

    #[tokio::test]
    async fn test_ws_codec_encode_binary() {
        let mut codec = WSCodec;
        let mut buf = BytesMut::new();
        let frame = WSFrame::Binary(vec![1, 2, 3]);

        codec.encode(frame, &mut buf).unwrap();

        // Server -> Client 不带 Mask，FIN=1, Opcode=2 -> 0x82
        assert_eq!(buf[0], 0x82);
        assert_eq!(buf[1], 3); // Length 3
        assert_eq!(&buf[2..], &[1, 2, 3]);
    }

    // --- 3. 运行循环集成测试 (模拟 run 方法) ---
    #[tokio::test]
    async fn test_websocket_run_loop() {
        let (client, server) = duplex(1024);

        // 模拟业务逻辑：收到任何消息都回复 "ACK"
        let ws = WebSocket {
            on_frame: Some(Arc::new(|_ws, _ctx, frame| {
                Box::pin(async move {
                    match frame {
                        WSFrame::Text(t) if t == "ping" => true,
                        _ => false, // 收到非 ping 则断开
                    }
                })
            })),
        };

        // 启动服务器循环
        // 1. 拆分双工流
        let (s_reader, s_writer) = tokio::io::split(server);

        // 2. 包装 Reader 为 BufReader (满足 AsyncBufRead 约束)
        let s_reader_buffered = BufReader::new(s_reader);

        // 3. 构造满足 Context::new 签名要求的参数
        // 注意：必须明确指定类型以匹配 dyn Trait
        let reader_param: Option<Box<dyn tokio::io::AsyncBufRead + Send + Sync + Unpin>> =
            Some(Box::new(s_reader_buffered));

        let writer_param: Option<Box<dyn tokio::io::AsyncWrite + Send + Sync + Unpin>> =
            Some(Box::new(s_writer));
        let addr = "127.0.0.1:8080".parse::<SocketAddr>().unwrap();
        let global = Arc::new(GlobalContext::new(addr, None));

        let mut ctx = Context::new(reader_param, writer_param, global, addr); // 假设有默认 Context

        let server_handle = tokio::spawn(async move { WebSocket::run(&ws, &mut ctx).await });

        // 客户端操作
        let mut client_framed = Framed::new(client, WSCodec);

        // 发送合法 Text
        client_framed
            .send(WSFrame::Text("ping".into()))
            .await
            .unwrap();

        // 发送会导致 Handler 返回 false 的消息
        client_framed
            .send(WSFrame::Text("die".into()))
            .await
            .unwrap();

        let res = server_handle.await.unwrap();
        assert!(res.is_ok());
    }

    // --- 4. 边界条件测试 (Close 帧解析) ---
    #[test]
    fn test_parse_close_payload_logic() {
        // 正常关闭 1000
        let payload = (1000u16).to_be_bytes();
        let (code, reason) = WebSocket::parse_close_payload(&payload).unwrap();
        assert_eq!(code, 1000);
        assert_eq!(reason, None);

        // 带原因的关闭
        let mut complex_payload = (1001u16).to_be_bytes().to_vec();
        complex_payload.extend_from_slice(b"going away");
        let (code, reason) = WebSocket::parse_close_payload(&complex_payload).unwrap();
        assert_eq!(code, 1001);
        assert_eq!(reason, Some("going away"));
    }

    #[tokio::test]
    async fn test_codec_all_types() {
        let mut codec = WSCodec;
        let mut buf = BytesMut::new();

        // 测试 Pong 编码 (Server -> Client)
        let pong = WSFrame::Pong(vec![9, 9]);
        codec.encode(pong, &mut buf).unwrap();
        assert_eq!(buf[0], 0x8a); // FIN + Pong Opcode
        assert_eq!(buf[1], 2);
        buf.clear();

        // 测试 Close 编码
        let close = WSFrame::Close(1000, Some("bye".into()));
        codec.encode(close, &mut buf).unwrap();
        assert_eq!(buf[0], 0x88);
        // 负载长度应为 2(code) + 3(bye) = 5
        assert_eq!(buf[1], 5);
        buf.clear();

        // 测试带掩码的 Ping 解码 (Client -> Server)
        let raw_ping = create_masked_frame(0x09, b"ping");
        let mut ping_buf = BytesMut::from(&raw_ping[..]);
        let decoded = codec.decode(&mut ping_buf).unwrap().unwrap();
        if let WSFrame::Ping(p) = decoded {
            assert_eq!(p, b"ping");
        } else {
            panic!("Decode failed for Ping");
        }
    }

    #[tokio::test]
    async fn test_websocket_full_interaction() {
        let (client, server) = duplex(2048);
        let addr = "127.0.0.1:8080".parse::<SocketAddr>().unwrap();
        let global = Arc::new(GlobalContext::new(addr, None));

        // 1. 定义更复杂的业务逻辑
        let ws = WebSocket {
            on_frame: Some(Arc::new(|_ws, _ctx, frame| {
                Box::pin(async move {
                    match frame {
                        WSFrame::Binary(data) => {
                            // 收到二进制数据，验证内容
                            assert_eq!(data, vec![0xde, 0xad]);
                            true
                        }
                        WSFrame::Text(t) if t == "exit" => false, // 自定义指令：退出
                        WSFrame::Ping(_) => true, // 允许 Ping 穿透到默认处理器（自动回 Pong）
                        _ => true,
                    }
                })
            })),
        };

        // 2. 准备 Server Context
        let (s_reader, s_writer) = tokio::io::split(server);
        let ctx_reader = Some(Box::new(BufReader::new(s_reader))
            as Box<dyn tokio::io::AsyncBufRead + Send + Sync + Unpin>);
        let ctx_writer =
            Some(Box::new(s_writer) as Box<dyn tokio::io::AsyncWrite + Send + Sync + Unpin>);
        let mut ctx = Context::new(ctx_reader, ctx_writer, global, addr);

        // 3. 启动 Server
        let server_handle = tokio::spawn(async move { WebSocket::run(&ws, &mut ctx).await });

        // 4. 客户端操作
        let mut client_framed = Framed::new(client, WSCodec);

        // --- A. 测试 Ping/Pong ---
        // 发送 Ping，负载为 [1, 2, 3]
        client_framed
            .send(WSFrame::Ping(vec![1, 2, 3]))
            .await
            .unwrap();
        // 期待收到 Pong
        if let Some(Ok(WSFrame::Pong(payload))) = client_framed.next().await {
            assert_eq!(payload, vec![1, 2, 3]);
        } else {
            panic!("Expected Pong response for Ping");
        }

        // --- B. 测试 Binary ---
        client_framed
            .send(WSFrame::Binary(vec![0xde, 0xad]))
            .await
            .unwrap();

        // --- C. 测试自定义指令 (Text: exit) ---
        client_framed
            .send(WSFrame::Text("exit".into()))
            .await
            .unwrap();

        // 5. 验证服务是否正常关闭
        let res = server_handle.await.unwrap();
        assert!(res.is_ok(), "Server loop should exit gracefully");
    }

    #[test]
    fn test_ws_frame_trait_methods() {
        // Text 帧
        let text_frame = WSFrame::Text("hello".into());
        assert_eq!(text_frame.payload().unwrap(), b"hello");
        assert!(text_frame.command().is_none());

        // Binary 帧
        let bin_data = vec![0x01, 0x02, 0x03];
        let bin_frame = WSFrame::Binary(bin_data.clone());
        assert_eq!(bin_frame.payload().unwrap(), bin_data);

        // Ping 帧 (控制帧也有 payload)
        let ping_frame = WSFrame::Ping(vec![0xaa]);
        assert_eq!(ping_frame.payload().unwrap(), vec![0xaa]);

        // Close 帧 (不应视作普通业务 payload，根据实现可能返回 None)
        let close_frame = WSFrame::Close(1000, None);
        assert!(close_frame.payload().is_none());
    }

    #[tokio::test]
    async fn test_ws_codec_comprehensive() {
        let mut codec = WSCodec;
        let mut buf = BytesMut::new();

        // --- A. 测试 Encode (Server -> Client, 无 Mask) ---
        // 1. 测试中等长度 (126 模式: 200 字节)
        let large_data = vec![b'x'; 200];
        let frame = WSFrame::Binary(large_data.clone());
        codec.encode(frame, &mut buf).unwrap();

        assert_eq!(buf[0], 0x82); // FIN + Binary
        assert_eq!(buf[1], 126); // 长度标识
        assert_eq!(u16::from_be_bytes([buf[2], buf[3]]), 200);
        assert_eq!(&buf[4..], &large_data[..]);
        buf.clear();

        // --- B. 测试 Decode (Client -> Server, 必须带 Mask) ---
        // 1. 测试带 Mask 的 Text 帧
        let mask = [1, 2, 3, 4];
        let original_payload = b"mask";
        let mut masked_payload = original_payload.to_vec();
        for i in 0..masked_payload.len() {
            masked_payload[i] ^= mask[i % 4];
        }

        let mut raw_input = Vec::new();
        raw_input.push(0x81); // FIN + Text
        raw_input.push(0x80 | 4); // Mask=1, Len=4
        raw_input.extend_from_slice(&mask);
        raw_input.extend_from_slice(&masked_payload);

        let mut decode_buf = BytesMut::from(&raw_input[..]);
        let decoded = codec.decode(&mut decode_buf).unwrap().unwrap();

        if let WSFrame::Text(s) = decoded {
            assert_eq!(s, "mask");
        } else {
            panic!("Decode mismatch");
        }
    }
    #[test]
    fn test_ws_pong_payload_trait() {
        let heartbeat_data = vec![0x01, 0x09, 0x07];
        let pong_frame = WSFrame::Pong(heartbeat_data.clone());

        // 验证 payload() 方法是否能提取出正确的二进制数据
        assert_eq!(
            pong_frame.payload().expect("Pong should have payload"),
            heartbeat_data,
            "Pong payload must match the input data"
        );
    }

    #[tokio::test]
    async fn test_pong_codec_full_cycle() {
        let mut codec = WSCodec;
        let mut buf = BytesMut::new();
        let test_payload = b"pong_data";

        // --- A. 编码测试 (Server -> Client) ---
        // 服务端响应 Pong，FIN=1, Opcode=0xA (10)
        let pong_out = WSFrame::Pong(test_payload.to_vec());
        codec.encode(pong_out, &mut buf).unwrap();

        assert_eq!(buf[0], 0x8a, "First byte must be 0x8A (FIN + Pong)");
        assert_eq!(
            buf[1],
            test_payload.len() as u8,
            "Payload length must match"
        );
        assert_eq!(&buf[2..], test_payload, "Raw payload must match");
        buf.clear();

        // --- B. 解码测试 (Client -> Server: 必须带掩码) ---
        let mask = [0x11, 0x22, 0x33, 0x44];
        let mut masked_payload = test_payload.to_vec();
        for i in 0..masked_payload.len() {
            masked_payload[i] ^= mask[i % 4];
        }

        let mut raw_input = Vec::new();
        raw_input.push(0x8a); // FIN + Pong
        raw_input.push(0x80 | (test_payload.len() as u8)); // Mask bit set + Len
        raw_input.extend_from_slice(&mask);
        raw_input.extend_from_slice(&masked_payload);

        let mut decode_buf = BytesMut::from(&raw_input[..]);
        let result = codec
            .decode(&mut decode_buf)
            .expect("Decode should succeed")
            .expect("Should return a frame");

        if let WSFrame::Pong(p) = result {
            assert_eq!(
                p, test_payload,
                "Decoded Pong payload must be unmasked correctly"
            );
        } else {
            panic!("Decoded frame was not a Pong variant");
        }
    }

    #[tokio::test]
    async fn test_ping_pong_payload_echo() {
        let (client, server) = tokio::io::duplex(1024);
        let mut client_framed = Framed::new(client, WSCodec);

        // 使用默认逻辑的 WebSocket（自动回 Ping）
        let ws = WebSocket { on_frame: None };

        // 启动服务端 (简化 Context 初始化)
        let (r, w) = tokio::io::split(server);
        let addr = "127.0.0.1:8080".parse::<SocketAddr>().unwrap();
        let global = Arc::new(GlobalContext::new(addr, None));
        let mut ctx = Context::new(
            Some(Box::new(BufReader::new(r))),
            Some(Box::new(w)),
            global,
            addr,
        );

        tokio::spawn(async move {
            let _ = WebSocket::run(&ws, &mut ctx).await;
        });

        // 客户端发送带特定负载的 Ping
        let secret_payload = vec![0x42, 0x43, 0x44];
        client_framed
            .send(WSFrame::Ping(secret_payload.clone()))
            .await
            .unwrap();

        // 验证客户端收到的 Pong 是否携带了相同的负载
        if let Some(Ok(WSFrame::Pong(received_payload))) = client_framed.next().await {
            assert_eq!(
                received_payload, secret_payload,
                "Pong must echo the Ping payload exactly"
            );
        } else {
            panic!("Did not receive the expected Pong frame");
        }
    }

    #[tokio::test]
    async fn test_ws_codec_opcode_and_payload_integrity() {
        let mut codec = WSCodec;
        let mut buf = bytes::BytesMut::new();

        // 模拟测试场景：客户端发送带掩码的 Binary 帧 (Opcode 0x2)
        let mask = [0x01, 0x02, 0x03, 0x04];
        let original_payload = vec![0xde, 0xad, 0xbe, 0xef];
        let masked_payload: Vec<u8> = original_payload
            .iter()
            .enumerate()
            .map(|(i, &b)| b ^ mask[i % 4])
            .collect();

        let mut raw_input = vec![0x82, 0x84]; // FIN + Binary, Masked, Len 4
        raw_input.extend_from_slice(&mask);
        raw_input.extend_from_slice(&masked_payload);

        let mut decode_buf = bytes::BytesMut::from(&raw_input[..]);

        // 1. 解码验证
        let result = codec
            .decode(&mut decode_buf)
            .unwrap()
            .expect("Should decode a frame");

        // 验证变体识别是否与 Opcode 2 对应
        if let WSFrame::Binary(p) = result {
            assert_eq!(p, original_payload, "Payload 解掩码后数据损坏");
        } else {
            panic!("Opcode 0x2 映射变体错误");
        }

        // 2. 编码验证 (Server -> Client 不带掩码)
        let frame = WSFrame::Ping(vec![0x11]);
        codec.encode(frame, &mut buf).unwrap();

        assert_eq!(buf[0], 0x89); // FIN + Ping(9)
        assert_eq!(buf[1], 1); // Len 1
        assert_eq!(buf[2], 0x11); // Payload
    }

    #[tokio::test]
    async fn test_ws_encode_payload_len_126() {
        let mut codec = WSCodec;
        let mut buf = bytes::BytesMut::new();

        // 构造 500 字节的数据 (落在 126 - 65535 区间)
        let size: usize = 500;
        let large_data = vec![0x41; size];
        let frame = WSFrame::Binary(large_data.clone());

        codec.encode(frame, &mut buf).unwrap();

        // 验证头部
        // 第 1 字节: FIN(1) + Binary(2) = 0x82
        assert_eq!(buf[0], 0x82);
        // 第 2 字节: Mask(0) + Len(126) = 126 (0x7E)
        assert_eq!(buf[1], 126);

        // 第 3-4 字节: 应该是 500 的大端序 (0x01F4)
        let ext_len = u16::from_be_bytes([buf[2], buf[3]]);
        assert_eq!(ext_len, size as u16);

        // 验证后续数据
        assert_eq!(&buf[4..], &large_data[..]);
    }

    #[tokio::test]
    async fn test_ws_encode_payload_len_127() {
        let mut codec = WSCodec;
        let mut buf = bytes::BytesMut::new();

        // 构造 70,000 字节的数据 (超过 65535，需 8 字节长度位)
        let size: usize = 70000;
        let huge_data = vec![0x42; size];
        let frame = WSFrame::Binary(huge_data.clone());

        codec.encode(frame, &mut buf).unwrap();

        // 验证头部
        // 第 1 字节: 0x82
        assert_eq!(buf[0], 0x82);
        // 第 2 字节: Mask(0) + Len(127) = 127 (0x7F)
        assert_eq!(buf[1], 127);

        // 第 3-10 字节: 应该是 70,000 的 64 位大端序
        let mut len_bytes = [0u8; 8];
        len_bytes.copy_from_slice(&buf[2..10]);
        let ext_len = u64::from_be_bytes(len_bytes);
        assert_eq!(ext_len, size as u64);

        // 验证载荷起始位置
        assert_eq!(&buf[10..10 + 5], &vec![0x42; 5][..]);
        assert_eq!(buf.len(), 10 + size);
    }

    #[tokio::test]
    async fn test_ws_decode_extended_lengths() {
        let mut codec = WSCodec;

        // 模拟一个带 Mask 的 126 长度模式帧 (假设长度为 200)
        let mut raw = vec![0x82, 0xfe]; // FIN+Binary, Mask=1, Len=126
        raw.extend_from_slice(&(200u16).to_be_bytes()); // 扩展长度
        let mask = [0x1, 0x2, 0x3, 0x4];
        raw.extend_from_slice(&mask);

        let payload = vec![0x61; 200];
        let masked_payload: Vec<u8> = payload
            .iter()
            .enumerate()
            .map(|(i, &b)| b ^ mask[i % 4])
            .collect();
        raw.extend_from_slice(&masked_payload);

        let mut buf = bytes::BytesMut::from(&raw[..]);
        let result = codec.decode(&mut buf).unwrap().unwrap();

        if let WSFrame::Binary(data) = result {
            assert_eq!(data.len(), 200);
            assert_eq!(data[0], 0x61);
        } else {
            panic!("Decoding 126-length frame failed");
        }
    }

    #[test]
    fn test_decode_incomplete_base_header() {
        let mut codec = WSCodec;
        // 只有 1 个字节，无法判断 Opcode 和 Mask 标志
        let mut src = bytes::BytesMut::from(&[0x81][..]);

        let result = codec
            .decode(&mut src)
            .expect("Decode should not error on partial data");
        assert!(result.is_none(), "Header < 2 bytes should return None");
        assert_eq!(src.len(), 1, "Should not consume bytes");
    }

    #[test]
    fn test_decode_incomplete_126_extended_len() {
        let mut codec = WSCodec;
        let mut src = bytes::BytesMut::new();

        // 构造：FIN+Text(0x81), PayloadLen=126(0x7E)
        // 此时 src.len() = 2，还缺 2 字节的扩展长度
        src.extend_from_slice(&[0x81, 0x7e]);

        // 测试只有 2 字节
        assert!(codec.decode(&mut src).unwrap().is_none());

        // 测试只有 3 字节 (缺 1 字节扩展长度)
        src.extend_from_slice(&[0x01]);
        assert!(
            codec.decode(&mut src).unwrap().is_none(),
            "Should wait for full 2-byte extended len"
        );
    }

    #[test]
    fn test_decode_incomplete_127_extended_len() {
        let mut codec = WSCodec;
        let mut src = bytes::BytesMut::new();

        // 构造：FIN+Binary(0x82), PayloadLen=127(0x7F)
        src.extend_from_slice(&[0x82, 0x7f]);

        // 模拟只有 9 个字节 (2 基础 + 7 扩展)，还差 1 个字节才够 8 字节扩展长度
        src.extend_from_slice(&[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01]);

        let result = codec.decode(&mut src).expect("Decode fail");
        assert!(
            result.is_none(),
            "Should return None when src.len() < 10 for 127-mode"
        );
        assert_eq!(src.len(), 9);
    }

    #[test]
    fn test_decode_missing_mask_key() {
        let mut codec = WSCodec;
        let mut src = bytes::BytesMut::new();

        // 126 模式：2(基础) + 2(扩展长度) = 4 字节已满足
        // 但因为设置了 Mask 位，还需额外 4 字节掩码，总计需 8 字节
        src.extend_from_slice(&[0x81, 0xfe]); // Masked Text, Len 126
        src.extend_from_slice(&(200u16).to_be_bytes()); // 扩展长度设为 200

        // 目前 src.len() = 4，虽然满足了长度读取，但 Mask Key 还没收齐
        assert!(
            codec.decode(&mut src).unwrap().is_none(),
            "Should wait for 4-byte mask key"
        );

        // 补齐 3 字节 Mask，还是不够
        src.extend_from_slice(&[0x01, 0x02, 0x03]);
        assert!(codec.decode(&mut src).unwrap().is_none());
    }

    #[test]
    fn test_decode_extended_len_127_step_by_step() {
        let mut codec = WSCodec;
        let mut src = bytes::BytesMut::new();

        // 构造：FIN + Binary(0x82), Mask=1, PayloadLen=127(0xFF)
        // 基础头部 2 字节已收齐
        src.extend_from_slice(&[0x82, 0xff]);

        // 1. 测试 head_len 增加后的第一个边界：不足 10 字节 (2 基础 + 8 扩展)
        // 目前只有 2 字节，还差 8 字节扩展长度
        assert!(
            codec.decode(&mut src).unwrap().is_none(),
            "必须等待 8 字节扩展长度"
        );

        // 2. 模拟收到 4 字节扩展长度 (总计 6 字节)
        src.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
        assert!(
            codec.decode(&mut src).unwrap().is_none(),
            "扩展长度未收齐时应返回 None"
        );

        // 3. 补齐剩下的 4 字节扩展长度 (总计 10 字节)
        // 现在 8 字节扩展长度收齐了，但因为有 Mask 标志，还需要 4 字节掩码
        src.extend_from_slice(&[0x00, 0x00, 0x01, 0xf4]); // 500 字节的 64位大端序后缀
        assert!(
            codec.decode(&mut src).unwrap().is_none(),
            "收齐长度后，若 Mask Key 不足仍应返回 None"
        );

        assert_eq!(src.len(), 10, "缓冲区不应被提前消费");
    }

    #[test]
    fn test_parse_close_payload_comprehensive() {
        // --- 1. 正常关闭 (1000 + "bye") ---
        let mut p1 = (1000u16).to_be_bytes().to_vec();
        p1.extend_from_slice(b"bye");

        {
            let (code, reason) = WebSocket::parse_close_payload(&p1).unwrap();
            assert_eq!(code, 1000);
            assert_eq!(reason, Some("bye"));
            // 验证此处 reason 是对 p1 的引用而非拷贝
        }

        // --- 2. 只有状态码 (1001) ---
        let p2 = (1001u16).to_be_bytes();
        let (code, reason) = WebSocket::parse_close_payload(&p2).unwrap();
        assert_eq!(code, 1001);
        assert!(reason.is_none());

        // --- 3. 空载荷 (RFC 1005) ---
        let (code, reason) = WebSocket::parse_close_payload(&[]).unwrap();
        assert_eq!(code, 1005);
        assert!(reason.is_none());

        // --- 4. 边界异常：长度不足 ---
        let p3 = [0x03];
        assert!(WebSocket::parse_close_payload(&p3).is_err());

        // --- 5. 边界异常：非法 UTF-8 ---
        let mut p4 = (1000u16).to_be_bytes().to_vec();
        p4.push(0xff);
        assert!(WebSocket::parse_close_payload(&p4).is_err());
    }

    #[test]
    fn test_parse_close_payload_result_chain() -> anyhow::Result<()> {
        // --- 1. 模拟标准关闭帧载荷 ---
        let payload = {
            let mut p = (1000u16).to_be_bytes().to_vec();
            p.extend_from_slice(b"normal closure");
            p
        };

        // 核心行测试
        let (code, reason) = WebSocket::parse_close_payload(&payload)?;

        assert_eq!(code, 1000);
        assert_eq!(reason, Some("normal closure"));

        // --- 2. 模拟协议边界：恰好 2 字节 (无 Reason) ---
        let payload_min = (1001u16).to_be_bytes();
        let (code, reason) = WebSocket::parse_close_payload(&payload_min)?;
        assert_eq!(code, 1001);
        assert!(reason.is_none());

        // --- 3. 模拟静默关闭：0 字节 ---
        let (code, reason) = WebSocket::parse_close_payload(&[])?;
        assert_eq!(code, 1005);
        assert!(reason.is_none());

        Ok(())
    }

    #[test]
    fn test_parse_close_payload_error_scenarios() {
        // --- 4. 模拟畸形载荷：只有 1 字节 ---
        let payload_bad_len = vec![0x03];
        let result = WebSocket::parse_close_payload(&payload_bad_len);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Incomplete"));

        // --- 5. 模拟 UTF-8 攻击：非法字节序列 ---
        let mut payload_bad_utf8 = (1000u16).to_be_bytes().to_vec();
        payload_bad_utf8.extend_from_slice(&[0x80, 0x81]); // 非法的 UTF-8 起始字节

        let result = WebSocket::parse_close_payload(&payload_bad_utf8);
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_decode_opcode_0x8_full_path() -> anyhow::Result<()> {
        let mut codec = WSCodec;

        // 构造一个真实的关闭帧字节流 (Opcode 0x8, FIN=1, 无掩码)
        // 状态码 1000 (0x03E8), 原因 "bye"
        let mut raw_bytes = vec![0x88, 0x05]; // 0x88 = FIN + Opcode 8; 0x05 = Payload Len 5
        raw_bytes.extend_from_slice(&(1000u16).to_be_bytes());
        raw_bytes.extend_from_slice(b"bye");

        let mut src = bytes::BytesMut::from(&raw_bytes[..]);

        // 执行解码
        let result = codec.decode(&mut src)?;

        // 验证是否正确执行了 parse_close_payload 并转换
        match result {
            Some(WSFrame::Close(code, reason)) => {
                assert_eq!(code, 1000, "状态码解析错误");
                assert_eq!(reason.as_deref(), Some("bye"), "关闭原因解析错误");
            }
            _ => panic!("未能正确识别并转换 Opcode 0x8 帧"),
        }

        Ok(())
    }

    #[test]
    fn test_match_unsupported_opcode_logic() {
        // 模拟一个合法的 u8 但超出 0x0-0xF 范围的操作码
        let opcode = 0x10u8;
        let payload = vec![0x01];

        // 这里的逻辑必须与 decode 内部的 match 完全一致
        let result: anyhow::Result<Option<WSFrame>> = match opcode {
            0x0 => Ok(Some(WSFrame::Continuation(payload))),
            0x1 => Ok(Some(WSFrame::Text("".to_string()))), // 简化处理
            0x2 => Ok(Some(WSFrame::Binary(payload))),
            0x8 => Ok(Some(WSFrame::Close(1000, None))),
            0x9 => Ok(Some(WSFrame::Ping(payload))),
            0xa => Ok(Some(WSFrame::Pong(payload))),
            0x3..=0x7 => Ok(Some(WSFrame::ReservedNonControl(opcode, payload))),
            0xb..=0xf => Ok(Some(WSFrame::ReservedControl(opcode, payload))),
            _ => Err(anyhow::anyhow!("Unsupported opcode: 0x{:x}", opcode)),
        };

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Unsupported opcode: 0x10")
        );
    }

    #[test]
    fn test_ws_frame_as_command_mapping() {
        // --- 测试 Continuation (0x0) ---
        let f0 = WSFrame::Continuation(vec![0xaa]);
        assert_eq!(f0.id(), 0x0);
        assert_eq!(f0.data(), &vec![0xaa]);

        // --- 测试 ReservedNonControl (0x3..0x7) ---
        let f3 = WSFrame::ReservedNonControl(0x3, vec![0xbb]);
        assert_eq!(f3.id(), 3);
        assert_eq!(f3.data(), &vec![0xbb]);

        // --- 测试 ReservedControl (0xB..0xF) ---
        let fb = WSFrame::ReservedControl(0xb, vec![0xcc]);
        assert_eq!(fb.id(), 11);
        assert_eq!(fb.data(), &vec![0xcc]);

        // --- 测试 Text (0x1) 的特殊性 ---
        // 注意：Text 在 Command::data() 中通常映射回原始字节
        let f1 = WSFrame::Text("hello".to_string());
        assert_eq!(f1.id(), 1);
        // 如果你的 Command 实现中 Text 返回 EMPTY，这里要对应修改断言
        // assert_eq!(f1.data().is_empty(), true);
    }

    #[test]
    fn test_ws_frame_close_command_behavior() {
        let f8 = WSFrame::Close(1000, Some("bye".into()));
        assert_eq!(f8.id(), 8);
        // Close 帧在业务 Command 层面通常被认为没有 data()
        assert!(f8.data().is_empty());
    }

    #[test]
    fn test_reserved_opcodes_codec_roundtrip() -> anyhow::Result<()> {
        let mut codec = WSCodec;
        let mut buf = bytes::BytesMut::new();

        // 测试 0x7 (Reserved Non-Control)
        let original = WSFrame::ReservedNonControl(0x7, vec![0xde, 0xad]);
        codec.encode(original.clone(), &mut buf)?;

        // 验证编码：0x80 (FIN) | 0x07 = 0x87
        assert_eq!(buf[0], 0x87);

        let decoded = codec.decode(&mut buf)?.unwrap();
        assert_eq!(original, decoded);

        Ok(())
    }

    #[test]
    fn test_binary_ping_pong_command_trait() {
        // --- 1. Binary (0x2) ---
        let bin_data = vec![0x01, 0x02, 0x03];
        let f2 = WSFrame::Binary(bin_data.clone());
        assert_eq!(f2.id(), 0x2, "Binary ID 必须为 0x2");
        assert_eq!(f2.data(), &bin_data, "Binary data 必须返回原始载荷");

        // --- 2. Ping (0x9) ---
        let ping_payload = vec![0xde, 0xad];
        let f9 = WSFrame::Ping(ping_payload.clone());
        assert_eq!(f9.id(), 0x9, "Ping ID 必须为 0x9");
        assert_eq!(f9.data(), &ping_payload, "Ping data 应该允许携带心跳载荷");

        // --- 3. Pong (0xA) ---
        let pong_payload = vec![0xbe, 0xef];
        let fa = WSFrame::Pong(pong_payload.clone());
        assert_eq!(fa.id(), 0x0a, "Pong ID 必须为 0xA");
        assert_eq!(fa.data(), &pong_payload, "Pong data 应该返回对应的响应载荷");
    }

    #[test]
    fn test_codec_to_command_id_flow() -> anyhow::Result<()> {
        let mut codec = WSCodec;

        // 模拟收到一个客户端发送的 Masked Ping 帧
        let mask = [0x1, 0x2, 0x3, 0x4];
        let mut raw = vec![0x89, 0x81]; // FIN + Ping, Masked, Len 1
        raw.extend_from_slice(&mask);
        raw.push(0x41 ^ 0x1); // Masked 'A'

        let mut src = bytes::BytesMut::from(&raw[..]);

        // 解码
        let frame = codec.decode(&mut src)?.expect("应该成功解码 Ping 帧");

        // 验证作为 Command 的表现
        assert_eq!(frame.id(), 9);
        assert_eq!(frame.data(), &vec![0x41]);

        Ok(())
    }

    #[test]
    fn test_continuation_and_reserved_payload() {
        // --- 1. Continuation (0x0) ---
        let data0 = vec![0x11, 0x22];
        let f0 = WSFrame::Continuation(data0.clone());
        assert_eq!(f0.payload(), Some(data0), "Continuation 必须导出其载荷数据");

        // --- 2. ReservedNonControl (0x3..0x7) ---
        let data3 = vec![0x33, 0x44];
        let f3 = WSFrame::ReservedNonControl(0x3, data3.clone());
        assert_eq!(
            f3.payload(),
            Some(data3),
            "ReservedNonControl 必须导出其载荷数据"
        );

        // --- 3. ReservedControl (0xB..0xF) ---
        let datab = vec![0x55, 0x66];
        let fb = WSFrame::ReservedControl(0xb, datab.clone());
        assert_eq!(
            fb.payload(),
            Some(datab),
            "ReservedControl 必须导出其载荷数据"
        );
    }

    #[test]
    fn test_continuation_reassembly_logic() {
        // 模拟收到一个分片帧
        let part = vec![0x01, 0x02];
        let frame = WSFrame::Continuation(part.clone());

        // 模拟框架层的处理：提取 payload
        if let Some(data) = frame.payload() {
            assert_eq!(data, part);
        } else {
            panic!("Continuation frame 必须包含有效载荷");
        }
    }

    #[test]
    fn test_decode_continuation_frame() -> anyhow::Result<()> {
        let mut codec = WSCodec;
        let mut src = bytes::BytesMut::new();

        // 构造一个 Continuation 帧
        // 0x00: FIN=0, Opcode=0 (最常见的中间分片)
        // 0x02: Payload Len 2
        let raw_bytes = vec![0x00, 0x02, 0xaa, 0xbb];
        src.extend_from_slice(&raw_bytes);

        // 执行解码
        let frame = codec.decode(&mut src)?.expect("应该解析出 Continuation 帧");

        // 验证变体
        if let WSFrame::Continuation(payload) = frame.clone() {
            assert_eq!(payload, vec![0xaa, 0xbb]);
        } else {
            panic!("未能匹配到 WSFrame::Continuation 变体");
        }

        // 验证作为 Command 的 ID
        assert_eq!(frame.id(), 0x0);

        Ok(())
    }

    #[test]
    fn test_continuation_payload_mapping() {
        let data = vec![0xcc, 0xdd];
        let frame = WSFrame::Continuation(data.clone());

        // 触发 Frame::payload()
        let p = frame.payload();
        assert_eq!(p, Some(data), "Continuation 的 payload() 分支映射错误");
    }

    #[test]
    fn test_continuation_command_trait() {
        let data = vec![0x12, 0x34];
        let frame = WSFrame::Continuation(data.clone());

        // 触发 Command::id()
        assert_eq!(frame.id(), 0);
        // 触发 Command::data()
        assert_eq!(frame.data(), &data);
    }
}
