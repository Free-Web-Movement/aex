//! 打洞信令测试：两内网节点经中继交换公网映射地址。
//!
//! 验证 PunchRequest → 中继双向 PunchHint（双方拿到对端公网映射地址）+ 双发
//! PunchStart 的信令闭环。TCP 打洞隧道本身在 nat_punch_test.rs 验证。

use aex::nat::{NatRelayServer, NatTunnelClient};
use std::net::SocketAddr;
use std::time::Duration;

async fn start_relay() -> std::sync::Arc<NatRelayServer> {
    let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
    let server = std::sync::Arc::new(
        NatRelayServer::bind(addr).await.expect("relay bind failed"),
    );
    let runner = server.clone();
    tokio::spawn(async move {
        let _ = runner.run().await;
    });
    server
}

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
async fn punch_request_exchanges_peer_addresses_bidirectionally() {
    let server = start_relay().await;
    let relay_addr = server.addr;

    let a = start_client("node_a", relay_addr).await;
    let b = start_client("node_b", relay_addr).await;

    // A 发起打洞请求。
    a.request_punch("node_b").await.expect("punch request failed");

    // 中继应完成双向地址交换：A、B 都学到对方公网映射地址。
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let a_peer = a.peer_public_addr("node_b").await;
            let b_peer = b.peer_public_addr("node_a").await;
            if a_peer.is_some() && b_peer.is_some() {
                return (a_peer.unwrap(), b_peer.unwrap());
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("timeout waiting for punch hint exchange");

    let a_sees_b = a.peer_public_addr("node_b").await.expect("a missing b addr");
    let b_sees_a = b.peer_public_addr("node_a").await.expect("b missing a addr");

    // 双方学到的是对方的连接源地址（本机模拟下都是 127.0.0.1）。
    assert_eq!(a_sees_b.ip(), b_sees_a.ip());
    assert!(a_sees_b.port() != 0);
    assert!(b_sees_a.port() != 0);
    assert_ne!(a_sees_b, b_sees_a, "映射端口应不同");

    // 双方仍在线（隧道未被破坏）。
    assert_eq!(a.state().await, aex::nat::TunnelState::Ready);
    assert_eq!(b.state().await, aex::nat::TunnelState::Ready);
}

#[tokio::test]
async fn punch_request_to_unregistered_peer_does_not_crash() {
    let server = start_relay().await;
    let relay_addr = server.addr;

    let a = start_client("node_a", relay_addr).await;
    // 请求打洞给不存在的节点：中继找不到目标，仅告警，不崩溃。
    let _ = a.request_punch("ghost").await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(a.state().await, aex::nat::TunnelState::Ready);
}
