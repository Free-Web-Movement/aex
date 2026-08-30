use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use futures::FutureExt;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use aex::communicators::event::Event;
use aex::connection::context::Context;
use aex::connection::global::GlobalContext;
use aex::connection::heartbeat::HeartbeatConfig;
use aex::connection::node::Node;
use aex::connection::scope::NetworkScope;

#[tokio::test]
async fn test_global_heartbeat_config_and_manager() {
    let addr: SocketAddr = "127.0.0.1:8080".parse().unwrap();
    let mut ctx = GlobalContext::new(addr, None);
    ctx = ctx.with_heartbeat_config(HeartbeatConfig::new().with_interval(5).with_timeout(3));
    assert_eq!(ctx.heartbeat_config.interval_secs, 5);
    assert_eq!(ctx.heartbeat_config.timeout_secs, 3);

    ctx.init_heartbeat_manager();
    assert!(ctx.heartbeat_manager.is_some());
}

#[tokio::test]
async fn test_global_start_heartbeat_no_panic() {
    let addr: SocketAddr = "127.0.0.1:8080".parse().unwrap();
    let mut ctx = GlobalContext::new(addr, None);
    ctx = ctx.with_heartbeat_config(HeartbeatConfig::new().with_interval(1));
    ctx.init_heartbeat_manager();

    let peer: SocketAddr = "127.0.0.1:9999".parse().unwrap();
    let (_resp, writer) = tokio::io::duplex(1024);
    let global = Arc::new(GlobalContext::new(peer, None));
    let cctx = Arc::new(Mutex::new(Context::new(
        None,
        Some(Box::new(writer)),
        global,
        peer,
    )));
    let token = CancellationToken::new();
    ctx.start_heartbeat(cctx, peer, token.clone());
    assert!(ctx.heartbeat_manager.is_some());
}

#[tokio::test]
async fn test_global_pipe_wrapper() {
    let addr: SocketAddr = "127.0.0.1:8080".parse().unwrap();
    let ctx = GlobalContext::new(addr, None);
    let received = Arc::new(Mutex::new(Vec::<String>::new()));
    let rx = received.clone();

    ctx.pipe::<String>(
        "audit",
        Box::new(move |msg: String| {
            let rx = rx.clone();
            (async move {
                rx.lock().await.push(msg);
            })
            .boxed()
        }),
    )
    .await;

    ctx.pipe.send("audit", "hello".to_string()).await.unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(received.lock().await.len(), 1);
}

#[tokio::test]
async fn test_global_spread_wrapper() {
    let addr: SocketAddr = "127.0.0.1:8080".parse().unwrap();
    let ctx = GlobalContext::new(addr, None);
    let counter = Arc::new(AtomicUsize::new(0));
    let c = counter.clone();

    ctx.spread::<usize>(
        "channel",
        Box::new(move |v: usize| {
            let c = c.clone();
            (async move {
                c.fetch_add(v, Ordering::SeqCst);
            })
            .boxed()
        }),
    )
    .await;

    ctx.spread.publish("channel", 10usize).await.unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(counter.load(Ordering::SeqCst), 10);
}

#[tokio::test]
async fn test_global_event_wrapper() {
    let addr: SocketAddr = "127.0.0.1:8080".parse().unwrap();
    let ctx = GlobalContext::new(addr, None);
    let counter = Arc::new(AtomicUsize::new(0));
    let c = counter.clone();

    ctx.event::<String>(
        "app.ready",
        Arc::new(move |_msg: String| {
            let c = c.clone();
            (async move {
                c.fetch_add(1, Ordering::SeqCst);
            })
            .boxed()
        }),
    )
    .await;

    ctx.event
        .notify("app.ready".to_string(), "ok".to_string())
        .await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(counter.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn test_global_get_connection_info() {
    use std::collections::HashSet;

    let addr: SocketAddr = "0.0.0.0:8080".parse().unwrap();
    let ctx = GlobalContext::new(addr, None);

    let inbound_addr: SocketAddr = "1.2.3.4:5000".parse().unwrap();
    let outbound_addr: SocketAddr = "5.6.7.8:6000".parse().unwrap();

    ctx.manager.add(
        inbound_addr,
        tokio::spawn(async {}).abort_handle(),
        CancellationToken::new(),
        true,
        None,
    );
    ctx.manager.add(
        outbound_addr,
        tokio::spawn(async {}).abort_handle(),
        CancellationToken::new(),
        false,
        None,
    );

    {
        let ip = inbound_addr.ip();
        let scope = NetworkScope::from_ip(&ip);
        let bi = ctx.manager.connections.get(&(ip, scope)).unwrap();
        let entry = bi.clients.get(&inbound_addr).unwrap();
        let mut lock = entry.node.write().await;
        *lock = Some(Node {
            id: vec![1, 2, 3],
            version: 1,
            started_at: 0,
            port: 5000,
            protocols: HashSet::new(),
            ips: vec![
                (NetworkScope::Intranet, "10.0.0.5".parse().unwrap()),
                (NetworkScope::Extranet, "1.2.3.4".parse().unwrap()),
            ],
        });
    }

    let info = ctx.get_connection_info().await;
    assert_eq!(info.inbound.len(), 1);
    assert_eq!(info.outbound.len(), 1);

    let inbound = &info.inbound[0];
    assert_eq!(inbound.direction, "inbound");
    assert_eq!(inbound.addr, "1.2.3.4:5000");
    assert!(inbound.local_addr.is_none());
    assert_eq!(inbound.node_id.as_deref(), Some("\u{1}\u{2}\u{3}"));
    assert_eq!(inbound.intranet_ips, vec!["10.0.0.5:5000"]);
    assert_eq!(inbound.wan_ips, vec!["1.2.3.4:5000"]);

    let outbound = &info.outbound[0];
    assert_eq!(outbound.direction, "outbound");
    assert_eq!(outbound.node_id, None);
    assert!(outbound.intranet_ips.is_empty());
    assert!(outbound.wan_ips.is_empty());
}

#[tokio::test]
async fn test_global_shutdown_all_cancels_exits() {
    let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
    let ctx = GlobalContext::new(addr, None);

    // 注册一个 exit，其 token 应被 shutdown_all 取消
    let token = CancellationToken::new();
    let handle = tokio::spawn(async {});
    ctx.add_exit("svc", token.clone(), handle.abort_handle())
        .await;

    let exits = ctx.get_exits().await;
    assert_eq!(exits, vec!["svc"]);

    ctx.shutdown_all().await;
    // 取消后 get_exits 应为空
    let exits = ctx.get_exits().await;
    assert!(exits.is_empty());
}

#[tokio::test]
async fn test_global_set_get_overwrite() {
    let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
    let ctx = GlobalContext::new(addr, None);

    ctx.set("first".to_string()).await;
    assert_eq!(ctx.get::<String>().await, Some("first".to_string()));

    // 覆盖同类型
    ctx.set("second".to_string()).await;
    assert_eq!(ctx.get::<String>().await, Some("second".to_string()));

    // 不同类型独立存储
    ctx.set(42u32).await;
    assert_eq!(ctx.get::<String>().await, Some("second".to_string()));
    assert_eq!(ctx.get::<u32>().await, Some(42));
}
