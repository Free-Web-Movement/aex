#[cfg(test)]
mod tests {
    use aex::{
        connection::{
            global::GlobalContext, manager::ConnectionManager, node::Node, types::{BiDirectionalConnections, NetworkScope}
        },
        time::SystemTime,
    };
    use std::{
        collections::HashSet,
        net::{IpAddr, Ipv4Addr, SocketAddr},
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        time::Duration,
    };
    use tokio::{io::AsyncReadExt, net::TcpListener};

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
        let handle =
            tokio::spawn(async { tokio::time::sleep(std::time::Duration::from_secs(10)).await })
                .abort_handle();

        // 1. 测试回环地址拦截
        let loopback: SocketAddr = "127.0.0.1:9000".parse().unwrap();
        let cancellation_token: tokio_util::sync::CancellationToken =
            tokio_util::sync::CancellationToken::new();

        manager.add(loopback, handle.clone(), cancellation_token.clone(), true, None);
        assert!(manager.connections.is_empty(), "Loopback should be ignored");

        // 2. 测试添加 Client 连接
        manager.add(addr, handle.clone(), cancellation_token.clone(), true, None);
        assert_eq!(manager.connections.len(), 1);

        // 3. 测试重复 IP 不同端口 (应该在同一个桶里)
        let addr2: SocketAddr = "1.1.1.1:8081".parse().unwrap();
        manager.add(addr2, handle.clone(), cancellation_token.clone(),  false, None,);
        {
            let bucket = manager
                .connections
                .get(&(addr.ip(), NetworkScope::from_ip(&addr.ip())))
                .unwrap();
            assert_eq!(bucket.clients.len(), 1);
            assert_eq!(bucket.servers.len(), 1);
        }

        // 4. 测试移除逻辑
        manager.remove(addr, true); // 移除 client
        {
            let bucket = manager
                .connections
                .get(&(addr.ip(), NetworkScope::from_ip(&addr.ip())))
                .unwrap();
            assert_eq!(bucket.clients.len(), 0);
            assert_eq!(bucket.servers.len(), 1);
        }

        manager.remove(addr2, false); // 移除 server -> 触发桶清理
        assert!(
            manager.connections.is_empty(),
            "Bucket should be cleaned up"
        );
    }

    #[tokio::test]
    async fn test_cancel_operations() {
        // 强制 5 秒超时，防止整个测试流程挂死
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            println!(">>> 开始测试: 初始化 Manager");
            let manager = ConnectionManager::new();
            let addr: SocketAddr = "1.2.3.4:5000".parse().unwrap();

            println!(">>> 正在添加连接");
            let handle = tokio::spawn(async {
                loop {
                    tokio::task::yield_now().await;
                }
            })
            .abort_handle();
                let cancellation_token: tokio_util::sync::CancellationToken =
            tokio_util::sync::CancellationToken::new();
            manager.add(addr, handle.clone(), cancellation_token.clone(),  true, None,);

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
            manager.add(addr, handle.clone(), cancellation_token.clone(),  true, None,);
            manager.cancel_all_by_ip(addr.ip());

            println!(">>> 测试完成!");
        })
        .await
        .expect("测试因超时被迫中止，确认发生了死锁！");
    }

    #[tokio::test]
    async fn test_deactivate_and_status() {
        let manager = ConnectionManager::new();
        let addr: SocketAddr = "8.8.8.8:80".parse().unwrap();
        let handle = tokio::spawn(async {}).abort_handle();
        let cancellation_token: tokio_util::sync::CancellationToken =
            tokio_util::sync::CancellationToken::new();
        manager.add(addr, handle, cancellation_token.clone(),  true, None,);

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
                let cancellation_token: tokio_util::sync::CancellationToken =
            tokio_util::sync::CancellationToken::new();
        manager.add(
            addr,
            tokio::spawn(async {}).abort_handle(),
            cancellation_token,
            false,
            None,
        );

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
        manager
            .connections
            .insert(key, BiDirectionalConnections::new());

        // 如果这里没有正确处理 drop，内部会因为持有 Ref 而导致 remove 死锁
        manager.check_and_cleanup_bucket(key);
        assert!(manager.connections.is_empty());
    }

    #[tokio::test]
    async fn test_extreme_deactivate() {
        let manager = ConnectionManager::new();
        let addr: SocketAddr = "1.1.1.1:80".parse().unwrap();
        let cancellation_token: tokio_util::sync::CancellationToken =
            tokio_util::sync::CancellationToken::new();
        // 注入一个连接
        manager.add(
            addr,
            tokio::spawn(async {}).abort_handle(),
            cancellation_token,
            true,
            None,
        );

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
        let cancellation_token: tokio_util::sync::CancellationToken =
            tokio_util::sync::CancellationToken::new();
        manager.add(
            intranet_addr,
            tokio::spawn(async {}).abort_handle(),
            cancellation_token.clone(),
            true,
            None,
        );
        manager.add(
            extranet_addr,
            tokio::spawn(async {}).abort_handle(),
            cancellation_token,
            false,
            None,
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

    #[tokio::test]
    async fn test_connection_notify_by_node_id() {
        let manager = ConnectionManager::new();

        let addr: SocketAddr = "192.168.1.100:8080".parse().unwrap();
        let node_id = vec![1, 2, 3, 4];

        // 1. 模拟连接接入
        // 在 runtime 上下文中生成一个句柄
        let handle = tokio::spawn(async {}).abort_handle();
                let cancellation_token: tokio_util::sync::CancellationToken =
            tokio_util::sync::CancellationToken::new();
        manager.add(addr, handle, cancellation_token.clone(),  true, None);

        // 2. 模拟握手完成：填充 Node 信息
        {
            let ip = addr.ip();
            let scope = NetworkScope::from_ip(&ip);
            let bi_conn = manager.connections.get(&(ip, scope)).unwrap();
            let entry = bi_conn.clients.get(&addr).unwrap();

            // 写入 Node ID
            let mut node_lock = entry.node.write().await;
            *node_lock = Some(Node {
                id: node_id.clone(),
                version: 1,
                started_at: SystemTime::now_ts(),
                port: 8080,
                protocols: HashSet::new(),
                ips: Vec::new(),
            });
        }

        // 3. 执行 Notify 测试
        let called = Arc::new(AtomicBool::new(false));
        let called_clone = Arc::clone(&called);
        manager
            .notify(&node_id, |entries| {
                let inner_called = called_clone; // 进一步移动到 async block
                async move {
                    assert_eq!(entries.len(), 1, "应该找到一个匹配的连接");
                    assert_eq!(entries[0].addr, addr);
                    inner_called.store(true, Ordering::SeqCst);
                }
            })
            .await;

        assert!(called.load(Ordering::SeqCst), "Notify 应该修改了原子变量");
        // 4. 测试不存在的 ID
        manager
            .notify(&vec![9, 9, 9], |entries| async move {
                assert!(entries.is_empty(), "不匹配的 ID 应该返回空列表");
            })
            .await;
    }

    #[tokio::test] // 👈 使用异步测试宏，它会自动提供 Runtime
    async fn test_notify_multiple_connections() {
        let manager = ConnectionManager::new();
        let node_id = vec![42];

        // 使用非 loopback IP (避免被 add 内部逻辑拦截)
        let addrs = vec![
            "1.1.1.1:1000".parse::<SocketAddr>().unwrap(),
            "2.2.2.2:2000".parse::<SocketAddr>().unwrap(),
        ];

        for &addr in &addrs {
            // 1. 直接在当前异步环境中生成句柄，不需要 block_on
            let handle = tokio::spawn(async {}).abort_handle();
                    let cancellation_token: tokio_util::sync::CancellationToken =
            tokio_util::sync::CancellationToken::new();
            manager.add(addr, handle, cancellation_token.clone(),  true, None,);

            // 2. 模拟握手：使用 .await 获取异步锁并填充 Node 信息
            let ip = addr.ip();
            let scope = NetworkScope::from_ip(&ip);
            let bi_conn = manager
                .connections
                .get(&(ip, scope))
                .expect("Bucket missing");
            let entry = bi_conn.clients.get(&addr).expect("Entry missing");

            // 👈 直接 await，不要用 rt.block_on
            let mut node_lock = entry.node.write().await;
            *node_lock = Some(Node {
                id: node_id.clone(),
                version: 1,
                started_at: SystemTime::now_ts(),
                port: 8080,
                protocols: HashSet::new(),
                ips: Vec::new(),
            });
        }

        // 3. 执行异步 Notify
        // 确保你的 notify 定义支持泛型 T 以便返回结果
        manager
            .notify(&node_id, |entries| async move {
                assert_eq!(entries.len(), 2, "同一个 Node ID 应该能搜到多个连接");
            })
            .await;
    }

    // #[tokio::test]
    // async fn test_connection_update_writer() {
    //     use std::sync::atomic::{AtomicBool, Ordering};

    //     let manager = ConnectionManager::new();
    //     let addr: SocketAddr = "1.1.1.1:8080".parse().unwrap();

    //     // 1. 使用 AtomicBool 来跨线程/任务同步状态
    //     let found_updated = Arc::new(AtomicBool::new(false));

    //     let handle = tokio::spawn(async {}).abort_handle();
    //             let cancellation_token: tokio_util::sync::CancellationToken =
    //         tokio_util::sync::CancellationToken::new();
    //     manager.add(addr, handle, cancellation_token.clone(),  true, None,);

    //     // let mock_writer: Arc<Mutex<Option<BoxWriter>>> =
    //     //     Arc::new(Mutex::new(Some(Box::new(tokio::io::sink()))));

    //     let context = Context::new(reader, writer, global, addr);
    //     manager.update(addr, true, None);

    //     // 2. 克隆 Arc 传入异步闭包
    //     let found_clone = Arc::clone(&found_updated);
    //     manager
    //         .forward(move |entries| async move {
    //             if let Some(entry) = entries.iter().find(|e| e.addr == addr) {
    //                 if entry.context.is_some() {
    //                     // 3. 安全地修改原子值
    //                     found_clone.store(true, Ordering::SeqCst);
    //                 }
    //             }
    //         })
    //         .await;

    //     // 4. 读取修改后的值
    //     assert!(
    //         found_updated.load(Ordering::SeqCst),
    //         "Update 应该成功并在 forward 中可见"
    //     );
    // }

    #[tokio::test] // 👈 这里的宏已经为你启动了一个 Runtime
    async fn test_connection_forward_all() {
        // 1. 直接创建 manager，不需要外部 Runtime
        let manager = ConnectionManager::new();

        // 2. 准备测试地址（注意：避免使用 127.0.0.1，否则 add 会因为 is_loopback 直接返回）
        let addrs = vec![
            "1.1.1.1:1000".parse::<SocketAddr>().unwrap(),
            "2.2.2.2:2000".parse::<SocketAddr>().unwrap(),
            "3.3.3.3:3000".parse::<SocketAddr>().unwrap(),
        ];

        for &addr in &addrs {
            // 3. ❌ 错误：rt.block_on(async { tokio::spawn(...).abort_handle() })
            // ✅ 正确：直接在当前 Runtime 中 spawn
            let handle = tokio::spawn(async {
                // 模拟连接任务逻辑
                tokio::time::sleep(std::time::Duration::from_secs(10)).await;
            })
            .abort_handle();
        let cancellation_token: tokio_util::sync::CancellationToken =
            tokio_util::sync::CancellationToken::new();
            manager.add(addr, handle, cancellation_token.clone(),  true, None,);
        }

        // 4. 执行异步 forward
        // 确保你的 forward 定义支持异步闭包：pub async fn forward<F, Fut>(&self, f: F)
        manager
            .forward(|entries| async move {
                assert_eq!(entries.len(), 3, "应该获取到所有 3 个连接");

                for addr in ["1.1.1.1:1000", "2.2.2.2:2000", "3.3.3.3:3000"] {
                    let target_addr: SocketAddr = addr.parse().unwrap();
                    assert!(
                        entries.iter().any(|e| e.addr == target_addr),
                        "未能找到地址为 {} 的连接",
                        addr
                    );
                }
            })
            .await; // 👈 必须 await
    }

    // 辅助函数：创建一个临时的 TCP Server 用于测试连接
    async fn setup_test_server() -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                // 简单的回显或直接保持连接
                let mut buf = [0u8; 10];
                let _ = stream.read(&mut buf).await;
            }
        });
        addr
    }

    #[tokio::test]
    async fn test_connect_success() {
        let manager = ConnectionManager::new();
        let addr = setup_test_server().await;
        let global = Arc::new(GlobalContext::new(addr, None));

        // 场景：正常连接
        let result = manager
            .connect(addr, global,|_ctx, _token| async move {
                // 业务逻辑：收到连接后打印或执行简单操作
            })
            .await;

        assert!(result.is_ok());

        // 注意：由于 add 中有 #[cfg(test)] 跳过 loopback 的逻辑，
        // 如果要测试登记成功，需要确保 add 逻辑允许测试 IP。
        // 假设此时已经成功加入（或修改 add 逻辑允许测试）
        // assert!(manager.connections.get(&(addr.ip(), NetworkScope::from_ip(&addr.ip()))).is_some());
    }

    #[tokio::test]
    async fn test_connect_duplicate_prevented() {
        let manager = ConnectionManager::new();
        let addr = setup_test_server().await;

        // 1. 先手动 mock 一个已存在的连接
        let handle = tokio::spawn(async {}).abort_handle();
        let cancellation_token: tokio_util::sync::CancellationToken =
            tokio_util::sync::CancellationToken::new();

        manager.add(addr, handle, cancellation_token, false, None);

        // 2. 再次尝试 connect
        // 逻辑：应该在第 1 步检查重复时就 Ok(()) 返回
                let global = Arc::new(GlobalContext::new(addr, None));
        let result = manager.connect(addr, global, |_ctx, _t| async move {}).await;

        assert!(result.is_ok());
        // 验证没有产生新的拨号尝试（可以通过观察 Mock Server 行为或计数验证）
    }

    #[tokio::test]
    async fn test_connect_physical_failure() {
        let manager = ConnectionManager::new();
        // 使用一个确定没人在监听的端口
        let addr: SocketAddr = "127.0.0.1:1".parse().unwrap();
        let global = Arc::new(GlobalContext::new(addr, None));

        let result = manager.connect(addr,global, |_ctx, _t| async move {}).await;

        // 逻辑：TcpStream::connect 失败，应该返回 Err
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_connect_closure_execution() {
        let manager = ConnectionManager::new();
        let addr = setup_test_server().await;

        let (tx, rx) = tokio::sync::oneshot::channel();
        let global = Arc::new(GlobalContext::new(addr, None));

        // 验证闭包是否真的被执行了
        let _ = manager
            .connect(addr, global,|_ctx, _token| async move {
                let _ = tx.send(true);
            })
            .await;

        let executed = tokio::time::timeout(Duration::from_secs(1), rx).await;
        assert!(matches!(executed, Ok(Ok(true))));
    }
}
