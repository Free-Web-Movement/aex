//! Additional coverage for `aex::p2p::nat::client`.
//!
//! A minimal in-process "fake relay" speaks the NAT wire protocol on
//! 127.0.0.1 so we can deterministically drive every client branch without a
//! real relay: RegisterAck, Peers auto-connect, PunchHint/PunchStart, EOF,
//! timeout, and the channel open/AsyncRead/AsyncWrite surface.

use std::net::SocketAddr;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::mpsc;

use aex::nat::{
    NAT_MAGIC_LEN, NatError, NatFrame, NatFrameType, NatTunnelClient, TunnelState,
    frame_len_from_header,
};

/// A scripted relay: auto-acks Register, forwards every decoded frame to
/// `received`, and injects frames from `inject` back to the client.
struct FakeRelay {
    addr: SocketAddr,
    received: mpsc::UnboundedReceiver<NatFrame>,
    inject: mpsc::UnboundedSender<NatFrame>,
}

async fn spawn_fake_relay() -> FakeRelay {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (recv_tx, recv_rx) = mpsc::unbounded_channel::<NatFrame>();
    let (inject_tx, mut inject_rx) = mpsc::unbounded_channel::<NatFrame>();
    let ack_tx = inject_tx.clone();

    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let (mut reader, mut writer) = stream.into_split();
        let writer_task = tokio::spawn(async move {
            while let Some(f) = inject_rx.recv().await {
                if writer.write_all(&f.encode_wire().unwrap()).await.is_err() {
                    break;
                }
            }
        });
        loop {
            let mut head = [0u8; NAT_MAGIC_LEN + 4];
            if reader.read_exact(&mut head).await.is_err() {
                break;
            }
            let len = match frame_len_from_header(&head) {
                Ok(l) => l,
                Err(_) => break,
            };
            let mut body = vec![0u8; len];
            if reader.read_exact(&mut body).await.is_err() {
                break;
            }
            let frame = match NatFrame::decode(&body) {
                Ok(f) => f,
                Err(_) => break,
            };
            if frame.frame_type == NatFrameType::Register {
                let _ = ack_tx.send(NatFrame::register_ack("127.0.0.1:40000"));
            }
            if recv_tx.send(frame).is_err() {
                break;
            }
        }
        writer_task.abort();
    });

    FakeRelay {
        addr,
        received: recv_rx,
        inject: inject_tx,
    }
}

async fn start_client(node_id: &str, addr: SocketAddr) -> std::sync::Arc<NatTunnelClient> {
    let client = NatTunnelClient::connect(node_id.to_string(), addr)
        .await
        .expect("connect");
    client
        .wait_ready(Duration::from_secs(3))
        .await
        .expect("ready");
    client
}

/// Listener that accepts once, drains the client's register frame, then closes
/// cleanly (a FIN, not a RST) so the client read loop observes EOF.
async fn spawn_eof_relay() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        if let Ok((mut s, _)) = listener.accept().await {
            let mut head = [0u8; NAT_MAGIC_LEN + 4];
            if s.read_exact(&mut head).await.is_ok() {
                if let Ok(len) = frame_len_from_header(&head) {
                    let mut body = vec![0u8; len];
                    let _ = s.read_exact(&mut body).await;
                }
            }
            drop(s);
        }
    });
    addr
}

fn free_port() -> u16 {
    let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    probe.local_addr().unwrap().port()
}

#[tokio::test]
async fn connect_to_unreachable_relay_errors() {
    let probe = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = probe.local_addr().unwrap();
    drop(probe);
    let res = NatTunnelClient::connect("node_a".to_string(), addr).await;
    assert!(res.is_err());
}

#[tokio::test]
async fn wait_ready_times_out_when_relay_never_acks() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    // Accept and hold the socket open without ever answering.
    let hold = tokio::spawn(async move {
        let (s, _) = listener.accept().await.unwrap();
        tokio::time::sleep(Duration::from_secs(3)).await;
        drop(s);
    });

    let client = NatTunnelClient::connect("node_a".to_string(), addr)
        .await
        .unwrap();
    let err = client
        .wait_ready(Duration::from_millis(150))
        .await
        .unwrap_err();
    assert!(matches!(err, NatError::Protocol(_)), "got {err:?}");
    hold.abort();
}

#[tokio::test]
async fn wait_ready_called_twice_is_protocol_error() {
    let relay = spawn_fake_relay().await;
    let client = start_client("node_a", relay.addr).await;
    let err = client.wait_ready(Duration::from_secs(1)).await.unwrap_err();
    assert!(matches!(err, NatError::Protocol(_)), "got {err:?}");
}

#[tokio::test]
async fn read_loop_marks_disconnected_on_eof() {
    let addr = spawn_eof_relay().await;
    let client = NatTunnelClient::connect("node_a".to_string(), addr)
        .await
        .unwrap();

    for _ in 0..100 {
        if client.state().await == TunnelState::Disconnected {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(client.state().await, TunnelState::Disconnected);
    assert!(client.public_addr().await.is_none());
}

#[tokio::test]
async fn shutdown_then_writes_fail_remote_closed() {
    let relay = spawn_fake_relay().await;
    let client = start_client("node_a", relay.addr).await;
    assert_eq!(client.state().await, TunnelState::Ready);

    client.shutdown().await;
    assert_eq!(client.state().await, TunnelState::Disconnected);

    assert!(matches!(
        client.send_to("node_b", vec![1]).await,
        Err(NatError::RemoteClosed)
    ));
    assert!(matches!(
        client.send_keepalive().await,
        Err(NatError::RemoteClosed)
    ));
    assert!(matches!(
        client.request_punch("node_b").await,
        Err(NatError::RemoteClosed)
    ));
}

#[tokio::test]
async fn channel_setters_and_tunnel_channel_io() {
    let relay = spawn_fake_relay().await;
    let client = start_client("node_a", relay.addr).await;

    let (punch_tx, _punch_rx) = mpsc::unbounded_channel();
    client.set_punch_channel(punch_tx).await;
    let (stream_tx, _stream_rx) = mpsc::unbounded_channel();
    client.set_stream_channel(stream_tx).await;
    client.set_punch_local_port(Some(0)).await;

    // Exercise AsyncRead/AsyncWrite on a per-peer channel.
    let mut ch = client.clone().open_channel("peer_x").await;
    ch.write_all(b"abc").await.unwrap();
    ch.flush().await.unwrap();
    ch.shutdown().await.unwrap();

    // A Data frame from an unopened peer with no data_channel set is dropped.
    relay
        .inject
        .send(NatFrame::data("stranger", "node_a", b"ignored".to_vec()))
        .unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(client.state().await, TunnelState::Ready);
}

#[tokio::test]
async fn punch_start_without_hint_is_ignored() {
    let relay = spawn_fake_relay().await;
    let client = start_client("node_a", relay.addr).await;
    let (punch_tx, mut punch_rx) = mpsc::unbounded_channel();
    client.set_punch_channel(punch_tx).await;

    // PunchStart without a preceding PunchHint → no peer address, skip.
    relay
        .inject
        .send(NatFrame::punch_start("node_a", "node_b"))
        .unwrap();
    // An unrelated frame type exercises the reader's catch-all arm.
    relay
        .inject
        .send(NatFrame::keep_alive_ack("relay"))
        .unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;

    assert!(punch_rx.try_recv().is_err());
    assert_eq!(client.state().await, TunnelState::Ready);
}

#[tokio::test]
async fn punch_succeeds_against_a_local_peer() {
    let mut relay = spawn_fake_relay().await;
    let client = start_client("node_a", relay.addr).await;

    let (punch_tx, mut punch_rx) = mpsc::unbounded_channel();
    client.set_punch_channel(punch_tx).await;
    client.set_punch_local_port(Some(free_port())).await;

    // Local listener the punch will actually connect to.
    let ctrl = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let ctrl_addr = ctrl.local_addr().unwrap();
    let acceptor = tokio::spawn(async move {
        if let Ok((s, _)) = ctrl.accept().await {
            tokio::time::sleep(Duration::from_secs(5)).await;
            drop(s);
        }
    });

    client.request_punch("node_b").await.unwrap();
    loop {
        let f = tokio::time::timeout(Duration::from_secs(3), relay.received.recv())
            .await
            .expect("relay recv timeout")
            .expect("relay closed");
        if f.frame_type == NatFrameType::PunchRequest {
            break;
        }
    }

    // Relay hands A the (real, reachable) peer address then tells it to punch.
    relay
        .inject
        .send(NatFrame::punch_hint("node_b", &ctrl_addr.to_string()))
        .unwrap();
    relay
        .inject
        .send(NatFrame::punch_start("node_a", "node_b"))
        .unwrap();

    let est = tokio::time::timeout(Duration::from_secs(6), punch_rx.recv())
        .await
        .expect("punch notification timeout")
        .expect("punch channel closed");
    assert_eq!(est.peer, "node_b");
    assert_eq!(est.peer_public, ctrl_addr);

    acceptor.abort();
}

#[tokio::test]
async fn peers_frame_autoconnects_and_skips_duplicates() {
    let relay = spawn_fake_relay().await;
    let client = start_client("node_a", relay.addr).await;

    let (stream_tx, mut stream_rx) = mpsc::unbounded_channel();
    client.set_stream_channel(stream_tx).await;

    // Empty id and self are skipped; node_b is opened.
    relay
        .inject
        .send(NatFrame::peers(
            "relay",
            &[
                (String::new(), "x".to_string()),
                ("node_a".to_string(), "self".to_string()),
                ("node_b".to_string(), "1.2.3.4:5".to_string()),
            ],
        ))
        .unwrap();

    let est = tokio::time::timeout(Duration::from_secs(3), stream_rx.recv())
        .await
        .expect("stream timeout")
        .expect("stream closed");
    assert_eq!(est.peer, "node_b");

    // Re-broadcasting the same peer must be skipped (channel already exists).
    relay
        .inject
        .send(NatFrame::peers(
            "relay",
            &[("node_b".to_string(), "1.2.3.4:5".to_string())],
        ))
        .unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(stream_rx.try_recv().is_err());
}
