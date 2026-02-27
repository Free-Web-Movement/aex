#[cfg(test)]
mod tests {
    
    use std::{net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr}, sync::atomic::Ordering};
    use aex::connection::{node::Node, protocol::Protocol, types::{ConnectionEntry, NetworkScope}};
    use tokio::net::{TcpListener, tcp::OwnedWriteHalf};
    use tokio_util::sync::CancellationToken;

    // 辅助函数：快速创建一个 mock 的 OwnedWriteHalf
    async fn mock_writer() -> OwnedWriteHalf {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let _client = tokio::net::TcpStream::connect(addr).await.unwrap();
        let (server_stream, _) = listener.accept().await.unwrap();
        let (_, writer) = server_stream.into_split();
        writer
    }

    // --- 1. NetworkScope 测试 (覆盖 IPv4/v6 各种分类) ---
    #[test]
    fn test_network_scope_logic() {
        // IPv4 Intranet
        assert_eq!(NetworkScope::from_ip(&IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))), NetworkScope::Intranet);
        assert_eq!(NetworkScope::from_ip(&IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1))), NetworkScope::Intranet);
        assert_eq!(NetworkScope::from_ip(&IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1))), NetworkScope::Intranet);
        assert_eq!(NetworkScope::from_ip(&IpAddr::V4(Ipv4Addr::new(169, 254, 1, 1))), NetworkScope::Intranet);
        
        // IPv4 Extranet
        assert_eq!(NetworkScope::from_ip(&IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8))), NetworkScope::Extranet);
        assert_eq!(NetworkScope::from_ip(&IpAddr::V4(Ipv4Addr::new(114, 114, 114, 114))), NetworkScope::Extranet);

        // IPv6 Intranet
        assert_eq!(NetworkScope::from_ip(&IpAddr::V6(Ipv6Addr::LOCALHOST)), NetworkScope::Intranet);
        assert_eq!(NetworkScope::from_ip(&IpAddr::V6(Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 1))), NetworkScope::Intranet); // Link-local
        assert_eq!(NetworkScope::from_ip(&IpAddr::V6(Ipv6Addr::new(0xfc00, 0, 0, 0, 0, 0, 0, 1))), NetworkScope::Intranet); // ULA

        // IPv6 Extranet
        assert_eq!(NetworkScope::from_ip(&IpAddr::V6(Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 1))), NetworkScope::Extranet);
    }

// --- 2. ConnectionEntry 逻辑测试 ---
    #[tokio::test]
    async fn test_connection_entry_lifecycle() {
        let addr: SocketAddr = "127.0.0.1:8080".parse().unwrap();
        let writer = mock_writer().await;
        let token = CancellationToken::new();
        
        // 创建一个真正的任务以获取 AbortHandle
        let handle = tokio::spawn(async {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }).abort_handle();
        
        // 1. 测试 new_empty_node (初始状态 node 应该是 None)
        let entry = ConnectionEntry::new_empty_node(addr, writer, handle, token);

        // 验证初始化时间戳
        assert!(entry.uptime_secs() <= 1);
        // 验证初始 node 锁内容为 None
        assert_eq!(entry.get_peer_id().await, None);

        // 2. 构造一个完整的 Node 实例
        // 使用之前定义的 from_addr 快捷方式，它会自动填充默认 ID, version 和 scope
        let node_id = b"fixed_node_id_32_bytes__________".to_vec();
        let mock_node = Node::from_addr(
            "192.168.1.100:9000".parse().unwrap(),
            Some(3),           // version
            Some(node_id.clone()) // id
        );

        // 3. 测试 update_node (验证异步写锁)
        entry.update_node(mock_node.clone()).await;

        // 4. 测试 get_peer_id (验证异步读锁与内容提取)
        let peer_id = entry.get_peer_id().await;
        assert!(peer_id.is_some());
        assert_eq!(peer_id.unwrap(), node_id);

        // 5. 额外覆盖：再次读取验证 Node 内容的一致性
        {
            let node_lock = entry.node.read().await;
            let node_ref = node_lock.as_ref().unwrap();
            assert_eq!(node_ref.port, 9000);
            assert_eq!(node_ref.version, 3);
            assert!(node_ref.protocols.contains(&Protocol::Tcp));
        }
    }

    #[tokio::test]
    async fn test_is_deactivated_logic() {
        let addr: SocketAddr = "127.0.0.1:8080".parse().unwrap();
        let writer = mock_writer().await;
        let entry = ConnectionEntry::new_empty_node(
            addr, 
            writer, 
            tokio::spawn(async {}).abort_handle(), 
            CancellationToken::new()
        );

        let now = entry.connected_at;

        // 路径 1: 正常状态 (未超时，未到寿命)
        assert!(!entry.is_deactivated(now + 10, 30, 100));

        // 路径 2: 寿命超限 (max_lifetime_secs)
        assert!(entry.is_deactivated(now + 101, 30, 100));

        // 路径 3: 活跃度超限 (timeout_secs)
        // 先手动模拟更新一下 last_seen
        entry.last_seen.store(now + 10, Ordering::SeqCst);
        // 当前时间 now + 50，距离上次活跃过去了 40s，超过了 timeout(30)
        assert!(entry.is_deactivated(now + 50, 30, 1000));
        
        // 路径 4: 时间倒流或边界 (saturating_sub 保护)
        assert!(!entry.is_deactivated(now - 100, 30, 100));
    }

    #[tokio::test]
    async fn test_drop_aborts_handle() {
        let addr: SocketAddr = "127.0.0.1:8080".parse().unwrap();
        let writer = mock_writer().await;
        
        let (tx, mut rx) = tokio::sync::mpsc::channel(1);
        
        let handle = tokio::spawn(async move {
            // 永远等待直到被 abort
            tokio::time::sleep(std::time::Duration::from_secs(100)).await;
            let _ = tx.send(()).await;
        });

        {
            let abort_handle = handle.abort_handle();
            let _entry = ConnectionEntry::new_empty_node(
                addr, 
                writer, 
                abort_handle, 
                CancellationToken::new()
            );
            // entry 在这里离开作用域，触发 Drop
        }

        // 验证 handle 是否真的被 abort 了
        let join_result = handle.await;
        assert!(join_result.is_err());
        assert!(join_result.unwrap_err().is_cancelled());
        assert!(rx.try_recv().is_err());
    }
}