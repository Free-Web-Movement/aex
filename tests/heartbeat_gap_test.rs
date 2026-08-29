use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context as TaskContext, Poll};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tokio::io::{AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use aex::connection::commands::{PingCommand, PongCommand};
use aex::connection::context::Context;
use aex::connection::global::GlobalContext;
use aex::connection::heartbeat::{HeartbeatConfig, HeartbeatManager};
use aex::connection::node::Node;

fn peer_addr() -> SocketAddr {
    "127.0.0.1:8080".parse().unwrap()
}

fn make_manager() -> HeartbeatManager {
    HeartbeatManager::new(Node::from_system(8080, vec![0xCCu8; 32], 1))
}

struct FailingWriter;
impl AsyncWrite for FailingWriter {
    fn poll_write(
        self: Pin<&mut Self>,
        _cx: &mut TaskContext<'_>,
        _buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Poll::Ready(Err(std::io::Error::new(
            std::io::ErrorKind::BrokenPipe,
            "failing writer",
        )))
    }
    fn poll_flush(self: Pin<&mut Self>, _cx: &mut TaskContext<'_>) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }
    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut TaskContext<'_>) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

#[tokio::test]
async fn test_handle_ping_writes_pong() {
    let manager = make_manager();
    let peer = peer_addr();

    let (mut resp_reader, writer_half) = tokio::io::duplex(1024);
    let global = Arc::new(GlobalContext::new(peer, None));
    let ctx = Arc::new(Mutex::new(Context::new(
        None,
        Some(Box::new(writer_half)),
        global,
        peer,
    )));

    let ping = PingCommand::new();
    manager.handle_ping(ctx, &ping.encode(), peer).await.unwrap();

    let mut len_buf = [0u8; 4];
    resp_reader.read_exact(&mut len_buf).await.unwrap();
    let len = u32::from_le_bytes(len_buf) as usize;
    let mut buf = vec![0u8; len];
    resp_reader.read_exact(&mut buf).await.unwrap();
    let pong = PongCommand::decode(&buf).unwrap();
    assert_eq!(pong.timestamp, ping.timestamp);
}

#[tokio::test]
async fn test_handle_ping_no_writer_errors() {
    let manager = make_manager();
    let peer = peer_addr();
    let global = Arc::new(GlobalContext::new(peer, None));
    let ctx = Arc::new(Mutex::new(Context::new(None, None, global, peer)));

    let ping = PingCommand::new();
    let result = manager.handle_ping(ctx, &ping.encode(), peer).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_handle_pong_updates_state_and_callback() {
    let called = Arc::new(AtomicBool::new(false));
    let c = called.clone();
    let node = Node::from_system(8080, vec![0xDDu8; 32], 1);
    let config = HeartbeatConfig::new().on_latency(move |_addr, _lat| {
        c.store(true, Ordering::SeqCst);
    });
    let manager = HeartbeatManager::new(node).with_config(config);
    let peer = peer_addr();

    manager.set_connection_state(peer, 0, 0).await;

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let pong = PongCommand::new(now.saturating_sub(5), None);
    let latency = manager.handle_pong(&pong.encode(), peer).await.unwrap();

    assert_eq!(latency, 5);
    assert!(manager.get_latency(peer).await.is_some());
    assert!(called.load(Ordering::SeqCst));
}

#[tokio::test]
async fn test_handle_pong_unknown_peer_returns_ok() {
    let manager = make_manager();
    let peer = peer_addr();

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let pong = PongCommand::new(now, None);
    let latency = manager.handle_pong(&pong.encode(), peer).await.unwrap();
    assert!(latency < 2);
    assert!(manager.get_latency(peer).await.is_none());
}

#[tokio::test]
async fn test_start_server_heartbeat_cancel_removes_connection() {
    let node = Node::from_system(8080, vec![0xEEu8; 32], 1);
    let manager = HeartbeatManager::new(node).with_config(HeartbeatConfig::new().with_interval(1));
    let peer = peer_addr();

    let (_resp, writer_half) = tokio::io::duplex(1024);
    let global = Arc::new(GlobalContext::new(peer, None));
    let ctx = Arc::new(Mutex::new(Context::new(
        None,
        Some(Box::new(writer_half)),
        global,
        peer,
    )));
    let token = CancellationToken::new();

    manager.set_connection_state(peer, 0, 111).await;
    assert_eq!(manager.get_latency(peer).await, Some(111));

    manager.start_server_heartbeat(ctx, peer, token.clone()).await;
    token.cancel();
    tokio::time::sleep(Duration::from_millis(100)).await;

    assert!(manager.get_latency(peer).await.is_none());
}

#[tokio::test]
async fn test_start_server_heartbeat_timeout_callback() {
    let fired = Arc::new(AtomicBool::new(false));
    let f = fired.clone();
    let node = Node::from_system(8080, vec![0xFFu8; 32], 1);
    let config = HeartbeatConfig::new()
        .with_interval(1)
        .on_timeout(move |_addr| {
            f.store(true, Ordering::SeqCst);
        });
    let manager = HeartbeatManager::new(node).with_config(config);
    let peer = peer_addr();

    // 写端必然失败 → 连续 2 次 send 失败触发 on_timeout（约 2 秒）
    let global = Arc::new(GlobalContext::new(peer, None));
    let ctx = Arc::new(Mutex::new(Context::new(
        None,
        Some(Box::new(FailingWriter)),
        global,
        peer,
    )));
    let token = CancellationToken::new();
    manager.start_server_heartbeat(ctx, peer, token.clone()).await;

    tokio::time::timeout(Duration::from_secs(5), async {
        while !fired.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("on_timeout should fire after 2 missed pings");

    token.cancel();
    assert!(fired.load(Ordering::SeqCst));
}
