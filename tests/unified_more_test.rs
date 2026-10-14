//! Additional coverage for `aex::server` — the `Server` builder and the
//! handler machinery in `aex::handler`.
//!
//! Complements `unified_server_unit_test.rs` / `protocol_handler_test.rs` with:
//! `Server::default`, datagram `push`, same-name protocol replacement, the
//! `CommandRegistry::dispatch` path when no `HandlerState` is installed and
//! when dispatching by numeric id, the missing-key `get_scratch_mut` branch,
//! `ProtocolHandler::find_sub_handler` / `router`, and a `Server::default`
//! start that serves both TCP and UDP.

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use bytes::Bytes;
use futures::future::BoxFuture;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use aex::connection::context::Context;
use aex::connection::global::GlobalContext;
use aex::handler::{CommandKey, CommandRegistry, HandlerState, Outcome, ProtocolHandler};
use aex::http::router::Router as HttpRouter;
use aex::server::Server;
use aex::{tcp_raw, udp};

fn noop<'a>(_h: &'a ProtocolHandler<Bytes>, _ctx: &'a mut Context) -> BoxFuture<'a, Outcome> {
    Box::pin(async { Outcome::Done })
}

fn cmd_done<'a>(_ctx: &'a mut Context, _payload: Bytes) -> BoxFuture<'a, Outcome> {
    Box::pin(async { Outcome::Done })
}

fn cmd_unhandled<'a>(_ctx: &'a mut Context, _payload: Bytes) -> BoxFuture<'a, Outcome> {
    Box::pin(async { Outcome::Unhandled })
}

fn test_ctx() -> Context {
    let addr: SocketAddr = "127.0.0.1:9000".parse().unwrap();
    Context::new(None, None, Arc::new(GlobalContext::new(addr, None)), addr)
}

fn gctx() -> Arc<GlobalContext> {
    Arc::new(GlobalContext::new("127.0.0.1:0".parse().unwrap(), None))
}

#[test]
fn server_default_owns_globals_and_is_empty() {
    let server = Server::default();
    assert!(server.globals.paired_session_keys.is_some());
    assert!(server.protocols.is_empty());
    assert!(server.datagram.is_none());
    assert!(server.detect_enabled);
    assert!(server.registry.is_empty());
}

#[test]
fn push_udp_sets_datagram_and_leaves_streams_empty() {
    let server = Server::new(gctx()).push(udp!());
    assert!(server.datagram.is_some());
    assert!(server.protocols.is_empty());
}

#[test]
fn mount_replaces_same_name_and_keeps_default_last() {
    let dup1 = ProtocolHandler::new("dup", noop).shared();
    let dup2 = ProtocolHandler::new("dup", noop).shared();
    let fallback = ProtocolHandler::new("fallback", noop)
        .with_default(true)
        .shared();

    let server = Server::new(gctx()).push(dup1).push(fallback).push(dup2);
    let names: Vec<String> = server
        .protocols
        .iter()
        .map(|p| p.name().to_string())
        .collect();
    assert_eq!(names, vec!["dup".to_string(), "fallback".to_string()]);
}

#[tokio::test]
async fn command_registry_dispatch_without_handler_state_still_runs() {
    let mut registry: CommandRegistry<Bytes> = CommandRegistry::new();
    registry.register("x", Arc::new(cmd_done));

    // `local` has no HandlerState: the recording step must be skipped, but the
    // handler itself still runs.
    let mut ctx = test_ctx();
    let outcome = registry
        .dispatch(&mut ctx, CommandKey::from("x"), Bytes::new())
        .await;
    assert_eq!(outcome, Outcome::Done);
    assert!(ctx.local.get_ref::<HandlerState>().is_none());
}

#[tokio::test]
async fn command_registry_dispatch_by_id_records_label() {
    let mut registry: CommandRegistry<Bytes> = CommandRegistry::new();
    registry.register(7u32, Arc::new(cmd_unhandled));
    assert!(registry.contains(&CommandKey::from(7u32)));

    let mut ctx = test_ctx();
    ctx.local.set_value(HandlerState::new());
    let outcome = registry
        .dispatch(&mut ctx, CommandKey::from(7u32), Bytes::new())
        .await;
    assert_eq!(outcome, Outcome::Unhandled);

    let state = ctx.local.get_ref::<HandlerState>().unwrap();
    assert_eq!(state.commands.get("7"), Some(&1));
}

#[test]
fn handler_state_get_scratch_mut_missing_is_none() {
    let mut state = HandlerState::new();
    assert!(state.get_scratch_mut::<String>().is_none());
    state.set_scratch("kept".to_string());
    assert!(state.get_scratch_mut::<String>().is_some());
}

#[test]
fn protocol_handler_find_sub_handler_and_router() {
    let child = ProtocolHandler::new("child", noop).shared();
    let router = Arc::new(HttpRouter::default());

    let parent = ProtocolHandler::new("parent", noop)
        .sub(child)
        .with_router(Some(router.clone()));

    assert!(parent.find_sub_handler("child").is_some());
    assert!(parent.find_sub_handler("missing").is_none());
    assert!(parent.router().is_some());
    assert!(parent.detector().is_none());

    let no_router = ProtocolHandler::new("no-router", noop).with_router(None);
    assert!(no_router.router().is_none());
}

#[tokio::test]
async fn server_default_start_serves_tcp_and_udp() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);

    let tcp_hits = Arc::new(AtomicUsize::new(0));
    let udp_hits = Arc::new(AtomicUsize::new(0));
    let th = tcp_hits.clone();
    let uh = udp_hits.clone();

    let server = Server::default()
        .push(udp!(move |ctx| {
            let uh = uh.clone();
            tokio::spawn(async move {
                let _data: Vec<u8> = ctx.get().unwrap_or_default();
                uh.fetch_add(1, Ordering::SeqCst);
            })
        }))
        .push(tcp_raw!(move |mut ctx| {
            let th = th.clone();
            tokio::spawn(async move {
                let mut buf = [0u8; 16];
                if let Some(reader) = ctx.reader.as_mut() {
                    let _ = reader.read(&mut buf).await;
                }
                th.fetch_add(1, Ordering::SeqCst);
            })
        }));

    let handle = tokio::spawn(async move { server.start(addr).await });
    tokio::time::sleep(Duration::from_millis(150)).await;

    let mut conn = tokio::net::TcpStream::connect(addr).await.unwrap();
    conn.write_all(b"hi").await.unwrap();
    conn.shutdown().await.ok();

    let sock = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
    sock.send_to(b"ping", addr).await.unwrap();

    tokio::time::timeout(Duration::from_secs(3), async {
        while tcp_hits.load(Ordering::SeqCst) == 0 || udp_hits.load(Ordering::SeqCst) == 0 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("unified default server should serve both TCP and UDP");

    assert_eq!(tcp_hits.load(Ordering::SeqCst), 1);
    assert_eq!(udp_hits.load(Ordering::SeqCst), 1);
    handle.abort();
}
