#[cfg(test)]
mod tests {
    
    use aex::connection::{
        manager::ConnectionManager, types::{ BiDirectionalConnections, NetworkScope }
    };
    use std::net::{ IpAddr, Ipv4Addr, SocketAddr };

    #[tokio::test]
    async fn test_new_manager() {
        let manager = ConnectionManager::new();
        assert!(manager.connections.is_empty());
        assert!(!manager.cancel_token.is_cancelled());
    }

    #[tokio::test]
    async fn test_add_and_remove_logic() {
        let manager = ConnectionManager::new();
        let addr: SocketAddr = "1.1.1.1:8080".parse().unwrap();

        // 模拟一个异步任务的 AbortHandle
        let handle = tokio
            ::spawn(async { tokio::time::sleep(std::time::Duration::from_secs(10)).await })
            .abort_handle();

        // 1. 测试回环地址拦截
        let loopback: SocketAddr = "127.0.0.1:9000".parse().unwrap();
        manager.add(loopback, handle.clone(), true);
        assert!(manager.connections.is_empty(), "Loopback should be ignored");

        // 2. 测试添加 Client 连接
        manager.add(addr, handle.clone(), true);
        assert_eq!(manager.connections.len(), 1);

        // 3. 测试重复 IP 不同端口 (应该在同一个桶里)
        let addr2: SocketAddr = "1.1.1.1:8081".parse().unwrap();
        manager.add(addr2, handle.clone(), false);
        {
            let bucket = manager.connections
                .get(&(addr.ip(), NetworkScope::from_ip(&addr.ip())))
                .unwrap();
            assert_eq!(bucket.clients.len(), 1);
            assert_eq!(bucket.servers.len(), 1);
        }

        // 4. 测试移除逻辑
        manager.remove(addr, true); // 移除 client
        {
            let bucket = manager.connections
                .get(&(addr.ip(), NetworkScope::from_ip(&addr.ip())))
                .unwrap();
            assert_eq!(bucket.clients.len(), 0);
            assert_eq!(bucket.servers.len(), 1);
        }

        manager.remove(addr2, false); // 移除 server -> 触发桶清理
        assert!(manager.connections.is_empty(), "Bucket should be cleaned up");
    }

    #[tokio::test]
    async fn test_cancel_operations() {
        // 强制 5 秒超时，防止整个测试流程挂死
        tokio::time
            ::timeout(std::time::Duration::from_secs(5), async {
                println!(">>> 开始测试: 初始化 Manager");
                let manager = ConnectionManager::new();
                let addr: SocketAddr = "1.2.3.4:5000".parse().unwrap();

                println!(">>> 正在添加连接");
                let handle = tokio
                    ::spawn(async {
                        loop {
                            tokio::task::yield_now().await;
                        }
                    })
                    .abort_handle();
                manager.add(addr, handle.clone(), true);

                println!(">>> 正在执行 cancel_gracefully");
                assert!(manager.cancel_gracefully(addr));

                println!(">>> 检查 cancel_token 状态");
                {
                    let ip_key = (addr.ip(), NetworkScope::from_ip(&addr.ip()));
                    if let Some(bucket) = manager.connections.get(&ip_key) {
                        if let Some(entry) = bucket.clients.get(&addr) {
                            assert!(entry.cancel_token.is_cancelled());
                        }
                    }
                } // 此处必须释放所有 Ref

                println!(">>> 正在执行 cancel_by_addr (最可能的死锁点)");
                // 如果这里死锁，说明 cancel_by_addr 内部逻辑有问题
                manager.cancel_by_addr(addr);

                println!(">>> 正在执行 cancel_all_by_ip");
                manager.add(addr, handle.clone(), true);
                manager.cancel_all_by_ip(addr.ip());

                println!(">>> 测试完成!");
            }).await
            .expect("测试因超时被迫中止，确认发生了死锁！");
    }

    #[tokio::test]
    async fn test_deactivate_and_status() {
        let manager = ConnectionManager::new();
        let addr: SocketAddr = "8.8.8.8:80".parse().unwrap();
        let handle = tokio::spawn(async {}).abort_handle();

        manager.add(addr, handle, true);

        // 验证状态统计
        let status = manager.status();
        assert_eq!(status.total_ips, 1);
        assert_eq!(status.total_clients, 1);
        assert_eq!(status.total_servers, 0);

        // 强制停用测试：设置极短的超时
        // 假设 entry.is_deactivated 逻辑依赖于最后活跃时间
        // 这里模拟时间流逝或直接调用 deactivate
        manager.deactivate(0, 0); // 应该会清理掉所有连接
        assert!(manager.connections.is_empty());

        // 边界：空管理器状态
        let empty_status = manager.status();
        assert_eq!(empty_status.average_uptime, 0);
    }

    #[tokio::test]
    async fn test_shutdown() {
        let manager = ConnectionManager::new();
        let addr: SocketAddr = "10.0.0.1:443".parse().unwrap();
        manager.add(addr, tokio::spawn(async {}).abort_handle(), false);

        manager.shutdown();

        assert!(manager.cancel_token.is_cancelled());
        assert!(manager.connections.is_empty());
    }

    #[test]
    fn test_cleanup_deadlock_prevention() {
        // 这个测试专门覆盖 check_and_cleanup_bucket 中的 drop(bi_conn)
        let manager = ConnectionManager::new();
        let ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1));
        let scope = NetworkScope::from_ip(&ip);
        let key = (ip, scope);

        // 手动插入一个空桶
        manager.connections.insert(key, BiDirectionalConnections::new());

        // 如果这里没有正确处理 drop，内部会因为持有 Ref 而导致 remove 死锁
        manager.check_and_cleanup_bucket(key);
        assert!(manager.connections.is_empty());
    }

    #[tokio::test]
    async fn test_extreme_deactivate() {
        let manager = ConnectionManager::new();
        let addr: SocketAddr = "1.1.1.1:80".parse().unwrap();

        // 注入一个连接
        manager.add(addr, tokio::spawn(async {}).abort_handle(), true);

        // 覆盖点：1. 仅超时停用 2. 仅最大寿命停用 3. 两者都不满足
        // 模拟 current 很大（未来时间）的情景
        // 注意：如果你的 deactivate 内部直接调用了 SystemTime::now()，
        // 你可以传入超大的 timeout 参数来触发 saturating_sub 的边界。

        manager.deactivate(0, 0); // 覆盖“全部立即清理”路径
        assert!(manager.connections.is_empty());
    }

    #[tokio::test]
    async fn test_network_scope_coverage() {
        let manager = ConnectionManager::new();
        let intranet_addr: SocketAddr = "10.0.0.1:80".parse().unwrap();
        let extranet_addr: SocketAddr = "1.1.1.1:80".parse().unwrap();

        manager.add(
            intranet_addr,
            tokio::spawn(async {}).abort_handle(),
            true
        );
        manager.add(
            extranet_addr,
            tokio::spawn(async {}).abort_handle(),
            false
        );

        let status = manager.status();
        assert!(status.intranet_conns > 0);
        assert!(status.extranet_conns > 0);
        assert_eq!(status.total_ips, 2);
    }

    #[test]
    fn test_status_empty_manager() {
        let manager = ConnectionManager::new();
        let status = manager.status();
        assert_eq!(status.total_ips, 0);
        assert_eq!(status.average_uptime, 0); // 覆盖 conn_count == 0 的分支
    }
}
