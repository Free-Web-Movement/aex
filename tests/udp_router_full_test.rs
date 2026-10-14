use aex::connection::global::GlobalContext;
use aex::tcp::types::{Codec, RawCodec};
use aex::udp::router::Router;
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::net::UdpSocket;
use tokio::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize, bincode_next::Encode, bincode_next::Decode)]
struct TestFrame {
    valid: bool,
    cmd_data: Option<Vec<u8>>,
}
impl Codec for TestFrame {}

#[derive(Debug, Clone, Serialize, Deserialize, bincode_next::Encode, bincode_next::Decode)]
struct TestCommand {
    key: u32,
    data: Vec<u8>,
}
impl Codec for TestCommand {}

/// Decode a `RawCodec`, using its first 4 little-endian bytes as the key.
fn raw_extract(buf: &[u8]) -> anyhow::Result<Option<(u32, Bytes, Bytes)>> {
    let (codec, consumed) = match <RawCodec as Codec>::decode_with_len(buf) {
        Ok(v) => v,
        Err(_) => return Ok(None),
    };
    let key = codec
        .0
        .get(0..4)
        .map(|s| u32::from_le_bytes(s.try_into().unwrap()))
        .unwrap_or(0);
    Ok(Some((
        key,
        Bytes::copy_from_slice(&buf[..consumed]),
        Bytes::copy_from_slice(&codec.0),
    )))
}

/// Decode a TestFrame + nested TestCommand; invalid frame → Err.
fn frame_extract(buf: &[u8]) -> anyhow::Result<Option<(u32, Bytes, Bytes)>> {
    let (frame, _consumed) = match <TestFrame as Codec>::decode_with_len(buf) {
        Ok(v) => v,
        Err(_) => return Ok(None),
    };
    if !frame.valid {
        anyhow::bail!("invalid frame");
    }
    match frame.cmd_data {
        Some(ref data) => {
            let command = <TestCommand as Codec>::decode(data)?;
            Ok(Some((
                command.key,
                Bytes::copy_from_slice(buf),
                Bytes::copy_from_slice(data),
            )))
        }
        None => Ok(Some((0, Bytes::copy_from_slice(buf), Bytes::new()))),
    }
}

#[test]
fn udp_router_new_is_empty() {
    let router = Router::new();
    assert!(router.handlers.is_empty());
    assert_eq!(router.handler_count(), 0);
    assert!(router.get_extractor().is_none());
}

#[test]
fn udp_router_extractor_set_and_get() {
    let router = Router::new().extractor(raw_extract);
    assert!(router.get_extractor().is_some());
}

#[test]
fn udp_router_on_registers_handler() {
    let mut router = Router::new();
    router.on(
        7,
        |_g: Arc<GlobalContext>, _f: Bytes, _c: Bytes, _a: SocketAddr, _s: Arc<UdpSocket>| async move {
            Ok::<bool, anyhow::Error>(true)
        },
    );
    assert_eq!(router.handler_count(), 1);
}

#[tokio::test]
async fn udp_router_handle_without_extractor_errors() {
    let global = Arc::new(GlobalContext::new("127.0.0.1:0".parse().unwrap(), None));
    let socket = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
    let router = Arc::new(Router::new());
    assert!(router.handle(global, socket).await.is_err());
}

#[tokio::test]
async fn udp_router_handle_dispatches_frame() {
    let global = Arc::new(GlobalContext::new("127.0.0.1:0".parse().unwrap(), None));
    let socket = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
    let target = socket.local_addr().unwrap();

    let counter = Arc::new(AtomicUsize::new(0));
    let c = counter.clone();
    let mut router = Router::new().extractor(raw_extract);
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
    client
        .send_to(&payload.encode().unwrap(), target)
        .await
        .unwrap();

    tokio::time::timeout(Duration::from_secs(2), async {
        while counter.load(Ordering::SeqCst) == 0 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("handler never ran");
    assert_eq!(counter.load(Ordering::SeqCst), 1);
    task.abort();
}

#[tokio::test]
async fn udp_router_handle_dispatches_nested_command() {
    let global = Arc::new(GlobalContext::new("127.0.0.1:0".parse().unwrap(), None));
    let socket = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
    let target = socket.local_addr().unwrap();

    let counter = Arc::new(AtomicUsize::new(0));
    let c = counter.clone();
    let mut router = Router::new().extractor(frame_extract);
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
        cmd_data: Some(
            TestCommand {
                key: 7,
                data: vec![],
            }
            .encode()
            .unwrap(),
        ),
    };
    client.send_to(&f.encode().unwrap(), target).await.unwrap();

    tokio::time::timeout(Duration::from_secs(2), async {
        while counter.load(Ordering::SeqCst) == 0 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("handler never ran");
    assert_eq!(counter.load(Ordering::SeqCst), 1);
    task.abort();
}

#[tokio::test]
async fn udp_router_handle_discards_garbage() {
    let global = Arc::new(GlobalContext::new("127.0.0.1:0".parse().unwrap(), None));
    let socket = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
    let target = socket.local_addr().unwrap();
    let router = Arc::new(Router::new().extractor(frame_extract));

    let task = tokio::spawn({
        let router = router.clone();
        let global = global.clone();
        let socket = socket.clone();
        async move { router.handle(global, socket).await }
    });
    let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    client
        .send_to(b"\xde\xad\xbe\xef garbage", target)
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    task.abort();
}

#[tokio::test]
async fn udp_router_handle_invalid_frame_no_dispatch() {
    let global = Arc::new(GlobalContext::new("127.0.0.1:0".parse().unwrap(), None));
    let socket = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
    let target = socket.local_addr().unwrap();

    let counter = Arc::new(AtomicUsize::new(0));
    let c = counter.clone();
    let mut router = Router::new().extractor(frame_extract);
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
    let bad = TestFrame {
        valid: false,
        cmd_data: None,
    };
    client
        .send_to(&bad.encode().unwrap(), target)
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(counter.load(Ordering::SeqCst), 0);
    task.abort();
}
