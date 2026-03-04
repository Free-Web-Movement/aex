#[cfg(test)]
mod tests {
    use aex::connection::global::GlobalContext;
    use std::net::SocketAddr;

    // 定义两个用于测试的结构体
    #[derive(Debug, Clone, PartialEq)]
    struct MyConfig {
        port: u16,
        enabled: bool,
    }

    #[derive(Debug, Clone, PartialEq)]
    struct DatabasePool {
        connection_string: String,
    }

    #[tokio::test]
    async fn test_global_context_extensions() {
        // 1. 初始化 Context
        let addr: SocketAddr = "127.0.0.1:8080".parse().unwrap();
        let ctx = GlobalContext::new(addr);

        // 2. 准备测试数据
        let config = MyConfig { port: 3000, enabled: true };
        let db_pool = DatabasePool { connection_string: "postgres://localhost".to_string() };

        // 3. 测试 set 功能
        ctx.set(config.clone()).await;
        ctx.set(db_pool.clone()).await;

        // 4. 测试 get 功能
        let fetched_config = ctx.get::<MyConfig>().await.expect("Should find MyConfig");
        let fetched_db = ctx.get::<DatabasePool>().await.expect("Should find DatabasePool");

        // 5. 断言验证
        assert_eq!(fetched_config, config);
        assert_eq!(fetched_db, db_pool);

        // 6. 测试不存在的情况
        let non_existent = ctx.get::<String>().await;
        assert!(non_existent.is_none());

        // 7. 测试覆盖更新
        let new_config = MyConfig { port: 9000, enabled: false };
        ctx.set(new_config.clone()).await;
        let updated_config = ctx.get::<MyConfig>().await.unwrap();
        assert_eq!(updated_config.port, 9000);
    }
}