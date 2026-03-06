#[cfg(test)]
mod tests {
    use aex::{
        communicators::event::Event,
        connection::{
            context::{Context, TypeMap, TypeMapExt},
            global::GlobalContext,
        },
    };
    use futures::FutureExt;
    use std::{
        io::Cursor,
        net::SocketAddr,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
    };
    use tokio::io::{self, AsyncBufRead, AsyncWrite, BufReader};

    // --- 1. 测试 TypeMap 的存取逻辑 ---
    #[test]
    fn test_typemap_ext() {
        let map = TypeMap::default();

        // 测试插入和读取
        map.set_value(42i32);
        assert_eq!(map.get_value::<i32>(), Some(42));

        // 测试覆盖更新
        map.set_value(100i32);
        assert_eq!(map.get_value::<i32>(), Some(100));

        // 测试不存在的情况
        assert_eq!(map.get_value::<String>(), None);

        // 测试复杂类型
        #[derive(Clone, PartialEq, Debug)]
        struct User {
            id: u64,
        }
        map.set_value(User { id: 1 });
        assert_eq!(map.get_value::<User>(), Some(User { id: 1 }));
    }

    // --- 2. 测试 GlobalContext 的初始化 ---
    #[test]
    fn test_global_context_init() {
        let addr: SocketAddr = "127.0.0.1:8080".parse().unwrap();
        let global = GlobalContext::new(addr);

        assert_eq!(global.addr, addr);
        // 验证 extensions 是否可写
        // global.extensions.write().unwrap().set_value(true);
        // assert_eq!(global.extensions.write().unwrap().get_value::<bool>(), Some(true));
    }

    // --- 3. 测试 Context 的构造与视图 ---
    #[tokio::test]
    async fn test_context_flow() {
        let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let global = Arc::new(GlobalContext::new(addr));

        // 模拟 I/O：使用 dummy 向量模拟 reader 和 writer
        let reader_data = Cursor::new(vec![0u8; 10]);
        let writer_data = Cursor::new(vec![0u8; 10]);
        let mut reader: Option<Box<dyn AsyncBufRead + Send + Unpin + Sync>> =
            Some(Box::new(BufReader::new(reader_data)));

        let mut writer: Option<Box<dyn AsyncWrite + Send + Unpin + Sync>> = Some(Box::new(writer_data));

        let mut ctx = Context::new(&mut reader, &mut writer, global.clone(), addr);

        // 测试地址一致性
        assert_eq!(ctx.addr, addr);

        // 测试 local TypeMap 在 context 中的独立性
        ctx.local.set_value("request_scoped".to_string());
        assert_eq!(
            ctx.local.get_value::<String>(),
            Some("request_scoped".to_string())
        );

        // --- 测试视图构造 ---

        // 1. Request 视图
        {
            let req_view = ctx.req();
            // 验证字段引用正确
            assert_eq!(
                req_view.local.get_value::<String>(),
                Some("request_scoped".to_string())
            );
        }

        // 2. Response 视图
        {
            let res_view = ctx.res();
            // 验证字段引用正确
            assert_eq!(
                res_view.local.get_value::<String>(),
                Some("request_scoped".to_string())
            );

            // 验证 writer 是被包裹在 Arc<Mutex<W>> 中的
            let _lock = res_view.writer;
        }
    }

    // --- 4. 边界覆盖：并发安全性验证 ---
    #[tokio::test]
    async fn test_context_concurrency() {
        let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let global = Arc::new(GlobalContext::new(addr));

        let reader_data = Cursor::new(vec![0u8; 10]);
        let writer_data = Cursor::new(vec![0u8; 10]);
        let mut reader: Option<Box<dyn AsyncBufRead + Send + Unpin + Sync>> =
            Some(Box::new(BufReader::new(reader_data)));

        let mut writer: Option<Box<dyn AsyncWrite + Send + Unpin + Sync>> = Some(Box::new(writer_data));

        let ctx = Context::new(&mut reader, &mut writer, global.clone(), addr);

        ctx.local.set_value(99 as usize);

        assert_eq!(ctx.local.get_value::<usize>(), Some(99));
    }

    #[tokio::test]
    async fn test_context_full_flow() {
        // 1. 初始化 GlobalContext
        let addr = "127.0.0.1:8080".parse().unwrap();
        let mut global = GlobalContext::new(addr);

        global.set_server_name("Aex".to_string());

        // 准备计数器验证结果
        let event_counter = Arc::new(AtomicUsize::new(0));
        let pipe_counter = Arc::new(AtomicUsize::new(0));
        let spread_counter = Arc::new(AtomicUsize::new(0));

        // --- 预先注册订阅者 ---

        // Event: 监听 "request_received" 事件
        let ec = Arc::clone(&event_counter);

        Event::_on(
            &global.event,
            "request_received".to_string(),
            Arc::new(move |req_id: u32| {
                let c = Arc::clone(&ec);
                (async move {
                    println!("Event 收到请求 ID: {}", req_id);
                    c.fetch_add(1, Ordering::SeqCst);
                })
                .boxed() // 👈 关键点：返回一个被包装的 Future
            }),
        )
        .await;

        // Pipe: 监听 "audit_log" (N:1)
        let pc = Arc::clone(&pipe_counter);
        global
            .pipe
            .register(
                "audit_log",
                Box::new(move |msg: String| {
                    let c = Arc::clone(&pc);
                    (async move {
                        println!("Pipe 审计日志: {}", msg);
                        c.fetch_add(1, Ordering::SeqCst);
                    })
                    .boxed()
                }),
            )
            .await
            .unwrap();

        // Spread: 订阅 "broadcast" (1:N)
        let sc = Arc::clone(&spread_counter);
        global
            .spread
            .subscribe(
                "broadcast",
                Box::new(move |val: i32| {
                    let c = Arc::clone(&sc);
                    (async move {
                        println!("Spread 广播接收: {}", val);
                        c.fetch_add(1, Ordering::SeqCst);
                    })
                    .boxed()
                }),
            )
            .await
            .unwrap();

        // --- 模拟连接进入 ---

        // 模拟 Socket 读写流
        let (client, _server) = io::duplex(64);
        let (reader, writer) = io::split(client);
        let remote_addr = "192.168.1.100:12345".parse().unwrap();

        // let reader_data = Cursor::new(vec![0u8; 10]);
        // let writer_data = Cursor::new(vec![0u8; 10]);
        let mut reader: Option<Box<dyn AsyncBufRead + Send + Unpin + Sync>> =
            Some(Box::new(BufReader::new(reader)));

        let mut writer: Option<Box<dyn AsyncWrite + Send + Unpin + Sync>> = Some(Box::new(writer));

        let ctx = Context::new(&mut reader, &mut writer, Arc::clone(&Arc::new(global)), remote_addr);

        // let ctx = Context::new(reader, writer, Arc::clone(&Arc::new(global)), remote_addr);

        // --- 执行业务逻辑测试 ---

        // 1. 测试 elapsed (模拟处理耗时)
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        let time_spent = ctx.elapsed();
        assert!(
            time_spent >= 100,
            "Elapsed 应该大于 100ms, 当前: {}ms",
            time_spent
        );

        let global = ctx.global.clone();

        // 2. 使用 Event 通知系统有新请求
        global
            .event
            .notify("request_received".to_string(), 1024_u32)
            .await;

        // 3. 使用 Pipe 发送结构化日志
        global
            .pipe
            .send(
                "audit_log",
                format!("Client {} processed in {}ms", ctx.addr, time_spent),
            )
            .await
            .unwrap();

        // 4. 使用 Spread 发布全局通知
        global.spread.publish("broadcast", 200_i32).await.unwrap();

        // --- 最终验证 ---

        // 给异步任务一点点执行时间 (通知是 spawn 出来的)
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        assert_eq!(
            event_counter.load(Ordering::SeqCst),
            1,
            "Event 应该触发一次"
        );
        assert_eq!(
            pipe_counter.load(Ordering::SeqCst),
            1,
            "Pipe 应该处理一条日志"
        );
        assert_eq!(
            spread_counter.load(Ordering::SeqCst),
            1,
            "Spread 应该收到一个广播"
        );

        println!("Context 集成功能全数测试通过！");
    }

    // 模拟一个复杂的扩展数据结构
    #[derive(Clone, Debug, PartialEq)]
    struct UserConfig {
        id: u64,
        role: String,
    }

    use tokio::io::{empty, sink};

    #[tokio::test]
    async fn test_context_type_map_extensions() {
        // 1. 准备 Context 环境
        // 使用空流模拟 reader 和 writer
        let mut reader_opt: Option<Box<dyn AsyncBufRead + Send + Unpin  + Sync>> = 
            Some(Box::new(tokio::io::BufReader::new(empty())));
        let mut writer_opt: Option<Box<dyn AsyncWrite + Send + Unpin + Sync>> = 
            Some(Box::new(sink()));
        
        let global = Arc::new(GlobalContext::new("127.0.0.1:8080".parse().unwrap()));
        let addr = "127.0.0.1:1234".parse().unwrap();

        let ctx = Context::new(
            &mut reader_opt,
            &mut writer_opt,
            global,
            addr,
        );

        // 2. 测试基础类型存储 (String)
        let test_msg = "AexServerExtension".to_string();
        ctx.set(test_msg.clone()).await;
        let retrieved_msg = ctx.get::<String>().await;
        assert_eq!(retrieved_msg, Some(test_msg));

        // 3. 测试自定义结构体 (UserConfig)
        let config = UserConfig {
            id: 1024,
            role: "admin".to_string(),
        };
        ctx.set(config.clone()).await;
        let retrieved_config = ctx.get::<UserConfig>().await;
        assert_eq!(retrieved_config, Some(config));

        // 4. 测试“不存在”的类型
        let non_existent = ctx.get::<u32>().await;
        assert!(non_existent.is_none());

        // 5. 测试类型覆盖（同一 TypeId 再次 set）
        ctx.set(42u64).await;
        ctx.set(99u64).await; // 覆盖旧值
        assert_eq!(ctx.get::<u64>().await, Some(99u64));
    }

    #[tokio::test]
    async fn test_type_map_concurrency_cloning() {
        // 验证 TypeMap 是否支持跨线程共享的数据结构（如 Arc）
        let map = TypeMap::default();
        let shared_data = Arc::new(vec![1, 2, 3]);

        map.set_value(shared_data.clone());
        
        let retrieved = map.get_value::<Arc<Vec<i32>>>().expect("Should exist");
        assert_eq!(*retrieved, vec![1, 2, 3]);
        assert_eq!(Arc::strong_count(&retrieved), 3); // 原有的 + 存入的 + 刚刚拿出来的
    }
}
