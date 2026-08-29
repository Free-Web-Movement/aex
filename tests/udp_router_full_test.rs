use aex::connection::global::GlobalContext;
use aex::tcp::types::{Codec, Command, Frame, RawCodec};
use aex::udp::router::Router;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize, bincode::Encode, bincode::Decode)]
struct TestFrame {
    valid: bool,
    flat: bool,
    cmd_data: Option<Vec<u8>>,
}
impl Codec for TestFrame {}
impl Frame for TestFrame {
    fn payload(&self) -> Option<Vec<u8>> {
        self.cmd_data.clone()
    }
    fn validate(&self) -> bool {
        self.valid
    }
    fn command(&self) -> Option<&Vec<u8>> {
        self.cmd_data.as_ref()
    }
    fn is_flat(&self) -> bool {
        self.flat
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, bincode::Encode, bincode::Decode)]
struct TestCommand {
    key: u32,
    data: Vec<u8>,
}
impl Codec for TestCommand {}
impl Command for TestCommand {
    fn id(&self) -> u32 {
        self.key
    }
    fn data(&self) -> &Vec<u8> {
        &self.data
    }
}

fn rawcodec_key(c: &RawCodec) -> u32 {
    c.0.get(0..4)
        .map(|s| u32::from_le_bytes(s.try_into().unwrap()))
        .unwrap_or(0)
}

#[test]
fn udp_router_new_is_empty() {
    let router = Router::<(), ()>::new();
    assert!(router.handlers.is_empty());
    assert_eq!(router.handler_count(), 0);
    assert!(router.get_extractor().is_none());
}

#[test]
fn udp_router_extractor_set_and_get() {
    let router = Router::<(), ()>::new().extractor(|_c: &()| 1u32);
    assert!(router.get_extractor().is_some());
}

#[test]
fn udp_router_new_with_handler_registers_default() {
    let router = Router::<RawCodec, RawCodec>::new_with_handler();
    assert_eq!(router.handler_count(), 1);
}

#[test]
fn udp_router_on_registers_handler() {
    let mut router = Router::<(), ()>::new();
    router.on(7, |_g: Arc<GlobalContext>, _f: (), _c: (), _a: SocketAddr, _s: Arc<UdpSocket>| async move {
        Ok::<bool, anyhow::Error>(true)
    });
    assert_eq!(router.handler_count(), 1);
}

#[tokio::test]
async fn udp_router_handle_without_extractor_errors() {
    let global = Arc::new(GlobalContext::new(
        "127.0.0.1:0".parse().unwrap(),
        None,
    ));
    let socket = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
    let router = Arc::new(Router::<RawCodec, RawCodec>::new());
    assert!(router.handle(global, socket).await.is_err());
}

#[tokio::test]
async fn udp_router_handle_dispatches_flat_frame() {
    let global = Arc::new(GlobalContext::new(
        "127.0.0.1:0".parse().unwrap(),
        None,
    ));
    let socket = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
    let target = socket.local_addr().unwrap();

    let counter = Arc::new(AtomicUsize::new(0));
    let c = counter.clone();
    let mut router = Router::<RawCodec, RawCodec>::new().extractor(rawcodec_key);
    router.on(42, move |_g, _f, _c, _a, _s| {
        let c = c.clone();
        async move {
            c.fetch_add(1, Ordering::SeqCst);
            Ok::<bool, anyhow::Error>(true)
        }
    });

    let task = tokio::spawn({
        let router = Arc::new(router);
        let global = global.clone();
        let socket = socket.clone();
        async move { router.handle(global, socket).await }
    });

    let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let payload = RawCodec(42u32.to_le_bytes().to_vec());
    let encoded = payload.encode().unwrap();
    client.send_to(&encoded, target).await.unwrap();

    tokio::time::timeout(Duration::from_secs(2), async {
        while counter.load(Ordering::SeqCst) == 0 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("flat handler never ran");
    assert_eq!(counter.load(Ordering::SeqCst), 1);
    task.abort();
}

#[tokio::test]
async fn udp_router_handle_dispatches_nonflat_frame() {
    let global = Arc::new(GlobalContext::new(
        "127.0.0.1:0".parse().unwrap(),
        None,
    ));
    let socket = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
    let target = socket.local_addr().unwrap();

    let counter = Arc::new(AtomicUsize::new(0));
    let c = counter.clone();
    let mut router = Router::<TestFrame, TestCommand>::new().extractor(|c: &TestCommand| c.key);
    router.on(7, move |_g, _f, _c, _a, _s| {
        let c = c.clone();
        async move {
            c.fetch_add(1, Ordering::SeqCst);
            Ok::<bool, anyhow::Error>(true)
        }
    });

    let task = tokio::spawn({
        let router = Arc::new(router);
        let global = global.clone();
        let socket = socket.clone();
        async move { router.handle(global, socket).await }
    });

    let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let f = TestFrame {
        valid: true,
        flat: false,
        cmd_data: Some(
            TestCommand {
                key: 7,
                data: vec![],
            }
            .encode()
            .unwrap(),
        ),
    };
    let encoded = f.encode().unwrap();
    client.send_to(&encoded, target).await.unwrap();

    tokio::time::timeout(Duration::from_secs(2), async {
        while counter.load(Ordering::SeqCst) == 0 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("nonflat handler never ran");
    assert_eq!(counter.load(Ordering::SeqCst), 1);
    task.abort();
}

#[tokio::test]
async fn udp_router_handle_discards_garbage() {
    let global = Arc::new(GlobalContext::new(
        "127.0.0.1:0".parse().unwrap(),
        None,
    ));
    let socket = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
    let target = socket.local_addr().unwrap();
    let router = Arc::new(Router::<RawCodec, RawCodec>::new().extractor(rawcodec_key));

    let task = tokio::spawn({
        let router = router.clone();
        let global = global.clone();
        let socket = socket.clone();
        async move { router.handle(global, socket).await }
    });
    let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    client.send_to(b"\xde\xad\xbe\xef garbage", target).await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    task.abort();
}
