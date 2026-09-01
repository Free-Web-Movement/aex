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

#[tokio::test]
async fn oversized_frame_header_does_not_crash_relay() {
    use tokio::io::AsyncWriteExt;
    use tokio::net::TcpStream;
    let server = start_relay().await;
    let relay_addr = server.addr;

    // 直接向中继发一个帧头：魔数 + 超限长度前缀。
    // 这曾导致 512MB 中继节点读取异常帧头后分配 1.3GB 崩溃。
    // 修复后应安全拒绝该帧，中继继续存活。
    let mut stream = TcpStream::connect(relay_addr)
        .await
        .expect("connect to relay");
    let mut head = Vec::new();
    head.extend_from_slice(aex::nat::types::NAT_MAGIC);
    // 超限长度（> NAT_MAX_FRAME_BODY）
    let oversize = (aex::nat::types::NAT_MAX_FRAME_BODY as u32) + 1;
    head.extend_from_slice(&oversize.to_le_bytes());
    stream.write_all(&head).await.expect("write malformed header");

    // 稍等，让中继处理异常帧；随后一个正常节点仍能注册，证明中继未崩溃。
    tokio::time::sleep(Duration::from_millis(300)).await;
    let ok = start_client("node_after_oversize", relay_addr).await;
    assert_eq!(ok.state().await, TunnelState::Ready);
    assert!(server.peer_public_addr("node_after_oversize").is_some());
}


#[tokio::test]
async fn open_channel_relays_byte_stream_between_peers() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let server = start_relay().await;
    let relay_addr = server.addr;

    let a = start_client("node_a", relay_addr).await;
    let b = start_client("node_b", relay_addr).await;

    // A、B 各自打开到对方的字节通道。
    let mut a_ch = a.clone().open_channel("node_b").await;
    let mut b_ch = b.clone().open_channel("node_a").await;

    // A 写 → 中继 → B 的通道读到。
    a_ch.write_all(b"hello-b").await.expect("a write");
    let mut buf = [0u8; 7];
    tokio::time::timeout(Duration::from_secs(3), b_ch.read_exact(&mut buf))
        .await
        .expect("b read timeout")
        .expect("b read");
    assert_eq!(&buf, b"hello-b");

    // B 写 → 中继 → A 的通道读到。
    b_ch.write_all(b"hi-a").await.expect("b write");
    let mut buf2 = [0u8; 4];
    tokio::time::timeout(Duration::from_secs(3), a_ch.read_exact(&mut buf2))
        .await
        .expect("a read timeout")
        .expect("a read");
    assert_eq!(&buf2, b"hi-a");
}

#[tokio::test]
async fn auto_connect_delivers_peer_stream_to_upper_layer() {
    let server = start_relay().await;
    let relay_addr = server.addr;

    // B 先注册并设置流对象消费者。
    let b = start_client("node_b", relay_addr).await;
    let (stream_tx, mut stream_rx) =
        tokio::sync::mpsc::unbounded_channel::<aex::nat::PeerStreamEstablished>();
    b.set_stream_channel(stream_tx).await;

    // A 后注册：中继广播 peers，B 自动发现 A 并建立流对象。
    let a = start_client("node_a", relay_addr).await;

    let est = tokio::time::timeout(Duration::from_secs(3), stream_rx.recv())
        .await
        .expect("B 应收到 A 的流对象")
        .expect("channel closed");
    assert_eq!(est.peer, "node_a");

    // 用该流对象双向收发：A 写入 → B 的流读到。
    let mut b_stream = est.stream;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    a.send_to("node_b", b"ping-b".to_vec()).await.unwrap();
    let mut buf = [0u8; 6];
    tokio::time::timeout(Duration::from_secs(3), b_stream.read_exact(&mut buf))
        .await
        .expect("read timeout")
        .expect("read");
    assert_eq!(&buf, b"ping-b");

    // B 经流对象写 → A 经 send_to 收到（A 用传统 data_channel 接收验证）。
    let (tx_a, mut rx_a) = tokio::sync::mpsc::unbounded_channel();
    a.set_data_channel(tx_a).await;
    b_stream.write_all(b"pong-a").await.unwrap();
    let data = tokio::time::timeout(Duration::from_secs(3), rx_a.recv())
        .await
        .expect("A 应收到")
        .expect("channel closed");
    assert_eq!(data.from, "node_b");
    assert_eq!(data.payload, b"pong-a".to_vec());
}
