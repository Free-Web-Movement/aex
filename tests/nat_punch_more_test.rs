//! Additional coverage for `aex::p2p::nat::punch`.
//!
//! Targets the uncovered `PunchTunnel`/`PunchCoordinator` paths:
//! - `recv` oversized length-prefix rejection.
//! - `connect` with an explicit (non-unspecified) local bind, and with a fixed
//!   local port already in use (listener bind failure → connect-slot fallback).
//! - `PunchCoordinator::default`.
//! - successful hole-punch against a local peer.

use std::net::SocketAddr;
use std::time::Duration;

use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};

use aex::nat::{NatError, PunchCoordinator, PunchState, PunchTunnel};

/// Wrap an accepted/connected stream into a `PunchTunnel` for manual I/O.
fn tunnel_from(stream: TcpStream) -> PunchTunnel {
    let peer_public = stream
        .peer_addr()
        .unwrap_or_else(|_| "0.0.0.0:0".parse().unwrap());
    let local_addr = stream
        .local_addr()
        .unwrap_or_else(|_| "0.0.0.0:0".parse().unwrap());
    let (reader, writer) = stream.into_split();
    PunchTunnel {
        peer_public,
        local_addr,
        reader,
        writer,
    }
}

#[tokio::test]
async fn punch_coordinator_default_is_idle() {
    let coord = PunchCoordinator::default();
    assert_eq!(coord.state().await, PunchState::Idle);
}

#[tokio::test]
async fn punch_tunnel_recv_rejects_oversized_length_prefix() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    // Peer sends an absurd length prefix and then holds the socket open.
    let peer = tokio::spawn(async move {
        let mut s = TcpStream::connect(addr).await.unwrap();
        s.write_all(&u32::MAX.to_le_bytes()).await.unwrap();
        tokio::time::sleep(Duration::from_secs(2)).await;
    });

    let (stream, _) = listener.accept().await.unwrap();
    let mut tunnel = tunnel_from(stream);
    let err = tunnel.recv().await.unwrap_err();
    assert!(matches!(err, NatError::Protocol(_)), "got {err:?}");

    peer.await.unwrap();
}

#[tokio::test]
async fn punch_tunnel_send_recv_roundtrip_after_connect() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let acceptor = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut tunnel = tunnel_from(stream);
        let got = tunnel.recv().await.unwrap();
        tunnel.send(&got).await.unwrap();
    });

    let bind: SocketAddr = "127.0.0.1:0".parse().unwrap();
    let mut tunnel = PunchTunnel::connect(addr, bind).await.unwrap();
    tunnel.send(b"punch-payload").await.unwrap();
    let echoed = tunnel.recv().await.unwrap();
    assert_eq!(echoed, b"punch-payload".to_vec());

    acceptor.await.unwrap();
}

#[tokio::test]
async fn punch_tunnel_connect_with_explicit_local_bind_succeeds() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let peer_addr = listener.local_addr().unwrap();
    let acceptor = tokio::spawn(async move {
        if let Ok((s, _)) = listener.accept().await {
            tokio::time::sleep(Duration::from_secs(3)).await;
            drop(s);
        }
    });

    // Non-unspecified IP forces the `else` arm of the bind-address selection.
    let bind: SocketAddr = "127.0.0.1:0".parse().unwrap();
    let tunnel = PunchTunnel::connect(peer_addr, bind).await.unwrap();
    assert_eq!(tunnel.local_addr.ip().to_string(), "127.0.0.1");
    acceptor.abort();
}

#[tokio::test]
async fn punch_tunnel_connect_falls_back_when_local_port_in_use() {
    // Hold a listener on the exact port we tell `connect` to bind, so its own
    // listener bind fails (accept slot stays `None`) and it must succeed via
    // the outbound connect slot.
    let blocked = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let blocked_port = blocked.local_addr().unwrap().port();

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let peer_addr = listener.local_addr().unwrap();
    let acceptor = tokio::spawn(async move {
        if let Ok((s, _)) = listener.accept().await {
            tokio::time::sleep(Duration::from_secs(3)).await;
            drop(s);
        }
    });

    let bind: SocketAddr = format!("127.0.0.1:{blocked_port}").parse().unwrap();
    let tunnel = PunchTunnel::connect(peer_addr, bind).await;
    assert!(tunnel.is_ok(), "expected connect-slot fallback to succeed");

    acceptor.abort();
    drop(blocked);
}

#[tokio::test]
async fn punch_coordinator_reaches_connected_state_against_local_peer() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let peer_addr = listener.local_addr().unwrap();
    let acceptor = tokio::spawn(async move {
        if let Ok((s, _)) = listener.accept().await {
            tokio::time::sleep(Duration::from_secs(3)).await;
            drop(s);
        }
    });

    let coord = PunchCoordinator::new();
    let bind: SocketAddr = "0.0.0.0:0".parse().unwrap();
    let tunnel = coord.punch(peer_addr, bind).await.unwrap();
    assert_eq!(coord.state().await, PunchState::Connected);
    assert_ne!(tunnel.local_addr.port(), 0);
    acceptor.abort();
}
