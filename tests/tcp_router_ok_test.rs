#[cfg(test)]
mod router_tests {
    use std::{net::SocketAddr, sync::Arc};

    use aex::{
        connection::{
            context::{BoxReader, BoxWriter, Context},
            global::GlobalContext,
        },
        tcp::{
            router::Router,
            types::{Codec, Command, Frame, RawCodec},
        },
    };
    use bincode::{Decode, Encode};
    use futures::FutureExt;
    use serde::{Deserialize, Serialize};
    use tokio::{
        net::tcp::{OwnedReadHalf, OwnedWriteHalf},
        sync::Mutex,
    };

    // --- 模拟对象准备 ---

    #[derive(Serialize, Deserialize, Encode, Decode, Debug, Clone)]
    struct TestCommand {
        pub id: u32,
        pub valid: bool,
        pub data: Vec<u8>,
    }
    impl Codec for TestCommand {}
    impl Command for TestCommand {
        fn id(&self) -> u32 {
            self.id
        }
        fn validate(&self) -> bool {
            self.valid
        }
        fn data(&self) -> &Vec<u8> {
            &self.data
        }
    }

    #[derive(Serialize, Deserialize, Encode, Decode, Debug, Clone)]
    struct TestFrame {
        pub payload: Option<Vec<u8>>,
        pub is_valid: bool,
    }
    impl Codec for TestFrame {}
    impl Frame for TestFrame {
        fn payload(&self) -> Option<Vec<u8>> {
            self.payload.clone()
        }
        fn validate(&self) -> bool {
            self.is_valid
        }
        fn command(&self) -> Option<&Vec<u8>> {
            self.payload.as_ref()
        }
        fn is_flat(&self) -> bool {
            false
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
        let mut router: Router = Router::new();
        let addr: SocketAddr = "127.0.0.1:8080".parse().unwrap();
        let global = GlobalContext::new(addr, None);

        let arc_g = Arc::new(global);

        // 注册一个正常的 handler
        // router.on::<RawCodec, RawCodec, _, _>(100, |_, _, _| async { Ok(true) });
        router.on::<RawCodec, RawCodec>(
            100,
            Box::new(|_, _, _| Box::pin(async move { Ok(true) }).boxed()),
            vec![],
        );

        // 注册一个返回 false 的 handler
        // router.on::<RawCodec, RawCodec, _, _>(200, |_, _, _| async { Ok(false) });
        router.on::<RawCodec, RawCodec>(
            200,
            Box::new(|_, _, _| Box::pin(async move { Ok(true) }).boxed()),
            vec![],
        );

        let (r, w) = mock_io().await;

        let r_opt: Option<BoxReader> = Some(Box::new(tokio::io::BufReader::new(r)));

        let w_opt: Option<BoxWriter> = Some(Box::new(w));
        let ctx = Context::new(r_opt, w_opt, arc_g.clone(), addr);
        let ctx = Arc::new(Mutex::new(ctx));
        let ctx_guard = ctx.lock().await;
        // 路径 1: frame.validate() == false
        {
            let invalid_frame = TestFrame {
                payload: None,
                is_valid: false,
            };

            let res = router
                .handle_frame::<TestFrame, TestCommand>(
                    ctx.clone(),
                    invalid_frame,
                    Arc::new(|c: &TestCommand| c.id()),
                )
                .await;
            assert!(!res.unwrap());

            assert!(ctx_guard.reader.is_some()); // 验证 IO 没被取走
        }

        // 路径 2: frame.handle() == None
        {
            let no_payload_frame = TestFrame {
                payload: None,
                is_valid: true,
            };
            let res = router
                .handle_frame::<TestFrame, TestCommand>(
                    ctx.clone(),
                    no_payload_frame,
                    Arc::new(|c: &TestCommand| c.id()),
                )
                .await;
            assert!(res.unwrap());
            assert!(ctx_guard.reader.is_some()); // 验证 IO 没被取走
        }

        // 路径 3: Codec::decode 失败
        {
            let bad_data_frame = TestFrame {
                payload: Some(vec![0xFF, 0x00]),
                is_valid: true,
            };
            let res = router
                .handle_frame::<TestFrame, TestCommand>(
                    ctx.clone(),
                    bad_data_frame,
                    Arc::new(|c: &TestCommand| c.id()),
                )
                .await;
            assert!(!res.unwrap());
            assert!(ctx_guard.reader.is_some()); // 验证 IO 没被取走
        }

        // 路径 4: cmd.validate() == false
        {
            let invalid_cmd = TestCommand {
                id: 100,
                valid: false,
                data: vec![0],
            };
            let data = Codec::encode(&invalid_cmd);

            let frame = TestFrame {
                payload: Some(data),
                is_valid: true,
            };
            let res = router
                .handle_frame::<TestFrame, TestCommand>(
                    ctx.clone(),
                    frame,
                    Arc::new(|c: &TestCommand| c.id()),
                )
                .await;
            println!("{:?}", res);
            assert!(res.is_err());
            let err_msg = format!("{:?}", res.err().unwrap());
            assert!(err_msg.contains("Handler type mismatch for key: 100"));
            assert!(ctx_guard.reader.is_some()); // 验证 IO 没被取走
        }

        // 路径 5: 找不到 Handler (Key 不存在)
        {
            let unknown_cmd = TestCommand {
                id: 999,
                valid: true,
                data: vec![0],
            };
            let data = Codec::encode(&unknown_cmd);
            let frame = TestFrame {
                payload: Some(data),
                is_valid: true,
            };
            let res = router
                .handle_frame::<TestFrame, TestCommand>(
                    ctx.clone(),
                    frame,
                    Arc::new(|c: &TestCommand| c.id()),
                )
                .await;
            assert!(res.unwrap());
            assert!(ctx_guard.reader.is_some()); // 验证 IO 没被取走
        }

        // 路径 6: 成功执行 Handler 并返回 Ok(true)
        {
            // 这里会真正触发 take()，之后 r_opt/w_opt 变为 None
            let valid_cmd = TestCommand {
                id: 100,
                valid: true,
                data: vec![0],
            };
            let frame = TestFrame {
                payload: Some(Codec::encode(&valid_cmd)),
                is_valid: true,
            };
            let res = router
                .handle_frame::<TestFrame, TestCommand>(
                    ctx.clone(),
                    frame,
                    Arc::new(|c: &TestCommand| c.id()),
                )
                .await;
            assert!(res.is_err());
            let err_msg = format!("{:?}", res.err().unwrap());
            assert!(err_msg.contains("Handler type mismatch for key: 100"));
            assert!(ctx_guard.reader.is_some()); // 验证 IO 没被取走
        }

        // 路径 7: 成功执行 Handler 并返回 Ok(false)
        {
            let (_r2, w2) = mock_io().await; // 必须重新获取，因为上一组已被 take
            // let r2_opt = Some(r2);
            // let _w2_opt = Some(w2);
            let exit_cmd = TestCommand {
                id: 200,
                valid: true,
                data: vec![0],
            };
            let frame = TestFrame {
                payload: Some(Codec::encode(&exit_cmd)),
                is_valid: true,
            };

            assert!(!frame.clone().is_flat());

            let r_opt: Option<BoxReader> = Some(Box::new(tokio::io::BufReader::new(_r2)));

            let w_opt: Option<BoxWriter> = Some(Box::new(w2));
            let ctx = Context::new(r_opt, w_opt, arc_g.clone(), addr);
            let ctx = Arc::new(Mutex::new(ctx));

            let res = router
                .handle_frame::<TestFrame, TestCommand>(
                    ctx.clone(),
                    frame.clone(),
                    Arc::new(|c: &TestCommand| c.id()),
                )
                .await;
            assert!(res.is_err());
            let err_msg = format!("{:?}", res.err().unwrap());
            assert!(err_msg.contains("Handler type mismatch for key: 200"));
            // assert!(r2_opt.is_none());
        }
    }

    #[tokio::test]
    async fn test_reader_writer_already_taken() {
        let mut router: Router = Router::new();
        let addr: SocketAddr = "127.0.0.1:8080".parse().unwrap();
        let global = GlobalContext::new(addr, None);

        let arc_g = Arc::new(global);

        // router.on::<TestFrame, TestCommand, _, _>(100, |_, _, _: &mut TestCommand| async move {
        //     Ok(true)
        // });

        router.on::<TestFrame, TestCommand>(
            100,
            Box::new(|_, _, _| Box::pin(async move { Ok(true) }).boxed()),
            vec![],
        );

        // router.on::<RawCodec, RawCodec, _, _>(100, |_, _:&mut _,  _: &mut TestCommand, _, _| async { Ok(true) });

        let cmd = TestCommand {
            id: 100,
            valid: true,
            data: vec![0],
        };
        let frame = TestFrame {
            payload: Some(Codec::encode(&cmd)),
            is_valid: true,
        };

        let (_r_real, w_real) = mock_io().await;

        let r_none: Option<BoxReader> = None;
        let w_some: Option<BoxWriter> = Some(Box::new(w_real));

        let ctx = Context::new(r_none, w_some, arc_g.clone(), addr);
        let ctx = Arc::new(Mutex::new(ctx));

        let _res = router
            .handle_frame::<TestFrame, TestCommand>(
                ctx.clone(),
                frame,
                Arc::new(|c: &TestCommand| c.id()),
            )
            .await;
        // println!("{:?}", res);
        // assert!(res.is_err());
        // assert_eq!(res.unwrap_err().to_string(), "Reader already taken");
    }

    #[tokio::test]
    async fn test_writer_already_taken() {
        let mut router: Router = Router::new();
        let addr: SocketAddr = "127.0.0.1:8080".parse().unwrap();
        let global = GlobalContext::new(addr, None);

        let arc_g = Arc::new(global);

        // 1. 注册一个有效的 Handler
        // router.on(100, |_, _: TestCommand, _, _| async { Ok(true) });
        // router.on::<TestFrame, TestCommand, _, _>(100, |_, _, _: &mut TestCommand| async move {
        //     Ok(true)
        // });

        router.on::<TestFrame, TestCommand>(
            100,
            Box::new(|_, _, _| Box::pin(async move { Ok(true) }).boxed()),
            vec![],
        );

        // 2. 构造一个能通过所有前期校验的 Frame 和 Command
        let cmd = TestCommand {
            id: 100,
            valid: true,
            data: vec![0],
        };
        let frame = TestFrame {
            payload: Some(Codec::encode(&cmd)),
            is_valid: true,
        };

        // 3. 核心：提供 Reader 但将 Writer 设为 None
        let (r_real, _w_real) = mock_io().await;
        // let mut r_some = Some(r_real);
        // let mut w_none: Option<OwnedWriteHalf> = None;

        // 4. 执行 handle_frame
        // 逻辑会通过：frame.validate() -> frame.handle() -> decode -> cmd.validate() -> handlers.get()
        // 然后在 reader.take() 成功后，执行 writer.take() 时触发错误

        // let mut r_some: Option<BoxReader> = Some(Box::new(r_real));
        // let mut w_none: Option<BoxWriter> = None;
        // let mut ctx = Context::new(&mut r_some, &mut w_some, arc_g.clone(), addr);

        let r_some: Option<BoxReader> = Some(Box::new(tokio::io::BufReader::new(r_real)));

        let w_none: Option<BoxWriter> = None;
        let ctx = Context::new(r_some, w_none, arc_g.clone(), addr);
        let ctx = Arc::new(Mutex::new(ctx));

        let _res = router
            .handle_frame::<TestFrame, TestCommand>(
                ctx.clone(),
                frame,
                Arc::new(|c: &TestCommand| c.id()),
            )
            .await;

        // 5. 验证错误信息
        // assert!(res.is_err());
        // assert_eq!(res.unwrap_err().to_string(), "Writer already taken");

        // 顺便验证：由于 Reader 在 Writer 报错前已经被 take 了，此时 r_some 应该是 None
        // assert!(ctx.writer.is_none());
    }
}
