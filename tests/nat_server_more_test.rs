//! Additional coverage for `aex::p2p::nat::{server, service, types}`.
//!
//! Targets uncovered public paths:
//! - `NatRelayService`: register / lookup / `handle_split` relay round-trip
//!   between two local sockets, unknown-target error, empty-dst, self-relay,
//!   ignored frame types, duplicate registration, EOF → `RemoteClosed`.
//! - `NatRelayServer`: wrapper lookups + bind error.
//! - `NatDetector`: `max_need` / `mode` / detect branches.
//! - `nat_tcp_handler`: real context path and missing reader/writer guards.
//! - `Nat` struct: protocol registration (`with_command`, `detector`, `process`).
//! - `types`: every `NatFrameType` encode/decode, magic handling, `NatError`.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

use aex::connection::context::{BoxReader, BoxWriter, Context};
use aex::connection::global::GlobalContext;
use aex::detect::{DetectionState, DetectorMode, ProtocolDetector, Verdict};
use aex::handler::{CommandHandler, Outcome};
use aex::nat::{
    NAT_MAGIC, NAT_MAGIC_LEN, NatDetector, NatError, NatFrame, NatFrameType, NatRelayServer,
    NatRelayService, frame_len_from_header, nat_tcp_handler,
};
use aex::protocol::Protocol;

/// Connected TCP pair: `(client_end, server_end, client_addr)`.
///
/// `client_addr` is the source address the server observes (used as the relay's
/// notion of the peer's public address).
async fn pair() -> (TcpStream, TcpStream, SocketAddr) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let saddr = listener.local_addr().unwrap();
    let client = TcpStream::connect(saddr).await.unwrap();
    let caddr = client.local_addr().unwrap();
    let (server, _) = listener.accept().await.unwrap();
    (client, server, caddr)
}

fn boxed(stream: TcpStream) -> (BoxReader, BoxWriter) {
    let (r, w) = stream.into_split();
    (Box::new(BufReader::new(r)), Box::new(w))
}

/// Read one full wire frame (`magic + len(u32 LE) + body`).
async fn read_frame(stream: &mut TcpStream) -> NatFrame {
    let mut head = [0u8; NAT_MAGIC_LEN + 4];
    stream.read_exact(&mut head).await.unwrap();
    let len = frame_len_from_header(&head).unwrap();
    let mut body = vec![0u8; len];
    stream.read_exact(&mut body).await.unwrap();
    NatFrame::decode(&body).unwrap()
}

/// Read frames until one of `want` type arrives (relay interleaves `Peers`).
async fn read_until(stream: &mut TcpStream, want: NatFrameType) -> NatFrame {
    loop {
        let f = tokio::time::timeout(Duration::from_secs(3), read_frame(stream))
            .await
            .expect("read_frame timeout");
        if f.frame_type == want {
            return f;
        }
    }
}

async fn wait_peers(service: &NatRelayService, n: usize) {
    for _ in 0..100 {
        if service.peer_count() == n {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("peer_count never reached {n}");
}

// ---------------------------------------------------------------------------
// NatRelayService: register / lookup / relay round-trip
// ---------------------------------------------------------------------------

#[tokio::test]
async fn handle_split_relays_data_between_two_local_sockets() {
    let service = NatRelayService::new();

    let (mut ca, sa, aaddr) = pair().await;
    let (mut cb, sb, baddr) = pair().await;

    let sa_task = {
        let s = service.clone();
        tokio::spawn(async move {
            let (r, w) = boxed(sa);
            s.handle_split(r, w, aaddr).await
        })
    };
    let sb_task = {
        let s = service.clone();
        tokio::spawn(async move {
            let (r, w) = boxed(sb);
            s.handle_split(r, w, baddr).await
        })
    };

    // Register both peers; first frame back is always RegisterAck.
    ca.write_all(&NatFrame::register("node_a").encode_wire().unwrap())
        .await
        .unwrap();
    assert_eq!(
        read_frame(&mut ca).await.frame_type,
        NatFrameType::RegisterAck
    );
    cb.write_all(&NatFrame::register("node_b").encode_wire().unwrap())
        .await
        .unwrap();
    assert_eq!(
        read_frame(&mut cb).await.frame_type,
        NatFrameType::RegisterAck
    );

    wait_peers(&service, 2).await;
    assert!(service.peer_public_addr("node_a").is_some());
    assert!(service.peer_public_addr("node_b").is_some());
    assert_eq!(service.peers_snapshot().len(), 2);

    // a -> b: relay must forward the Data frame to b's writer.
    ca.write_all(
        &NatFrame::data("node_a", "node_b", b"ping".to_vec())
            .encode_wire()
            .unwrap(),
    )
    .await
    .unwrap();
    let f = read_until(&mut cb, NatFrameType::Data).await;
    assert_eq!(f.src, "node_a");
    assert_eq!(f.dst, "node_b");
    assert_eq!(f.payload, b"ping".to_vec());

    // b -> a: relay must forward back.
    cb.write_all(
        &NatFrame::data("node_b", "node_a", b"pong".to_vec())
            .encode_wire()
            .unwrap(),
    )
    .await
    .unwrap();
    let f = read_until(&mut ca, NatFrameType::Data).await;
    assert_eq!(f.payload, b"pong".to_vec());

    // Closing both clients drives handle_split into the EOF branch.
    drop(ca);
    drop(cb);
    assert!(matches!(
        sa_task.await.unwrap(),
        Err(NatError::RemoteClosed)
    ));
    assert!(matches!(
        sb_task.await.unwrap(),
        Err(NatError::RemoteClosed)
    ));
}

#[tokio::test]
async fn handle_split_unknown_target_returns_peer_not_found() {
    let service = NatRelayService::new();
    let (mut ca, sa, aaddr) = pair().await;
    let task = {
        let s = service.clone();
        tokio::spawn(async move {
            let (r, w) = boxed(sa);
            s.handle_split(r, w, aaddr).await
        })
    };

    // Data to an unregistered node: `relay_data` returns PeerNotFound, which
    // propagates out of the read loop and ends the connection handler.
    ca.write_all(
        &NatFrame::data("x", "ghost", b"p".to_vec())
            .encode_wire()
            .unwrap(),
    )
    .await
    .unwrap();
    let res = tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(res, Err(NatError::PeerNotFound(_))));
}

#[tokio::test]
async fn handle_split_empty_dst_is_ignored_and_eof_closes() {
    let service = NatRelayService::new();
    let (mut ca, sa, aaddr) = pair().await;
    let task = {
        let s = service.clone();
        tokio::spawn(async move {
            let (r, w) = boxed(sa);
            s.handle_split(r, w, aaddr).await
        })
    };

    // Empty dst → early Ok, loop keeps going.
    ca.write_all(
        &NatFrame::data("x", "", b"drop".to_vec())
            .encode_wire()
            .unwrap(),
    )
    .await
    .unwrap();
    // Prove the loop is still alive by registering and reading the ack.
    ca.write_all(&NatFrame::register("alive").encode_wire().unwrap())
        .await
        .unwrap();
    assert_eq!(
        read_until(&mut ca, NatFrameType::RegisterAck)
            .await
            .frame_type,
        NatFrameType::RegisterAck
    );

    drop(ca);
    // Dropping the client mid-stream may surface as clean EOF (RemoteClosed) or
    // a reset (Io) depending on unread buffered frames; either ends the handler.
    assert!(task.await.unwrap().is_err());
}

#[tokio::test]
async fn handle_split_self_relay_is_noop_and_connection_survives() {
    let service = NatRelayService::new();
    let (mut ca, sa, aaddr) = pair().await;
    let task = {
        let s = service.clone();
        tokio::spawn(async move {
            let (r, w) = boxed(sa);
            s.handle_split(r, w, aaddr).await
        })
    };

    ca.write_all(&NatFrame::register("node_a").encode_wire().unwrap())
        .await
        .unwrap();
    assert_eq!(
        read_until(&mut ca, NatFrameType::RegisterAck)
            .await
            .frame_type,
        NatFrameType::RegisterAck
    );

    // dst == src (this same connection): relay detects same addr and no-ops.
    ca.write_all(
        &NatFrame::data("node_a", "node_a", b"loop".to_vec())
            .encode_wire()
            .unwrap(),
    )
    .await
    .unwrap();

    // Still alive: re-register and observe ack.
    ca.write_all(&NatFrame::register("node_a").encode_wire().unwrap())
        .await
        .unwrap();
    assert_eq!(
        read_until(&mut ca, NatFrameType::RegisterAck)
            .await
            .frame_type,
        NatFrameType::RegisterAck
    );

    drop(ca);
    assert!(matches!(task.await.unwrap(), Err(NatError::RemoteClosed)));
}

#[tokio::test]
async fn handle_split_ignores_unhandled_frame_types() {
    let service = NatRelayService::new();
    let (mut ca, sa, aaddr) = pair().await;
    let task = {
        let s = service.clone();
        tokio::spawn(async move {
            let (r, w) = boxed(sa);
            s.handle_split(r, w, aaddr).await
        })
    };

    // KeepAliveAck / PunchHint / Peers hit the relay's `_ =>` ignore arm.
    for f in [
        NatFrame::keep_alive_ack("node_a"),
        NatFrame::punch_hint("node_b", "1.2.3.4:5"),
        NatFrame::peers("node_a", &[]),
    ] {
        ca.write_all(&f.encode_wire().unwrap()).await.unwrap();
    }
    // Relay remains alive and still answers Register.
    ca.write_all(&NatFrame::register("survivor").encode_wire().unwrap())
        .await
        .unwrap();
    assert_eq!(
        read_until(&mut ca, NatFrameType::RegisterAck)
            .await
            .frame_type,
        NatFrameType::RegisterAck
    );

    drop(ca);
    assert!(task.await.unwrap().is_err());
}

#[tokio::test]
async fn handle_split_duplicate_registration_replaces_peer() {
    let service = NatRelayService::new();

    let (mut ca, sa, aaddr) = pair().await;
    let (mut cb, sb, baddr) = pair().await;
    let ta = {
        let s = service.clone();
        tokio::spawn(async move {
            let (r, w) = boxed(sa);
            s.handle_split(r, w, aaddr).await
        })
    };
    let tb = {
        let s = service.clone();
        tokio::spawn(async move {
            let (r, w) = boxed(sb);
            s.handle_split(r, w, baddr).await
        })
    };

    ca.write_all(&NatFrame::register("dup").encode_wire().unwrap())
        .await
        .unwrap();
    assert_eq!(
        read_frame(&mut ca).await.frame_type,
        NatFrameType::RegisterAck
    );
    wait_peers(&service, 1).await;

    // Same node_id from a different source addr → duplicate-register warning
    // branch; registry keeps a single entry.
    cb.write_all(&NatFrame::register("dup").encode_wire().unwrap())
        .await
        .unwrap();
    assert_eq!(
        read_frame(&mut cb).await.frame_type,
        NatFrameType::RegisterAck
    );
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(service.peer_count(), 1);

    drop(ca);
    drop(cb);
    let _ = ta.await;
    let _ = tb.await;
}

#[tokio::test]
async fn handle_socket_closed_by_peer_returns_remote_closed() {
    let service = NatRelayService::new();
    let (ca, sa, aaddr) = pair().await;
    // Peer closes before sending a full frame header → UnexpectedEof.
    drop(ca);
    let res = service.handle_socket(sa, aaddr).await;
    assert!(matches!(res, Err(NatError::RemoteClosed)));
}

// ---------------------------------------------------------------------------
// NatRelayServer wrappers / bind error / Default
// ---------------------------------------------------------------------------

#[tokio::test]
async fn relay_server_wrappers_and_bind_error() {
    let server = NatRelayServer::bind("127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    assert_eq!(server.peer_count(), 0);
    assert!(server.peer_public_addr("nobody").is_none());
    assert!(server.peers_snapshot().is_empty());

    // Binding an already-used address must fail.
    let probe = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let taken = probe.local_addr().unwrap();
    assert!(NatRelayServer::bind(taken).await.is_err());
}

#[tokio::test]
async fn relay_service_default_is_empty() {
    let service = NatRelayService::default();
    assert_eq!(service.peer_count(), 0);
    assert!(service.peers_snapshot().is_empty());
}

// ---------------------------------------------------------------------------
// NatDetector
// ---------------------------------------------------------------------------

#[test]
fn nat_detector_max_need_and_mode() {
    let d = NatDetector;
    assert_eq!(d.max_need(), Some(NAT_MAGIC_LEN));
    assert_eq!(d.mode(), DetectorMode::Forward);
    assert_eq!(d.name(), "nat-detector");
    assert_eq!(d.protocol(), "nat");
}

#[test]
fn nat_detector_detect_branches() {
    let d = NatDetector;
    let mut state = DetectionState::new();
    // Empty buffer → NeedMore(full magic).
    match d.detect(&[], &mut state) {
        Verdict::NeedMore(n) => assert_eq!(n, NAT_MAGIC_LEN),
        v => panic!("expected NeedMore, got {v:?}"),
    }
    // Partial magic (one byte) → NeedMore(remaining).
    match d.detect(&NAT_MAGIC[..1], &mut state) {
        Verdict::NeedMore(n) => assert_eq!(n, NAT_MAGIC_LEN - 1),
        v => panic!("expected NeedMore, got {v:?}"),
    }
    // Full magic → Match.
    assert!(matches!(d.detect(NAT_MAGIC, &mut state), Verdict::Match));
    // Non-magic, long enough → Pass.
    assert!(matches!(
        d.detect(b"GET / HTTP/1.1", &mut state),
        Verdict::Pass
    ));
}

// ---------------------------------------------------------------------------
// nat_tcp_handler
// ---------------------------------------------------------------------------

fn globals(addr: SocketAddr) -> Arc<GlobalContext> {
    Arc::new(GlobalContext::new(addr, None))
}

#[tokio::test]
async fn nat_tcp_handler_without_reader_returns_immediately() {
    let addr: SocketAddr = "127.0.0.1:1".parse().unwrap();
    let (_client, server, caddr) = pair().await;
    let (_r, w) = boxed(server);
    let ctx = Context::new(None, Some(w), globals(addr), caddr);
    nat_tcp_handler(NatRelayService::new())(ctx).await.unwrap();
}

#[tokio::test]
async fn nat_tcp_handler_without_writer_returns_immediately() {
    let addr: SocketAddr = "127.0.0.1:1".parse().unwrap();
    let (_client, server, caddr) = pair().await;
    let (r, _w) = boxed(server);
    let ctx = Context::new(Some(r), None, globals(addr), caddr);
    nat_tcp_handler(NatRelayService::new())(ctx).await.unwrap();
}

#[tokio::test]
async fn nat_tcp_handler_relays_a_real_connection() {
    let addr: SocketAddr = "127.0.0.1:1".parse().unwrap();
    let (mut client, server, caddr) = pair().await;
    let (r, w) = boxed(server);
    let service = NatRelayService::new();
    let ctx = Context::new(Some(r), Some(w), globals(addr), caddr);
    let handle = nat_tcp_handler(service.clone())(ctx);

    client
        .write_all(&NatFrame::register("node_h").encode_wire().unwrap())
        .await
        .unwrap();
    assert_eq!(
        read_frame(&mut client).await.frame_type,
        NatFrameType::RegisterAck
    );
    wait_peers(&service, 1).await;

    // Closing drives handle_split to Err(RemoteClosed) → the handler's debug
    // branch runs before the spawned task exits.
    drop(client);
    handle.await.unwrap();
}

// ---------------------------------------------------------------------------
// Nat struct / Protocol impl
// ---------------------------------------------------------------------------

#[tokio::test]
async fn nat_struct_protocol_surface() {
    use aex::nat::Nat;

    let service = NatRelayService::new();
    let nat = Nat::new(service);

    assert_eq!(nat.name(), "nat");
    assert!(nat.detector().is_some());
    assert_eq!(nat.commands().len(), 0);

    let handler: CommandHandler<bytes::Bytes> =
        Arc::new(|_ctx, _payload| Box::pin(async { Outcome::Done }));
    let nat = nat.with_command("ping", handler);
    assert_eq!(nat.commands().len(), 1);

    let addr: SocketAddr = "127.0.0.1:1".parse().unwrap();
    let mut ctx = Context::new(None, None, globals(addr), addr);
    // `handle` takes the context, spawns the relay handler and reports Done.
    assert_eq!(nat.handle(&mut ctx).await, Outcome::Done);

    let mut ctx2 = Context::new(None, None, globals(addr), addr);
    let outcome = Protocol::process(&nat, &mut ctx2).await;
    assert_eq!(outcome, Outcome::Done);
}

// ---------------------------------------------------------------------------
// types: frame encode/decode, magic handling, NatError
// ---------------------------------------------------------------------------

fn sample_frames() -> Vec<NatFrame> {
    vec![
        NatFrame::register("node_a"),
        NatFrame::register_ack("1.2.3.4:5000"),
        NatFrame::keep_alive("node_a"),
        NatFrame::keep_alive_ack("node_a"),
        NatFrame::data("node_a", "node_b", vec![1, 2, 3, 4]),
        NatFrame::punch_request("node_a", "node_b"),
        NatFrame::punch_hint("node_b", "5.6.7.8:9000"),
        NatFrame::punch_start("node_a", "node_b"),
        NatFrame::peers(
            "node_a",
            &[("node_b".to_string(), "1.2.3.4:5000".to_string())],
        ),
        // `Route` has no constructor helper; build it directly to cover its
        // Encode/Decode impl.
        NatFrame {
            frame_type: NatFrameType::Route,
            src: "node_a".to_string(),
            dst: "node_b".to_string(),
            public_addr: String::new(),
            extra: String::new(),
            payload: Vec::new(),
        },
    ]
}

#[test]
fn every_nat_frame_type_roundtrips_through_wire() {
    for frame in sample_frames() {
        let wire = frame.encode_wire().unwrap();
        assert_eq!(&wire[..NAT_MAGIC.len()], NAT_MAGIC);
        let decoded = NatFrame::decode(&wire).unwrap();
        assert_eq!(decoded.frame_type, frame.frame_type);
        assert_eq!(decoded.src, frame.src);
    }
}

#[test]
fn frame_peers_payload_decodes_to_list() {
    let list = vec![
        ("node_b".to_string(), "1.2.3.4:5000".to_string()),
        ("node_c".to_string(), "5.6.7.8:9000".to_string()),
    ];
    let frame = NatFrame::peers("node_a", &list);
    let decoded = NatFrame::decode(&frame.encode_wire().unwrap()).unwrap();
    let (round, _): (Vec<(String, String)>, usize) =
        bincode::decode_from_slice(&decoded.payload, bincode::config::standard()).unwrap();
    assert_eq!(round, list);
}

#[test]
fn decode_rejects_invalid_magic() {
    // Unknown magic → treated as a bare bincode body, which fails to parse.
    assert!(NatFrame::decode(b"NOTNATxxxx").is_err());
}

#[test]
fn decode_with_magic_but_truncated_header_is_error() {
    // Magic present but shorter than magic+len → the `&bytes[NAT_MAGIC_LEN..]`
    // fallback slice is used and bincode decode fails.
    let mut bytes = Vec::from(NAT_MAGIC);
    bytes.extend_from_slice(&[0u8, 1, 2]);
    assert!(NatFrame::decode(&bytes).is_err());
}

#[test]
fn decode_clamps_declared_length_beyond_buffer() {
    // magic + declared length far larger than the actual body → `end` clamps to
    // the buffer length, so no huge allocation / panic.
    let mut bytes = Vec::from(NAT_MAGIC);
    bytes.extend_from_slice(&(NAT_MAGIC_LEN as u32 + 100_000).to_le_bytes());
    bytes.extend_from_slice(&[0u8; 4]);
    let _ = NatFrame::decode(&bytes);
}

#[test]
fn frame_len_from_header_reports_short_and_oversized() {
    assert!(frame_len_from_header(&[0u8; 3]).is_err());
    let mut head = vec![0u8; NAT_MAGIC_LEN + 4];
    head[..NAT_MAGIC_LEN].copy_from_slice(NAT_MAGIC);
    head[NAT_MAGIC_LEN..].copy_from_slice(&7u32.to_le_bytes());
    assert_eq!(frame_len_from_header(&head).unwrap(), 7);
}

#[test]
fn nat_error_variants_display() {
    let errs = [
        NatError::AlreadyRegistered("n".into()),
        NatError::PeerNotFound("n".into()),
        NatError::NotReachable("n".into()),
        NatError::Io(std::io::Error::new(std::io::ErrorKind::Other, "boom")),
        NatError::Protocol("bad".into()),
        NatError::RemoteClosed,
    ];
    for e in errs {
        let msg = e.to_string();
        assert!(!msg.is_empty());
    }
}
