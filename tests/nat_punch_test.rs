//! TCP 打洞（Simultaneous Open）测试。
//!
//! 模拟两内网节点同时向对方 TCP 打洞，建立互为 server/client 的双向直连。

use aex::nat::{PunchCoordinator, PunchState, PunchTunnel};
use std::net::SocketAddr;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::test]
async fn punch_tunnel_send_recv_roundtrip() {
    // 直接测 PunchTunnel 双向数据收发：一端 accept、一端 connect 建立一条
    // 连接后手动构造 PunchTunnel（模拟打洞成功后的双向连接）。
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let peer_addr = listener.local_addr().unwrap();

    let acceptor = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let local = stream.local_addr().unwrap();
        let (reader, writer) = stream.into_split();
        PunchTunnel { peer_public: local, local_addr: local, reader, writer }
    });
    let dialer = tokio::spawn(async move {
        let stream = tokio::net::TcpStream::connect(peer_addr).await.unwrap();
        let local = stream.local_addr().unwrap();
        let (reader, writer) = stream.into_split();
        PunchTunnel { peer_public: peer_addr, local_addr: local, reader, writer }
    });

    let mut a = dialer.await.unwrap();
    let mut b = acceptor.await.unwrap();

    a.send(b"hello from a").await.unwrap();
    let recv_b = b.recv().await.unwrap();
    assert_eq!(recv_b, b"hello from a".to_vec());

    b.send(b"hello from b").await.unwrap();
    let recv_a = a.recv().await.unwrap();
    assert_eq!(recv_a, b"hello from b".to_vec());
}

#[tokio::test]
async fn punch_coordinator_tracks_state() {
    let coord = PunchCoordinator::new();
    assert_eq!(coord.state().await, PunchState::Idle);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let peer = listener.local_addr().unwrap();

    let c2 = coord.clone();
    let acceptor = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let local = stream.local_addr().unwrap();
        let (reader, writer) = stream.into_split();
        let _ = PunchTunnel { peer_public: local, local_addr: local, reader, writer };
    });
    let bind: SocketAddr = "0.0.0.0:0".parse().unwrap();
    let t = c2.punch(peer, bind).await.expect("punch failed");
    acceptor.await.unwrap();
    assert_eq!(coord.state().await, PunchState::Connected);
    assert_eq!(t.local_addr.port() != 0, true);
}

#[tokio::test]
async fn punch_to_unreachable_peer_fails_with_failed_state() {
    // 打洞到不可达地址（未监听端口）：应失败并进入 Failed 状态（降级触发）。
    let coord = PunchCoordinator::new();
    // 找一个未监听端口。
    let probe = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = probe.local_addr().unwrap();
    drop(probe); // 端口立即释放，无监听者。

    let bind: SocketAddr = "0.0.0.0:0".parse().unwrap();
    let result = coord.punch(addr, bind).await;
    assert!(result.is_err());
    assert_eq!(coord.state().await, PunchState::Failed);
}
