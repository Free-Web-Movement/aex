#[cfg(test)]
mod router_tests {
    use aex::tcp::{
        router::Router,
        types::{Codec, Command, Frame},
    };
    use bincode::{Decode, Encode};
    use serde::{Deserialize, Serialize};
    use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};

    // --- 模拟对象准备 ---

    #[derive(Serialize, Deserialize, Encode, Decode, Debug, Clone)]
    struct TestCommand {
        pub id: u32,
        pub valid: bool,
    }
    impl Codec for TestCommand {}
    impl Command for TestCommand {
        fn id(&self) -> u32 {
            self.id
        }
        fn validate(&self) -> bool {
            self.valid
        }
    }

    #[derive(Serialize, Deserialize, Encode, Decode, Debug, Clone)]
    struct TestFrame {
        pub payload: Option<Vec<u8>>,
        pub is_valid: bool,
    }
    impl Codec for TestFrame {}
    impl Frame for TestFrame {
        fn payload(&self) -> Option<&[u8]> {
            self.payload.as_deref()
        }
        fn validate(&self) -> bool {
            self.is_valid
        }
        fn handle(&self) -> Option<Vec<u8>> {
            self.payload.clone()
        }
    }

    // 辅助函数：创建 Mock IO
    async fn mock_io() -> (OwnedReadHalf, OwnedWriteHalf) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let _client = tokio::net::TcpStream::connect(addr).await.unwrap();
        let (stream, _) = listener.accept().await.unwrap();
        stream.into_split()
    }

    // --- 测试用例 ---

    #[tokio::test]
    async fn test_handle_frame_coverage() {
        let mut router: Router<TestFrame, TestCommand, u32> = Router::new(|c: &TestCommand| c.id());

        // 注册一个正常的 handler
        router.on(100, |_, _, _| async { Ok(true) });
        // 注册一个返回 false 的 handler
        router.on(200, |_, _, _| async { Ok(false) });

        let (r, w) = mock_io().await;
        // 💡 修复点：预先放入 Option，避免在参数位置生成临时 Option 导致 move
        let mut r_opt = Some(r);
        let mut w_opt = Some(w);

        // 路径 1: frame.validate() == false
        {
            let invalid_frame = TestFrame {
                payload: None,
                is_valid: false,
            };
            let res = router
                .handle_frame(invalid_frame, &mut r_opt, &mut w_opt)
                .await;
            assert!(res.unwrap()); 
            assert!(r_opt.is_some()); // 验证 IO 没被取走
        }

        // 路径 2: frame.handle() == None
        {
            let no_payload_frame = TestFrame {
                payload: None,
                is_valid: true,
            };
            let res = router
                .handle_frame(no_payload_frame, &mut r_opt, &mut w_opt)
                .await;
            assert!(res.unwrap());
            assert!(r_opt.is_some());
        }

        // 路径 3: Codec::decode 失败
        {
            let bad_data_frame = TestFrame {
                payload: Some(vec![0xFF, 0x00]),
                is_valid: true,
            };
            let res = router
                .handle_frame(bad_data_frame, &mut r_opt, &mut w_opt)
                .await;
            assert!(res.unwrap());
            assert!(r_opt.is_some());
        }

        // 路径 4: cmd.validate() == false
        {
            let invalid_cmd = TestCommand {
                id: 100,
                valid: false,
            };
            let data = Codec::encode(&invalid_cmd);

            let frame = TestFrame {
                payload: Some(data),
                is_valid: true,
            };
            let res = router.handle_frame(frame, &mut r_opt, &mut w_opt).await;
            assert!(res.unwrap());
            assert!(r_opt.is_some());
        }

        // 路径 5: 找不到 Handler (Key 不存在)
        {
            let unknown_cmd = TestCommand {
                id: 999,
                valid: true,
            };
            let data = Codec::encode(&unknown_cmd);
            let frame = TestFrame {
                payload: Some(data),
                is_valid: true,
            };
            let res = router.handle_frame(frame, &mut r_opt, &mut w_opt).await;
            assert!(res.unwrap());
            assert!(r_opt.is_some());
        }

        // 路径 6: 成功执行 Handler 并返回 Ok(true)
        {
            // 这里会真正触发 take()，之后 r_opt/w_opt 变为 None
            let valid_cmd = TestCommand {
                id: 100,
                valid: true,
            };
            let frame = TestFrame {
                payload: Some(Codec::encode(&valid_cmd)),
                is_valid: true,
            };
            let res = router.handle_frame(frame, &mut r_opt, &mut w_opt).await;
            assert!(res.unwrap());
            assert!(r_opt.is_none()); // 确认所有权被转移
        }

        // 路径 7: 成功执行 Handler 并返回 Ok(false)
        {
            let (r2, w2) = mock_io().await; // 必须重新获取，因为上一组已被 take
            let mut r2_opt = Some(r2);
            let mut w2_opt = Some(w2);
            let exit_cmd = TestCommand {
                id: 200,
                valid: true,
            };
            let frame = TestFrame {
                payload: Some(Codec::encode(&exit_cmd)),
                is_valid: true,
            };
            let res = router.handle_frame(frame, &mut r2_opt, &mut w2_opt).await;
            assert!(!res.unwrap()); 
            assert!(r2_opt.is_none());
        }
    }

    #[tokio::test]
    async fn test_reader_writer_already_taken() {
        let mut router: Router<TestFrame, TestCommand, u32> = Router::new(|c: &TestCommand| c.id());
        router.on(100, |_, _, _| async { Ok(true) });

        let cmd = TestCommand {
            id: 100,
            valid: true,
        };
        let frame = TestFrame {
            payload: Some(Codec::encode(&cmd)),
            is_valid: true,
        };

        let mut r_none: Option<OwnedReadHalf> = None;
        let (r_real, w_real) = mock_io().await;
        let mut w_some = Some(w_real);

        let res = router.handle_frame(frame, &mut r_none, &mut w_some).await;
        assert!(res.is_err());
        assert_eq!(res.unwrap_err().to_string(), "Reader already taken");
    }

    #[tokio::test]
    async fn test_writer_already_taken() {
        let mut router: Router<TestFrame, TestCommand, u32> = Router::new(|c: &TestCommand| c.id());
        
        // 1. 注册一个有效的 Handler
        router.on(100, |_, _, _| async { Ok(true) });

        // 2. 构造一个能通过所有前期校验的 Frame 和 Command
        let cmd = TestCommand {
            id: 100,
            valid: true,
        };
        let frame = TestFrame {
            payload: Some(Codec::encode(&cmd)),
            is_valid: true,
        };

        // 3. 核心：提供 Reader 但将 Writer 设为 None
        let (r_real, _w_real) = mock_io().await;
        let mut r_some = Some(r_real);
        let mut w_none: Option<OwnedWriteHalf> = None;

        // 4. 执行 handle_frame
        // 逻辑会通过：frame.validate() -> frame.handle() -> decode -> cmd.validate() -> handlers.get()
        // 然后在 reader.take() 成功后，执行 writer.take() 时触发错误
        let res = router.handle_frame(frame, &mut r_some, &mut w_none).await;

        // 5. 验证错误信息
        assert!(res.is_err());
        assert_eq!(res.unwrap_err().to_string(), "Writer already taken");
        
        // 顺便验证：由于 Reader 在 Writer 报错前已经被 take 了，此时 r_some 应该是 None
        assert!(r_some.is_none());
    }
}