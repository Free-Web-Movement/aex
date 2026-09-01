use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use aex::connection::context::Context;
use aex::connection::global::GlobalContext;
use aex::connection::manager::ConnectionManager;
use aex::connection::node::Node;
use aex::connection::scope::NetworkScope;

fn key_of(addr: SocketAddr) -> (std::net::IpAddr, NetworkScope) {
    (addr.ip(), NetworkScope::from_ip(&addr.ip()))
}

#[tokio::test]
async fn test_get_all_entries() {
    let manager = ConnectionManager::new();
    let a: SocketAddr = "1.1.1.1:1000".parse().unwrap();
    let b: SocketAddr = "2.2.2.2:2000".parse().unwrap();
    let token = CancellationToken::new();
    let handle = tokio::spawn(async {}).abort_handle();
    manager.add(a, handle.clone(), token.clone(), true, None);
    manager.add(b, handle, token, false, None);

    let addrs = manager.get_all_entries();
    assert_eq!(addrs.len(), 2);
    assert!(addrs.contains(&a));
    assert!(addrs.contains(&b));
}

#[tokio::test]
async fn test_update_replaces_context() {
    let manager = ConnectionManager::new();
    let addr: SocketAddr = "3.3.3.3:3000".parse().unwrap();
    let token = CancellationToken::new();
    manager.add(addr, tokio::spawn(async {}).abort_handle(), token, true, None);

    let global = Arc::new(GlobalContext::new(addr, None));
    let ctx = Arc::new(Mutex::new(Context::new(None, None, global, addr)));
    manager.update(addr, true, Some(ctx));

    let bi = manager.connections.get(&key_of(addr)).unwrap();
    let entry = bi.clients.get(&addr).unwrap();
    assert!(entry.context.is_some());
}

#[tokio::test]
async fn test_update_indexes_node() {
    let manager = ConnectionManager::new();
    let addr: SocketAddr = "3.3.3.4:3001".parse().unwrap();
    let token = CancellationToken::new();
    manager.add(addr, tokio::spawn(async {}).abort_handle(), token, true, None);

    {
        let bi = manager.connections.get(&key_of(addr)).unwrap();
        let entry = bi.clients.get(&addr).unwrap();
        let mut lock = entry.node.write().await;
        *lock = Some(Node {
            id: vec![9, 9, 9],
            version: 1,
            started_at: 0,
            port: 3001,
            protocols: HashSet::new(),
            ips: vec![],
            nat_addrs: vec![],
        });
    }
    let global = Arc::new(GlobalContext::new(addr, None));
    let ctx = Arc::new(Mutex::new(Context::new(None, None, global, addr)));
    manager.update(addr, true, Some(ctx));

    assert!(manager.index_by_id.contains_key(&vec![9, 9, 9]));
}

#[tokio::test]
async fn test_mark_active_updates_last_seen() {
    let manager = ConnectionManager::new();
    let addr: SocketAddr = "4.4.4.4:4000".parse().unwrap();
    let token = CancellationToken::new();
    manager.add(addr, tokio::spawn(async {}).abort_handle(), token, true, None);

    let bi = manager.connections.get(&key_of(addr)).unwrap();
    let entry = bi.clients.get(&addr).unwrap();
    entry.last_seen.store(0, Ordering::Relaxed);
    manager.mark_active(addr, true);
    assert!(entry.last_seen.load(Ordering::Relaxed) > 0);
}

#[tokio::test]
async fn test_index_and_deindex_node() {
    let manager = ConnectionManager::new();
    let addr: SocketAddr = "5.5.5.5:5000".parse().unwrap();
    let token = CancellationToken::new();
    manager.add(addr, tokio::spawn(async {}).abort_handle(), token, true, None);

    let bi = manager.connections.get(&key_of(addr)).unwrap();
    let entry = bi.clients.get(&addr).unwrap().value().clone();
    let id = vec![7u8, 8, 9];

    manager.index_node(id.clone(), entry);
    assert!(manager.index_by_id.contains_key(&id));

    manager.deindex_node(&id);
    assert!(!manager.index_by_id.contains_key(&id));
}

#[tokio::test]
async fn test_find_entry_servers_and_clients() {
    let manager = ConnectionManager::new();
    let client: SocketAddr = "6.6.6.6:6000".parse().unwrap();
    let server: SocketAddr = "6.6.6.6:6001".parse().unwrap();
    let token = CancellationToken::new();
    manager.add(client, tokio::spawn(async {}).abort_handle(), token.clone(), true, None);
    manager.add(server, tokio::spawn(async {}).abort_handle(), token, false, None);

    assert!(manager.find_entry(&client).is_some());
    assert!(manager.find_entry(&server).is_some());
    let missing: SocketAddr = "6.6.6.6:9999".parse().unwrap();
    assert!(manager.find_entry(&missing).is_none());
}

#[tokio::test]
async fn test_cancel_miss_returns_false() {
    let manager = ConnectionManager::new();
    let addr: SocketAddr = "7.7.7.7:7000".parse().unwrap();
    assert!(!manager.cancel_by_addr(addr));
    assert!(!manager.cancel_gracefully(addr));
}

#[tokio::test]
async fn test_remove_missing_no_panic() {
    let manager = ConnectionManager::new();
    let addr: SocketAddr = "9.9.9.9:9000".parse().unwrap();
    manager.remove(addr, true);
}

#[tokio::test]
async fn test_deactivate_keeps_fresh_connections() {
    let manager = ConnectionManager::new();
    let addr: SocketAddr = "8.8.8.8:8000".parse().unwrap();
    let token = CancellationToken::new();
    manager.add(addr, tokio::spawn(async {}).abort_handle(), token, true, None);

    manager.deactivate(3600, 3600);
    assert_eq!(manager.connections.len(), 1);
}

#[tokio::test]
async fn test_add_with_context() {
    let manager = ConnectionManager::new();
    let addr: SocketAddr = "10.10.10.10:10000".parse().unwrap();
    let global = Arc::new(GlobalContext::new(addr, None));
    let ctx = Arc::new(Mutex::new(Context::new(None, None, global, addr)));
    manager.add(
        addr,
        tokio::spawn(async {}).abort_handle(),
        CancellationToken::new(),
        true,
        Some(ctx),
    );

    let bi = manager.connections.get(&key_of(addr)).unwrap();
    assert!(bi.clients.get(&addr).unwrap().context.is_some());
}
