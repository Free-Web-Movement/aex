#[cfg(test)]
mod tests {
    use aex::{communicators::event::Event, connection::context::{Context, GlobalContext, TypeMap, TypeMapExt}};
    use futures::FutureExt;
    use tokio::io;
    use std::{net::SocketAddr, sync::{Arc, atomic::{AtomicUsize, Ordering}}};

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
        struct User { id: u64 }
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
        global.extensions.blocking_write().set_value(true);
        assert_eq!(global.extensions.blocking_read().get_value::<bool>(), Some(true));
    }

    // --- 3. 测试 Context 的构造与视图 ---
    #[tokio::test]
    async fn test_context_flow() {
        let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let global = Arc::new(GlobalContext::new(addr));

        // 模拟 I/O：使用 dummy 向量模拟 reader 和 writer
        let reader = vec![0u8; 10];
        let writer = vec![0u8; 10];

        let mut ctx = Context::new(reader, writer, global.clone(), addr);

        // 测试地址一致性
        assert_eq!(ctx.addr, addr);

        // 测试 local TypeMap 在 context 中的独立性
        ctx.local.set_value("request_scoped".to_string());
        assert_eq!(ctx.local.get_value::<String>(), Some("request_scoped".to_string()));

        // --- 测试视图构造 ---
        
        // 1. Request 视图
        {
            let req_view = ctx.req().await;
            // 验证字段引用正确
            assert_eq!(req_view.local.get_value::<String>(), Some("request_scoped".to_string()));
        }

        // 2. Response 视图
        {
            let res_view = ctx.res();
            // 验证字段引用正确
            assert_eq!(res_view.local.get_value::<String>(), Some("request_scoped".to_string()));
            
            // 验证 writer 是被包裹在 Arc<Mutex<W>> 中的
            let _lock = res_view.writer.lock().await;
        }
    }

    // --- 4. 边界覆盖：并发安全性验证 ---
    #[tokio::test]
    async fn test_context_concurrency() {
        let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let global = Arc::new(GlobalContext::new(addr));
        let ctx = Arc::new(tokio::sync::Mutex::new(
            Context::<Vec<u8>, Vec<u8>>::new(vec![], vec![], global, addr)
        ));

        let ctx_clone = ctx.clone();
        let handle = tokio::spawn(async move {
            let guard = ctx_clone.lock().await;
            guard.local.set_value(99usize);
        });

        handle.await.unwrap();
        assert_eq!(ctx.lock().await.local.get_value::<usize>(), Some(99));
    }
    
    #[tokio::test]
    async fn test_context_full_flow() {
        // 1. 初始化 GlobalContext
        let addr = "127.0.0.1:8080".parse().unwrap();
        let global = Arc::new(GlobalContext::new(addr));

        // 准备计数器验证结果
        let event_counter = Arc::new(AtomicUsize::new(0));
        let pipe_counter = Arc::new(AtomicUsize::new(0));
        let spread_counter = Arc::new(AtomicUsize::new(0));

        // --- 预先注册订阅者 ---

        // Event: 监听 "request_received" 事件
        let ec = Arc::clone(&event_counter);
        global.event.on("request_received".to_string(), move |req_id: u32| {
            let c = Arc::clone(&ec);
            async move {
                println!("Event 收到请求 ID: {}", req_id);
                c.fetch_add(1, Ordering::SeqCst);
            }.boxed()
        }).await;

        // Pipe: 监听 "audit_log" (N:1)
        let pc = Arc::clone(&pipe_counter);
        global.pipe.register("audit_log", move |msg: String| {
            let c = Arc::clone(&pc);
            async move {
                println!("Pipe 审计日志: {}", msg);
                c.fetch_add(1, Ordering::SeqCst);
            }.boxed()
        }).await.unwrap();

        // Spread: 订阅 "broadcast" (1:N)
        let sc = Arc::clone(&spread_counter);
        global.spread.subscribe("broadcast", move |val: i32| {
            let c = Arc::clone(&sc);
            async move {
                println!("Spread 广播接收: {}", val);
                c.fetch_add(1, Ordering::SeqCst);
            }.boxed()
        }).await.unwrap();

        // --- 模拟连接进入 ---

        // 模拟 Socket 读写流
        let (client, _server) = io::duplex(64);
        let (reader, writer) = io::split(client);
        let remote_addr = "192.168.1.100:12345".parse().unwrap();

        let ctx = Context::new(reader, writer, Arc::clone(&global), remote_addr);

        // --- 执行业务逻辑测试 ---

        // 1. 测试 elapsed (模拟处理耗时)
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        let time_spent = ctx.elapsed();
        assert!(time_spent >= 100, "Elapsed 应该大于 100ms, 当前: {}ms", time_spent);

        // 2. 使用 Event 通知系统有新请求
        ctx.global.event.notify("request_received".to_string(), 1024_u32).await;

        // 3. 使用 Pipe 发送结构化日志
        ctx.global.pipe.send("audit_log", format!("Client {} processed in {}ms", ctx.addr, time_spent)).await.unwrap();

        // 4. 使用 Spread 发布全局通知
        ctx.global.spread.publish("broadcast", 200_i32).await.unwrap();

        // --- 最终验证 ---
        
        // 给异步任务一点点执行时间 (通知是 spawn 出来的)
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        assert_eq!(event_counter.load(Ordering::SeqCst), 1, "Event 应该触发一次");
        assert_eq!(pipe_counter.load(Ordering::SeqCst), 1, "Pipe 应该处理一条日志");
        assert_eq!(spread_counter.load(Ordering::SeqCst), 1, "Spread 应该收到一个广播");

        println!("Context 集成功能全数测试通过！");
    }
}