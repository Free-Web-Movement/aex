#[cfg(test)]
mod tests {
    use aex::connection::global::GlobalContext;
    use std::{net::SocketAddr, sync::Arc, time::Duration};
    use tokio::task;
    use tokio_util::sync::CancellationToken;

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
        let ctx = GlobalContext::new(addr, None);

        // 2. 准备测试数据
        let config = MyConfig {
            port: 3000,
            enabled: true,
        };
        let db_pool = DatabasePool {
            connection_string: "postgres://localhost".to_string(),
        };

        // 3. 测试 set 功能
        ctx.set(config.clone()).await;
        ctx.set(db_pool.clone()).await;

        // 4. 测试 get 功能
        let fetched_config = ctx.get::<MyConfig>().await.expect("Should find MyConfig");
        let fetched_db = ctx
            .get::<DatabasePool>()
            .await
            .expect("Should find DatabasePool");

        // 5. 断言验证
        assert_eq!(fetched_config, config);
        assert_eq!(fetched_db, db_pool);

        // 6. 测试不存在的情况
        let non_existent = ctx.get::<String>().await;
        assert!(non_existent.is_none());

        // 7. 测试覆盖更新
        let new_config = MyConfig {
            port: 9000,
            enabled: false,
        };
        ctx.set(new_config.clone()).await;
        let updated_config = ctx.get::<MyConfig>().await.unwrap();
        assert_eq!(updated_config.port, 9000);
    }

    #[tokio::test]
    async fn test_global_lifecycle_management() {
        // 1. 初始化 GlobalContext (假设已具备相应构造函数)
        // 这里需要根据你的实际代码构造，只需确保 exits 字段已初始化
        let addr = "127.0.0.1:8080".parse().unwrap();
        let globals = GlobalContext::new(addr, None);

        // 2. 模拟 TCP 服务启动并注册
        let tcp_token = CancellationToken::new();
        let tcp_handle = task::spawn(async move {
            // 模拟 TCP 主循环
            tokio::time::sleep(Duration::from_secs(10)).await;
        });
        globals
            .add_exit("tcp", tcp_token.clone(), tcp_handle.abort_handle())
            .await;

        // 3. 模拟 UDP 服务启动并注册
        let udp_token = CancellationToken::new();
        let udp_handle = task::spawn(async move {
            // 模拟 UDP 处理
            tokio::time::sleep(Duration::from_secs(10)).await;
        });
        globals
            .add_exit("udp", udp_token.clone(), udp_handle.abort_handle())
            .await;

        globals
            .add_exit("tcp", tcp_token.clone(), tcp_handle.abort_handle())
            .await;

        // 4. 验证服务是否已在列表中
        let mut active_services = globals.get_exits().await;
        active_services.sort(); // 排序以便断言
        assert_eq!(active_services.len(), 2);
        assert_eq!(active_services[0], "tcp");
        assert_eq!(active_services[1], "udp");

        // 5. 模拟派生连接 (Child Token)
        let conn_token = tcp_token.child_token();
        let conn_alive = Arc::new(tokio::sync::Mutex::new(true));
        let conn_alive_clone = conn_alive.clone();

        task::spawn(async move {
            tokio::select! {
                _ = conn_token.cancelled() => {
                    let mut alive = conn_alive_clone.lock().await;
                    *alive = false;
                }
                _ = tokio::time::sleep(Duration::from_secs(5)) => {}
            }
        });

        // 6. 执行一键关停
        println!("🚀 Triggering shutdown_all...");
        globals.shutdown_all().await;

        // 7. 验证结果
        // A. 验证服务列表是否清空
        let final_services = globals.get_exits().await;
        assert!(
            final_services.is_empty(),
            "Exits map should be empty after shutdown"
        );

        // B. 验证主服务 Token 是否已取消
        assert!(tcp_token.is_cancelled(), "TCP token should be cancelled");
        assert!(udp_token.is_cancelled(), "UDP token should be cancelled");

        // C. 验证派生的子连接是否因联动而关闭
        // 给一点点时间让协程完成状态切换
        tokio::time::sleep(Duration::from_millis(50)).await;
        let is_alive = *conn_alive.lock().await;
        assert!(
            !is_alive,
            "Child connection should be closed by parent token cancellation"
        );

        println!("✅ Global lifecycle test passed!");
    }
}
