use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::task::{Context as TaskContext, Poll};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tokio::io::{AsyncReadExt, AsyncWrite};
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
    HeartbeatManager::new(Node::from_system(8080, vec![0xABu8; 32], 1))
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

#[test]
fn heartbeat_config_default_matches_new() {
    let default = HeartbeatConfig::default();
    let new = HeartbeatConfig::new();

    assert_eq!(default.interval_secs, new.interval_secs);
    assert_eq!(default.timeout_secs, new.timeout_secs);
    assert!(default.on_timeout.is_none());
    assert!(default.on_latency.is_none());
}

#[tokio::test]
async fn handle_ping_bad_data_errors() {
    let manager = make_manager();
    let peer = peer_addr();
    let global = Arc::new(GlobalContext::new(peer, None));
    let ctx = Arc::new(Mutex::new(Context::new(None, None, global, peer)));

    let result = manager.handle_ping(ctx, b"not-a-ping", peer).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn handle_ping_writer_error_propagates() {
    let manager = make_manager();
    let peer = peer_addr();
    let global = Arc::new(GlobalContext::new(peer, None));
    let ctx = Arc::new(Mutex::new(Context::new(
        None,
        Some(Box::new(FailingWriter)),
        global,
        peer,
    )));

    let ping = PingCommand::new();
    let result = manager.handle_ping(ctx, &ping.encode(), peer).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn handle_pong_bad_data_errors() {
    let manager = make_manager();
    let peer = peer_addr();

    let result = manager.handle_pong(b"not-a-pong", peer).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn handle_pong_twice_updates_average_latency() {
    let manager = make_manager();
    let peer = peer_addr();
    manager.set_connection_state(peer, 0, 0).await;

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let first = PongCommand::new(now.saturating_sub(10), None);
    let l1 = manager.handle_pong(&first.encode(), peer).await.unwrap();
    assert!(l1 >= 10);

    let second = PongCommand::new(now.saturating_sub(20), None);
    let l2 = manager.handle_pong(&second.encode(), peer).await.unwrap();
    assert!(l2 >= 20);

    // latency_ns = latency * 1000; average starts at 0 then (old + new) / 2.
    let first_avg = l1 * 1000 / 2;
    let second_avg = (first_avg + l2 * 1000) / 2;
    assert_eq!(manager.get_latency(peer).await, Some(second_avg));
}

#[tokio::test]
async fn check_timeout_unknown_peer_is_false() {
    let manager = make_manager();
    assert!(!manager.check_timeout(peer_addr()).await);
}

#[tokio::test]
async fn check_timeout_below_threshold_is_false() {
    let manager = make_manager();
    let peer = peer_addr();
    manager.set_connection_state(peer, 1, 0).await;

    assert!(!manager.check_timeout(peer).await);
}

#[tokio::test]
async fn check_timeout_threshold_without_callback_is_true() {
    let manager = make_manager();
    let peer = peer_addr();
    manager.set_connection_state(peer, 2, 0).await;

    assert!(manager.check_timeout(peer).await);
}

#[tokio::test]
async fn start_server_heartbeat_success_branch_sends_ping_and_removes_on_cancel() {
    let node = Node::from_system(8080, vec![0x01u8; 32], 1);
    let manager = HeartbeatManager::new(node).with_config(HeartbeatConfig::new().with_interval(1));
    let peer = peer_addr();

    let (mut resp_reader, writer_half) = tokio::io::duplex(1024);
    let global = Arc::new(GlobalContext::new(peer, None));
    let ctx = Arc::new(Mutex::new(Context::new(
        None,
        Some(Box::new(writer_half)),
        global,
        peer,
    )));
    let token = CancellationToken::new();

    manager
        .start_server_heartbeat(ctx, peer, token.clone())
        .await;

    // First interval tick fires immediately and the ping write succeeds.
    let mut len_buf = [0u8; 4];
    tokio::time::timeout(Duration::from_secs(3), async {
        resp_reader.read_exact(&mut len_buf).await.unwrap();
    })
    .await
    .expect("a ping frame should be written");
    let len = u32::from_le_bytes(len_buf) as usize;
    let mut buf = vec![0u8; len];
    resp_reader.read_exact(&mut buf).await.unwrap();
    let ping = PingCommand::decode(&buf).unwrap();
    assert!(ping.timestamp > 0);

    token.cancel();
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert!(manager.get_latency(peer).await.is_none());
}

#[tokio::test]
async fn start_server_heartbeat_no_writer_triggers_timeout_callback() {
    let fired = Arc::new(AtomicU32::new(0));
    let f = fired.clone();
    let node = Node::from_system(8080, vec![0x02u8; 32], 1);
    let config = HeartbeatConfig::new()
        .with_interval(1)
        .on_timeout(move |_addr| {
            f.fetch_add(1, Ordering::SeqCst);
        });
    let manager = HeartbeatManager::new(node).with_config(config);
    let peer = peer_addr();

    let global = Arc::new(GlobalContext::new(peer, None));
    let ctx = Arc::new(Mutex::new(Context::new(None, None, global, peer)));
    let token = CancellationToken::new();

    manager
        .start_server_heartbeat(ctx, peer, token.clone())
        .await;

    tokio::time::timeout(Duration::from_secs(6), async {
        while fired.load(Ordering::SeqCst) < 2 {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("timeout callback should fire on repeated missing writer");

    token.cancel();
}

#[tokio::test]
async fn remove_connection_unknown_is_noop() {
    let manager = make_manager();
    manager.remove_connection(&peer_addr()).await;
    assert!(manager.get_latency(peer_addr()).await.is_none());
}

#[tokio::test]
async fn set_connection_state_overwrites_previous() {
    let manager = make_manager();
    let peer = peer_addr();
    manager.set_connection_state(peer, 0, 100).await;
    manager.set_connection_state(peer, 5, 200).await;

    assert_eq!(manager.get_latency(peer).await, Some(200));
    assert!(manager.check_timeout(peer).await);
}

#[tokio::test]
async fn heartbeat_manager_with_session_keys_uses_nonce() {
    use aex::crypto::session_key_manager::PairedSessionKey;

    let node = Node::from_system(8080, vec![0x03u8; 32], 1);
    let keys = Arc::new(Mutex::new(PairedSessionKey::new(32)));
    let manager = HeartbeatManager::new(node).with_session_keys(keys);

    assert!(manager.session_keys.is_some());
    let ping = manager.create_ping();
    assert!(ping.nonce.is_some());
}
