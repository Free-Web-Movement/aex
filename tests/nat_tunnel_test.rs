//! NAT 穿透隧道集成测试。
//!
//! 模拟：一个公网中继节点（relay server）+ 多个内网节点（tunnel client），
//! 验证内网节点经公网中继互联、公网映射地址学习、保活与数据转发。

use aex::nat::{NatRelayServer, NatTunnelClient, TunnelState};
use std::net::SocketAddr;
use std::time::Duration;

/// 启动一个公网中继节点，返回 server 句柄（Arc，保持 run 存活）。
async fn start_relay() -> std::sync::Arc<NatRelayServer> {
    let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
    let server = std::sync::Arc::new(
        NatRelayServer::bind(addr)
            .await
            .expect("relay bind failed"),
    );
    let runner = server.clone();
    tokio::spawn(async move {
        let _ = runner.run().await;
    });
    server
}

/// 启动一个内网节点隧道客户端，等待注册就绪。
async fn start_client(node_id: &str, relay_addr: SocketAddr) -> std::sync::Arc<NatTunnelClient> {
    let client = NatTunnelClient::connect(node_id.to_string(), relay_addr)
        .await
        .expect("client connect failed");
    client
        .wait_ready(Duration::from_secs(5))
        .await
        .expect("client register failed");
    client
}

#[tokio::test]
async fn relay_registers_two_clients_and_learns_public_addr() {
    let server = start_relay().await;
    let relay_addr = server.addr;

    let a = start_client("node_a", relay_addr).await;
    let b = start_client("node_b", relay_addr).await;

    // 两个内网节点都成功注册，学习到公网映射地址。
    assert_eq!(server.peer_count(), 2);
    let pa = a.public_addr().await.expect("node_a public addr");
    let pb = b.public_addr().await.expect("node_b public addr");
    assert!(!pa.is_empty());
    assert!(!pb.is_empty());
    assert_eq!(a.state().await, TunnelState::Ready);
    assert_eq!(b.state().await, TunnelState::Ready);

    // 公网中继登记表包含两个节点，且能查到公网映射地址。
    let snapshot = server.peers_snapshot();
    assert_eq!(snapshot.len(), 2);
    assert!(snapshot.iter().any(|p| p.node_id == "node_a"));
    assert!(snapshot.iter().any(|p| p.node_id == "node_b"));
    assert!(server.peer_public_addr("node_a").is_some());
    assert!(server.peer_public_addr("node_b").is_some());
}

#[tokio::test]
async fn two_clients_relay_message_through_public_node() {
    let server = start_relay().await;
    let relay_addr = server.addr;

    let a = start_client("node_a", relay_addr).await;
    let b = start_client("node_b", relay_addr).await;

    // b 注册数据接收通道。
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    b.set_data_channel(tx).await;

    // a 发消息给 b，经公网中继转发。
    a.send_to("node_b", b"hello from a".to_vec())
        .await
        .expect("send failed");

    // b 应收到。
    let data = tokio::time::timeout(Duration::from_secs(3), rx.recv())
        .await
        .expect("b did not receive message")
        .expect("channel closed");
    assert_eq!(data.from, "node_a");
    assert_eq!(data.payload, b"hello from a".to_vec());
}

#[tokio::test]
async fn three_clients_all_interconnect_through_public_node() {
    let server = start_relay().await;
    let relay_addr = server.addr;

    let a = start_client("node_a", relay_addr).await;
    let b = start_client("node_b", relay_addr).await;
    let c = start_client("node_c", relay_addr).await;

    assert_eq!(server.peer_count(), 3);

    // 三对独立收发通道。
    let (tx_b, mut rx_b) = tokio::sync::mpsc::unbounded_channel();
    b.set_data_channel(tx_b).await;
    let (tx_c, mut rx_c) = tokio::sync::mpsc::unbounded_channel();
    c.set_data_channel(tx_c).await;
    let (tx_a, mut rx_a) = tokio::sync::mpsc::unbounded_channel();
    a.set_data_channel(tx_a).await;

    // a -> b, b -> c, c -> a。
    a.send_to("node_b", b"a->b".to_vec()).await.unwrap();
    b.send_to("node_c", b"b->c".to_vec()).await.unwrap();
    c.send_to("node_a", b"c->a".to_vec()).await.unwrap();

    let m1 = tokio::time::timeout(Duration::from_secs(3), rx_b.recv())
        .await
        .expect("b timeout")
        .expect("b channel closed");
    assert_eq!(m1.payload, b"a->b".to_vec());

    let m2 = tokio::time::timeout(Duration::from_secs(3), rx_c.recv())
        .await
        .expect("c timeout")
        .expect("c channel closed");
    assert_eq!(m2.payload, b"b->c".to_vec());

    let m3 = tokio::time::timeout(Duration::from_secs(3), rx_a.recv())
        .await
        .expect("a timeout")
        .expect("a channel closed");
    assert_eq!(m3.payload, b"c->a".to_vec());
}

#[tokio::test]
async fn keepalive_refreshes_tunnel() {
    let server = start_relay().await;
    let relay_addr = server.addr;

    let a = start_client("node_a", relay_addr).await;

    // 发送保活，隧道保持 ready，未被清理。
    a.send_keepalive().await.expect("keepalive failed");
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(a.state().await, TunnelState::Ready);
    assert_eq!(server.peer_count(), 1);

    // 再发数据验证隧道仍可用。
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let b = start_client("node_b", relay_addr).await;
    b.set_data_channel(tx).await;
    a.send_to("node_b", b"still alive".to_vec()).await.unwrap();
    let data = tokio::time::timeout(Duration::from_secs(3), rx.recv())
        .await
        .expect("timeout")
        .expect("channel closed");
    assert_eq!(data.payload, b"still alive".to_vec());
}

#[tokio::test]
async fn relay_to_unknown_peer_fails_but_does_not_crash() {
    let server = start_relay().await;
    let relay_addr = server.addr;

    let a = start_client("node_a", relay_addr).await;

    // 发给不存在的节点：发送成功（本节点发出），但中继找不到目标即丢弃。
    // 不应 panic、隧道仍存活。
    let _ = a.send_to("ghost", b"lost".to_vec()).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(a.state().await, TunnelState::Ready);
    assert_eq!(server.peer_count(), 1);
}

#[tokio::test]
async fn peer_public_addr_queryable_after_registration() {
    let server = start_relay().await;
    let relay_addr = server.addr;

    let a = start_client("node_a", relay_addr).await;
    let _b = start_client("node_b", relay_addr).await;

    // 中继能查到已登记对端的公网映射地址。
    assert!(server.peer_public_addr("node_a").is_some());
    assert!(server.peer_public_addr("node_b").is_some());
    // 未登记节点查不到。
    assert!(server.peer_public_addr("ghost").is_none());
    let _ = a.state().await;
}

#[tokio::test]
async fn shutdown_marks_disconnected() {
    let server = start_relay().await;
    let relay_addr = server.addr;

    let a = start_client("node_a", relay_addr).await;
    assert_eq!(a.state().await, TunnelState::Ready);
    a.shutdown().await;
    assert_eq!(a.state().await, TunnelState::Disconnected);
}

#[tokio::test]
async fn nat_tcp_handler_builds() {
    use aex::nat::nat_tcp_handler;
    let service = aex::nat::NatRelayService::new();
    let handler = nat_tcp_handler(service.clone());
    // Arc<dyn Fn> 可克隆；service 可 clone（共享登记表）。
    let handler2 = handler.clone();
    assert!(std::ptr::eq(handler.as_ref(), handler2.as_ref()));
    let service2 = service.clone();
    assert_eq!(service2.peer_count(), 0);
}
