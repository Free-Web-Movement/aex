//! NAT 作为 unified server 服务的集成测试（注册 + 检测协作）。
//!
//! 验证：NAT 服务经 `enable_nat` 注册进 unified server 的检测层后，NAT 魔数
//! 连接被正确识别并分派给 nat handler（与 HTTP 同端口共存）。

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use aex::connection::global::GlobalContext;
use aex::nat::{NatRelayService, NatTunnelClient, UnifiedServerExt};
use aex::unified::UnifiedServer;

async fn start_unified_nat_server() -> SocketAddr {
    let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
    let globals = Arc::new(GlobalContext::new(addr, None));
    let service = NatRelayService::new();

    // 绑定端口前先确定实际地址：用预绑定获取空闲端口。
    let probe = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let actual = probe.local_addr().unwrap();
    drop(probe);

    let server = UnifiedServer::new(actual, globals.clone())
        .enable_nat(service)
        // 注册 HTTP 检测器，验证与 NAT 同端口共存。
        .detector(Arc::new(aex::unified::Http11Detector))
        .detector(Arc::new(aex::unified::Http2Detector));
    tokio::spawn(async move {
        let _ = server.start().await;
    });
    // 等待监听就绪。
    tokio::time::sleep(Duration::from_millis(200)).await;
    actual
}

#[tokio::test]
async fn nat_service_detected_and_dispatch_on_shared_port() {
    let addr = start_unified_nat_server().await;

    // 用 NatTunnelClient 连 unified server 的同一端口（NAT 魔数被检测层识别）。
    let client = NatTunnelClient::connect("node_a".to_string(), addr)
        .await
        .expect("client connect failed");
    client
        .wait_ready(Duration::from_secs(5))
        .await
        .expect("register failed");
    assert_eq!(client.public_addr().await.is_some(), true);

    // 第二个 client 也连上（中继登记多个）。
    let b = NatTunnelClient::connect("node_b".to_string(), addr)
        .await
        .expect("client b connect failed");
    b.wait_ready(Duration::from_secs(5)).await.expect("b register failed");

    // 数据经 NAT 服务中继（验证 handler 真正接管连接）。
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<aex::nat::TunnelData>();
    b.set_data_channel(tx).await;
    client.send_to("node_b", b"via-unified-nat".to_vec()).await.expect("send");
    match tokio::time::timeout(Duration::from_secs(3), rx.recv()).await {
        Ok(Some(d)) => assert_eq!(d.payload, b"via-unified-nat".to_vec()),
        other => panic!("expected message via unified nat, got {:?}", other.map(|o| o.is_none())),
    }
}

#[tokio::test]
async fn nat_service_registers_detector_with_correct_protocol() {
    // NatDetector 的 protocol 标签应为 "nat"（分派键）。
    use aex::unified::detect::ProtocolDetector;
    let d = aex::nat::NatDetector;
    assert_eq!(d.protocol(), "nat");
    assert_eq!(d.name(), "nat-detector");
}
