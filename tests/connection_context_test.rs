#[cfg(test)]
mod tests {
    use aex::connection::context::{Context, GlobalContext, TypeMap, TypeMapExt};
    use std::{net::SocketAddr, sync::Arc};

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
}