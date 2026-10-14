//! Additional coverage for `aex::connection::global::GlobalContext`.
//!
//! Complements `connection_global_context_test.rs` /
//! `connection_global_context_gap_test.rs` with the `FrameErrorPolicy` builder
//! (both variants + `Default`), `set_server_name`, `start_heartbeat` when no
//! manager exists, `add_exit` key replacement, duplicate `pipe` registration,
//! and the `get_connection_info` local-address / node-id branches for both
//! inbound and outbound entries.

use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::Arc;

use futures::FutureExt;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use aex::connection::context::Context;
use aex::connection::global::{FrameErrorPolicy, GlobalContext};
use aex::connection::heartbeat::HeartbeatConfig;
use aex::connection::node::Node;
use aex::connection::scope::NetworkScope;

fn addr(s: &str) -> SocketAddr {
    s.parse().unwrap()
}

fn pending_handle() -> tokio::task::AbortHandle {
    tokio::spawn(std::future::pending::<()>()).abort_handle()
}

fn ctx_with_local(addr: SocketAddr, local: SocketAddr) -> Arc<Mutex<Context>> {
    let mut c = Context::new(None, None, Arc::new(GlobalContext::new(addr, None)), addr);
    c.local_addr = Some(local);
    Arc::new(Mutex::new(c))
}

#[test]
fn frame_error_policy_default_and_builder() {
    assert_eq!(FrameErrorPolicy::default(), FrameErrorPolicy::Close);

    let drop_ctx = GlobalContext::new(addr("127.0.0.1:0"), None)
        .with_frame_error_policy(FrameErrorPolicy::Drop);
    assert_eq!(drop_ctx.frame_error_policy, FrameErrorPolicy::Drop);

    let close_ctx = GlobalContext::new(addr("127.0.0.1:0"), None)
        .with_frame_error_policy(FrameErrorPolicy::Close);
    assert_eq!(close_ctx.frame_error_policy, FrameErrorPolicy::Close);
}

#[test]
fn set_server_name_updates_name() {
    let mut ctx = GlobalContext::new(addr("127.0.0.1:0"), None);
    ctx.set_server_name("custom-server".to_string());
    assert_eq!(ctx.name, "custom-server");
}

#[tokio::test]
async fn start_heartbeat_without_manager_is_noop() {
    let ctx = GlobalContext::new(addr("127.0.0.1:8000"), None);
    assert!(ctx.heartbeat_manager.is_none());

    let peer = addr("127.0.0.1:9999");
    let cctx = Arc::new(Mutex::new(Context::new(
        None,
        None,
        Arc::new(GlobalContext::new(peer, None)),
        peer,
    )));
    ctx.start_heartbeat(cctx, peer, CancellationToken::new());
    assert!(ctx.heartbeat_manager.is_none());
}

#[test]
fn init_heartbeat_manager_builds_manager() {
    let mut ctx = GlobalContext::new(addr("127.0.0.1:8001"), None)
        .with_heartbeat_config(HeartbeatConfig::new().with_interval(7).with_timeout(4));
    assert!(ctx.heartbeat_manager.is_none());
    ctx.init_heartbeat_manager();
    let mgr = ctx.heartbeat_manager.as_ref().expect("manager initialised");
    assert_eq!(mgr.config.interval_secs, 7);
    assert_eq!(mgr.config.timeout_secs, 4);
}

#[tokio::test]
async fn add_exit_replacing_key_cancels_the_old_service() {
    let ctx = GlobalContext::new(addr("127.0.0.1:0"), None);

    let old_token = CancellationToken::new();
    let old_handle = tokio::spawn(std::future::pending::<()>());
    ctx.add_exit("svc", old_token.clone(), old_handle.abort_handle())
        .await;
    assert!(!old_token.is_cancelled());

    let new_token = CancellationToken::new();
    let new_handle = tokio::spawn(std::future::pending::<()>());
    ctx.add_exit("svc", new_token.clone(), new_handle.abort_handle())
        .await;

    assert!(old_token.is_cancelled(), "old token cancelled on replace");
    assert!(!new_token.is_cancelled());
    assert_eq!(ctx.get_exits().await, vec!["svc".to_string()]);

    ctx.shutdown_all().await;
    assert!(new_token.is_cancelled());
    assert!(ctx.get_exits().await.is_empty());
}

#[tokio::test]
async fn pipe_duplicate_registration_is_swallowed() {
    let ctx = GlobalContext::new(addr("127.0.0.1:0"), None);

    ctx.pipe::<u32>("dup", Box::new(|_v| async {}.boxed()))
        .await;
    // Second registration fails internally; the wrapper logs and continues.
    ctx.pipe::<u32>("dup", Box::new(|_v| async {}.boxed()))
        .await;

    // The original registration is still usable.
    ctx.pipe.send("dup", 1u32).await.unwrap();
}

#[tokio::test]
async fn get_connection_info_reports_local_addr_and_node_ids() {
    let ctx = GlobalContext::new(addr("0.0.0.0:8080"), None);

    let inbound: SocketAddr = "203.0.113.10:1111".parse().unwrap();
    let outbound: SocketAddr = "203.0.113.20:2222".parse().unwrap();

    // Inbound entry: no node record yet, but a known local address.
    ctx.manager.add(
        inbound,
        pending_handle(),
        CancellationToken::new(),
        true,
        Some(ctx_with_local(inbound, "127.0.0.1:40001".parse().unwrap())),
    );

    // Outbound entry: handshake finished (node id + intranet/extranet IPs) and
    // a known local address.
    ctx.manager.add(
        outbound,
        pending_handle(),
        CancellationToken::new(),
        false,
        Some(ctx_with_local(outbound, "127.0.0.1:40002".parse().unwrap())),
    );
    ctx.manager
        .find_entry(&outbound)
        .unwrap()
        .update_node(Node {
            id: b"out-node".to_vec(),
            version: 1,
            started_at: 0,
            port: 2222,
            protocols: HashSet::new(),
            ips: vec![
                (NetworkScope::Intranet, "10.0.0.1".parse().unwrap()),
                (NetworkScope::Extranet, "203.0.113.20".parse().unwrap()),
            ],
            nat_addrs: vec![],
        })
        .await;

    let info = ctx.get_connection_info().await;

    let inb = info
        .inbound
        .iter()
        .find(|p| p.addr == inbound.to_string())
        .expect("inbound entry");
    assert_eq!(inb.direction, "inbound");
    assert_eq!(inb.node_id, None);
    assert_eq!(inb.local_addr.as_deref(), Some("127.0.0.1:40001"));

    let out = info
        .outbound
        .iter()
        .find(|p| p.addr == outbound.to_string())
        .expect("outbound entry");
    assert_eq!(out.direction, "outbound");
    assert_eq!(out.node_id.as_deref(), Some("out-node"));
    assert_eq!(out.local_addr.as_deref(), Some("127.0.0.1:40002"));
    assert_eq!(out.intranet_ips, vec!["10.0.0.1:2222"]);
    assert_eq!(out.wan_ips, vec!["203.0.113.20:2222"]);
}

#[tokio::test]
async fn get_connection_info_empty_manager() {
    let ctx = GlobalContext::new(addr("127.0.0.1:0"), None);
    let info = ctx.get_connection_info().await;
    assert!(info.inbound.is_empty());
    assert!(info.outbound.is_empty());
}
