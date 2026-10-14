//! Additional coverage for `aex::connection::manager::ConnectionManager` and
//! the `ConnectionEntry` lifecycle it stores. Complements
//! `connection_manager_test.rs` / `connection_manager_gap_test.rs` by hitting
//! branches they leave open: the `Default` impl, the client-side duplicate
//! short-circuit in `connect`, connect timeouts, deindexing on removal, the
//! outbound (server) side of `update`/`mark_active`/`cancel_by_addr`/
//! `cancel_all_by_ip`, uptime statistics, and the `nat_contact_addrs` fallback
//! for server buckets (and the indexed-but-node-less path).

use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use aex::connection::context::Context;
use aex::connection::entry::ConnectionEntry;
use aex::connection::global::GlobalContext;
use aex::connection::manager::ConnectionManager;
use aex::connection::node::Node;
use aex::connection::scope::NetworkScope;
use aex::tcp::types::RawCodec;

/// Allow loopback connections for this test binary (the manager otherwise
/// ignores `127.0.0.1`). Set once, before any `ConnectionManager` use, so the
/// lazily-cached decision sees "1".
fn allow_loopback() {
    static ALLOW: std::sync::Once = std::sync::Once::new();
    ALLOW.call_once(|| unsafe {
        std::env::set_var("AEX_ALLOW_LOOPBACK", "1");
    });
}

fn pending_handle() -> tokio::task::AbortHandle {
    allow_loopback();
    tokio::spawn(std::future::pending::<()>()).abort_handle()
}

fn key_of(addr: SocketAddr) -> (std::net::IpAddr, NetworkScope) {
    (addr.ip(), NetworkScope::from_ip(&addr.ip()))
}

/// Replace the node record on an existing entry (simulating a finished handshake).
async fn set_node(
    manager: &ConnectionManager,
    addr: SocketAddr,
    id: Vec<u8>,
    nat: Vec<SocketAddr>,
) {
    let entry = manager.find_entry(&addr).expect("entry present");
    entry
        .update_node(Node {
            id,
            version: 1,
            started_at: 0,
            port: addr.port(),
            protocols: HashSet::new(),
            ips: vec![],
            nat_addrs: nat
                .into_iter()
                .map(|a| (NetworkScope::Extranet, a))
                .collect(),
        })
        .await;
}

#[test]
fn manager_default_matches_new() {
    let manager = ConnectionManager::default();
    assert!(manager.connections.is_empty());
    assert!(manager.index_by_id.is_empty());
    assert!(!manager.cancel_token.is_cancelled());
}

#[tokio::test]
async fn connect_short_circuits_when_client_entry_exists() {
    let manager = ConnectionManager::new();
    let addr: SocketAddr = "127.0.0.1:45999".parse().unwrap();

    // A *client*-direction entry for the exact address: the `connect` duplicate
    // guard must return before dialing.
    manager.add(addr, pending_handle(), CancellationToken::new(), true, None);

    let global = Arc::new(GlobalContext::new(addr, None));
    let ran = Arc::new(AtomicBool::new(false));
    let flag = ran.clone();

    let result = manager
        .connect::<RawCodec, RawCodec, _, _>(
            addr,
            global,
            move |_ctx| {
                let flag = flag.clone();
                async move {
                    flag.store(true, Ordering::SeqCst);
                }
            },
            Some(2),
        )
        .await;

    assert!(result.is_ok());
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(!ran.load(Ordering::SeqCst), "duplicate must not dial");
}

#[tokio::test]
async fn connect_success_registers_outbound_entry() {
    allow_loopback();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { while listener.accept().await.is_ok() {} });

    let manager = ConnectionManager::new();
    let global = Arc::new(GlobalContext::new(addr, None));
    manager
        .connect::<RawCodec, RawCodec, _, _>(addr, global, |_ctx| async {}, Some(5))
        .await
        .unwrap();

    assert!(manager.find_entry(&addr).is_some());
    assert!(manager.get_all_entries().contains(&addr));
    assert!(
        manager
            .connections
            .get(&key_of(addr))
            .unwrap()
            .servers
            .contains_key(&addr)
    );
}

#[tokio::test]
async fn connect_timeout_returns_error() {
    allow_loopback();
    // TEST-NET-1 (RFC 5737) is a blackhole; a zero timeout cannot be satisfied
    // before the dial completes, so the timeout arm is taken.
    let addr: SocketAddr = "192.0.2.1:9".parse().unwrap();
    let manager = ConnectionManager::new();
    let global = Arc::new(GlobalContext::new(addr, None));

    let result = manager
        .connect::<RawCodec, RawCodec, _, _>(addr, global, |_ctx| async {}, Some(0))
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn remove_deindexes_entry_with_node_id() {
    let manager = ConnectionManager::new();
    let addr: SocketAddr = "10.1.2.3:5000".parse().unwrap();
    manager.add(addr, pending_handle(), CancellationToken::new(), true, None);
    set_node(&manager, addr, vec![5, 5, 5], vec![]).await;

    let entry = manager.find_entry(&addr).unwrap();
    manager.index_node(vec![5, 5, 5], entry);
    assert!(manager.index_by_id.contains_key(&vec![5, 5, 5]));

    manager.remove(addr, true);
    assert!(!manager.index_by_id.contains_key(&vec![5, 5, 5]));
    assert!(manager.connections.is_empty());
}

#[tokio::test]
async fn update_outbound_replaces_context() {
    let manager = ConnectionManager::new();
    let addr: SocketAddr = "10.2.3.4:5001".parse().unwrap();
    manager.add(
        addr,
        pending_handle(),
        CancellationToken::new(),
        false,
        None,
    );

    let global = Arc::new(GlobalContext::new(addr, None));
    let ctx = Arc::new(Mutex::new(Context::new(None, None, global, addr)));
    manager.update(addr, false, Some(ctx));

    let bi = manager.connections.get(&key_of(addr)).unwrap();
    assert!(bi.servers.get(&addr).unwrap().context.is_some());
}

#[tokio::test]
async fn mark_active_updates_outbound_last_seen() {
    let manager = ConnectionManager::new();
    let addr: SocketAddr = "10.3.4.5:5002".parse().unwrap();
    manager.add(
        addr,
        pending_handle(),
        CancellationToken::new(),
        false,
        None,
    );

    let bi = manager.connections.get(&key_of(addr)).unwrap();
    let entry = bi.servers.get(&addr).unwrap();
    entry.last_seen.store(0, Ordering::Relaxed);
    drop(entry);
    drop(bi);

    manager.mark_active(addr, false);
    let bi = manager.connections.get(&key_of(addr)).unwrap();
    assert!(
        bi.servers
            .get(&addr)
            .unwrap()
            .last_seen
            .load(Ordering::Relaxed)
            > 0
    );
}

#[tokio::test]
async fn cancel_by_addr_finds_server_entries() {
    let manager = ConnectionManager::new();
    let addr: SocketAddr = "10.4.5.6:5003".parse().unwrap();
    manager.add(
        addr,
        pending_handle(),
        CancellationToken::new(),
        false,
        None,
    );

    assert!(manager.cancel_by_addr(addr));
    assert!(manager.connections.is_empty(), "empty bucket is cleaned up");
}

#[tokio::test]
async fn cancel_all_by_ip_clears_clients_and_servers() {
    let manager = ConnectionManager::new();
    let ip: std::net::IpAddr = "10.5.6.7".parse().unwrap();
    let client: SocketAddr = SocketAddr::new(ip, 6001);
    let server: SocketAddr = SocketAddr::new(ip, 6002);

    manager.add(
        client,
        pending_handle(),
        CancellationToken::new(),
        true,
        None,
    );
    manager.add(
        server,
        pending_handle(),
        CancellationToken::new(),
        false,
        None,
    );
    set_node(&manager, client, vec![1], vec![]).await;
    set_node(&manager, server, vec![2], vec![]).await;

    manager.cancel_all_by_ip(ip);
    assert!(
        manager
            .connections
            .get(&(ip, NetworkScope::from_ip(&ip)))
            .is_none()
    );
}

#[tokio::test]
async fn status_tracks_scopes_uptime_and_average() {
    let manager = ConnectionManager::new();
    let intranet: SocketAddr = "10.6.7.8:7000".parse().unwrap();
    let extranet: SocketAddr = "1.1.1.1:7001".parse().unwrap();
    manager.add(
        intranet,
        pending_handle(),
        CancellationToken::new(),
        true,
        None,
    );
    manager.add(
        extranet,
        pending_handle(),
        CancellationToken::new(),
        true,
        None,
    );

    // Let real wall-clock uptime grow past zero so `oldest_uptime` is exercised.
    tokio::time::sleep(Duration::from_millis(1100)).await;
    let status = manager.status();
    assert_eq!(status.total_ips, 2);
    assert_eq!(status.total_clients, 2);
    assert_eq!(status.intranet_conns, 1);
    assert_eq!(status.extranet_conns, 1);
    assert!(status.oldest_uptime >= 1);
    assert!(status.average_uptime >= 1);
}

#[tokio::test]
async fn notify_fallback_without_node_matches_nothing() {
    let manager = ConnectionManager::new();
    let addr: SocketAddr = "10.7.8.9:8000".parse().unwrap();
    manager.add(addr, pending_handle(), CancellationToken::new(), true, None);

    // No node was ever set, so the full-traversal fallback matches nothing.
    let seen = Arc::new(std::sync::atomic::AtomicUsize::new(usize::MAX));
    let s = seen.clone();
    manager
        .notify(&[9, 9, 9], |entries| async move {
            s.store(entries.len(), Ordering::SeqCst);
        })
        .await;
    assert_eq!(seen.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn notify_finds_node_through_traversal_without_index() {
    let manager = ConnectionManager::new();
    let addr: SocketAddr = "10.8.9.10:8001".parse().unwrap();
    manager.add(addr, pending_handle(), CancellationToken::new(), true, None);
    set_node(&manager, addr, vec![4, 2], vec![]).await;

    manager
        .notify(&[4, 2], |entries| async move {
            assert_eq!(entries.len(), 1);
            assert_eq!(entries[0].addr, addr);
        })
        .await;
}

#[tokio::test]
async fn nat_contact_addrs_index_without_node_is_empty() {
    let manager = ConnectionManager::new();
    let id = vec![1u8, 2, 3];
    let entry = Arc::new(ConnectionEntry::new_empty_node(
        "4.4.4.4:4000".parse().unwrap(),
        None,
        pending_handle(),
        CancellationToken::new(),
    ));
    manager.index_node(id.clone(), entry);
    assert!(manager.nat_contact_addrs(&id).await.is_empty());
}

#[tokio::test]
async fn nat_contact_addrs_fallback_covers_server_bucket() {
    let manager = ConnectionManager::new();
    let id = vec![7u8, 7, 7];
    let addr: SocketAddr = "10.9.9.9:9000".parse().unwrap();
    // Outbound (server) entry, deliberately not indexed -> traversal fallback.
    manager.add(
        addr,
        pending_handle(),
        CancellationToken::new(),
        false,
        None,
    );
    set_node(
        &manager,
        addr,
        id.clone(),
        vec!["203.0.113.5:9000".parse().unwrap()],
    )
    .await;

    let addrs = manager.nat_contact_addrs(&id).await;
    assert!(addrs.contains(&"203.0.113.5:9000".parse().unwrap()));
}

// --- ConnectionEntry lifecycle -------------------------------------------------

#[tokio::test]
async fn connection_entry_basic_lifecycle_and_debug() {
    let addr: SocketAddr = "10.10.10.10:9100".parse().unwrap();
    let entry =
        ConnectionEntry::new_empty_node(addr, None, pending_handle(), CancellationToken::new());

    assert_eq!(format!("{:?}", entry).contains("ConnectionEntry"), true);
    assert!(entry.get_peer_id().await.is_none());
    assert!(entry.uptime_secs() < 5);

    entry
        .update_node(Node {
            id: vec![1, 2, 3],
            version: 1,
            started_at: 0,
            port: 9100,
            protocols: HashSet::new(),
            ips: vec![],
            nat_addrs: vec![],
        })
        .await;
    assert_eq!(entry.get_peer_id().await, Some(vec![1, 2, 3]));

    // A generous timeout keeps the fresh connection alive.
    assert!(!entry.is_deactivated(0, 3600, 3600));
}

#[tokio::test]
async fn connection_entry_deactivated_on_max_lifetime() {
    let entry = ConnectionEntry::new_empty_node(
        "10.11.11.11:9101".parse().unwrap(),
        None,
        pending_handle(),
        CancellationToken::new(),
    );
    // max_lifetime_secs = 0 -> uptime >= 0 is always true.
    assert!(entry.is_deactivated(0, 3600, 0));
    // timeout_secs = 0 with a future current timestamp -> idle too long.
    assert!(entry.is_deactivated(u64::MAX, 0, 3600));
}

#[tokio::test]
async fn connection_entry_drop_aborts_task() {
    let join = tokio::spawn(std::future::pending::<()>());
    let handle = join.abort_handle();
    {
        let _entry = ConnectionEntry::new_empty_node(
            "10.12.12.12:9102".parse().unwrap(),
            None,
            handle.clone(),
            CancellationToken::new(),
        );
    }
    tokio::task::yield_now().await;
    assert!(handle.is_finished(), "Drop must abort the task");
}

#[tokio::test]
async fn connection_entry_start_heartbeat_without_context_is_noop() {
    let entry = ConnectionEntry::new_empty_node(
        "10.13.13.13:9103".parse().unwrap(),
        None,
        pending_handle(),
        CancellationToken::new(),
    );
    entry.start_heartbeat(
        Node::from_addr("10.13.13.13:9103".parse().unwrap(), None, None),
        Default::default(),
    );
}

#[tokio::test]
async fn connection_entry_start_heartbeat_with_context_spawns() {
    use aex::connection::heartbeat::HeartbeatConfig;

    let addr: SocketAddr = "10.14.14.14:9104".parse().unwrap();
    let (rd, _wr) = tokio::io::duplex(64);
    let reader: aex::connection::context::BoxReader = Box::new(tokio::io::BufReader::new(rd));
    let writer: aex::connection::context::BoxWriter = Box::new(tokio::io::sink());
    let global = Arc::new(GlobalContext::new(addr, None));
    let ctx = Arc::new(Mutex::new(Context::new(
        Some(reader),
        Some(writer),
        global,
        addr,
    )));

    let entry = ConnectionEntry::new_empty_node(
        addr,
        Some(ctx),
        pending_handle(),
        CancellationToken::new(),
    );
    entry.start_heartbeat(
        Node::from_addr(addr, None, None),
        HeartbeatConfig::new().with_interval(1).with_timeout(1),
    );
    tokio::time::sleep(Duration::from_millis(20)).await;
}

#[tokio::test]
async fn connection_entry_start_heartbeat_with_global() {
    let addr: SocketAddr = "10.15.15.15:9105".parse().unwrap();
    let (_rd, wr) = tokio::io::duplex(64);
    let writer: aex::connection::context::BoxWriter = Box::new(tokio::io::sink());
    drop(wr);
    let global = Arc::new(GlobalContext::new(addr, None).with_heartbeat_config(
        aex::connection::heartbeat::HeartbeatConfig::new().with_interval(1),
    ));
    let ctx = Arc::new(Mutex::new(Context::new(
        None,
        Some(writer),
        global.clone(),
        addr,
    )));
    let entry = ConnectionEntry::new_empty_node(
        addr,
        Some(ctx),
        pending_handle(),
        CancellationToken::new(),
    );
    entry.start_heartbeat_with_global(&global).await;
    tokio::time::sleep(Duration::from_millis(20)).await;
}

#[tokio::test]
async fn connection_entry_default_pipeline_and_start() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let accept = tokio::spawn(async move { listener.accept().await.unwrap().0 });
    let client = tokio::net::TcpStream::connect(addr).await.unwrap();
    let server = accept.await.unwrap();

    let parent = CancellationToken::new();
    let global = Arc::new(GlobalContext::new(addr, None));
    let pipeline = ConnectionEntry::default_pipeline::<RawCodec, RawCodec>(addr, true);

    let (child_token, abort, ctx) =
        ConnectionEntry::start(parent.clone(), server, addr, global, move |c| {
            Box::pin(async move {
                pipeline(c).await?;
                Ok(())
            })
        });

    assert!(!child_token.is_cancelled());
    assert!(ctx.lock().await.local_addr.is_some());
    parent.cancel();
    abort.abort();
    drop(client);
    tokio::time::sleep(Duration::from_millis(20)).await;
}
