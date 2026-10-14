use aex::connection::context::Context;
use aex::connection::global::{FrameErrorPolicy, GlobalContext};
use aex::tcp::router::{Doer, Router};
use aex::tcp::types::Codec;
use bytes::Bytes;
use futures::future::BoxFuture;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::io::{AsyncWriteExt, BufReader};
use tokio::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize, bincode::Encode, bincode::Decode)]
struct TestFrame {
    valid: bool,
    cmd_data: Option<Vec<u8>>,
}
impl Codec for TestFrame {}

#[derive(Debug, Clone, Serialize, Deserialize, bincode::Encode, bincode::Decode)]
struct TestCommand {
    key: u32,
    data: Vec<u8>,
}
impl Codec for TestCommand {}

fn cmd(key: u32) -> TestCommand {
    TestCommand {
        key,
        data: vec![1, 2, 3],
    }
}
fn frame(cmd_data: Option<Vec<u8>>) -> TestFrame {
    TestFrame {
        valid: true,
        cmd_data,
    }
}

/// Extractor used by the tests: decodes a `TestFrame`, validates it (invalid →
/// `Err`, so the policy decides whether to drop or close), then decodes the
/// nested `TestCommand` and uses its key for dispatch.
fn extract(buf: &[u8]) -> anyhow::Result<Option<(usize, u32, Bytes, Bytes)>> {
    let (frame, consumed) = match <TestFrame as Codec>::decode_with_len(buf) {
        Ok(v) => v,
        Err(_) => return Ok(None),
    };
    if !frame.valid {
        anyhow::bail!("invalid frame");
    }
    let frame_bytes = Bytes::copy_from_slice(&buf[..consumed]);
    match frame.cmd_data {
        Some(ref data) => {
            let command = <TestCommand as Codec>::decode(data)?;
            Ok(Some((
                consumed,
                command.key,
                frame_bytes,
                Bytes::copy_from_slice(data),
            )))
        }
        None => Ok(Some((consumed, 0, frame_bytes, Bytes::new()))),
    }
}

fn ctx_none() -> Arc<Mutex<Context>> {
    ctx_with_policy(FrameErrorPolicy::Close)
}
fn ctx_with_policy(policy: FrameErrorPolicy) -> Arc<Mutex<Context>> {
    let global = Arc::new(
        GlobalContext::new("127.0.0.1:0".parse().unwrap(), None).with_frame_error_policy(policy),
    );
    Arc::new(Mutex::new(Context::new(
        None,
        None,
        global,
        "127.0.0.1:0".parse().unwrap(),
    )))
}
fn doer(result: bool) -> Doer {
    Box::new(
        move |_ctx: Arc<Mutex<Context>>,
              _f: Bytes,
              _c: Bytes|
              -> BoxFuture<'static, anyhow::Result<bool>> {
            Box::pin(async move { Ok(result) })
        },
    )
}

#[test]
fn tcp_router_new_is_empty() {
    let router = Router::new();
    assert!(router.handlers.is_empty());
    assert!(router.get_extractor().is_none());
}

#[test]
fn tcp_router_extractor_set_and_get() {
    let router = Router::new().extractor(extract);
    assert!(router.get_extractor().is_some());
}

#[test]
fn tcp_router_on_and_on_simple_register() {
    let mut router = Router::new();
    router.on(1, doer(true), Vec::new());
    router.on_simple(2, doer(true));
    assert_eq!(router.handlers.len(), 2);
    assert_eq!(router.handlers.get(&2).unwrap().len(), 1);
}

#[test]
fn tcp_router_on_middleware_chain_build() {
    let mut router = Router::new();
    router.on(1, doer(true), vec![doer(true), doer(true)]);
    assert_eq!(router.handlers.get(&1).unwrap().len(), 3);
}

#[tokio::test]
async fn handle_frame_without_extractor_errors() {
    let router = Router::new();
    let bytes = frame(Some(cmd(1).encode().unwrap())).encode().unwrap();
    assert!(router.handle_frame(ctx_none(), &bytes).await.is_err());
}

#[tokio::test]
async fn handle_frame_invalid_frame_errors_by_default() {
    let router = Router::new().extractor(extract);
    let bad = TestFrame {
        valid: false,
        cmd_data: Some(cmd(1).encode().unwrap()),
    };
    let bytes = bad.encode().unwrap();
    assert!(router.handle_frame(ctx_none(), &bytes).await.is_err());
}

#[tokio::test]
async fn handle_frame_invalid_frame_dropped_with_drop_policy() {
    let router = Router::new().extractor(extract);
    let bad = TestFrame {
        valid: false,
        cmd_data: None,
    };
    let bytes = bad.encode().unwrap();
    assert!(
        !router
            .handle_frame(ctx_with_policy(FrameErrorPolicy::Drop), &bytes)
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn handle_frame_no_command_returns_true() {
    let router = Router::new().extractor(extract);
    let bytes = frame(None).encode().unwrap();
    assert!(router.handle_frame(ctx_none(), &bytes).await.unwrap());
}

#[tokio::test]
async fn handle_frame_decode_failure_returns_false() {
    let router = Router::new().extractor(extract);
    assert!(
        !router
            .handle_frame(ctx_none(), &[0xde, 0xad])
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn handle_frame_no_handler_returns_true() {
    let mut router = Router::new().extractor(extract);
    router.on_simple(99, doer(true));
    let bytes = frame(Some(cmd(1).encode().unwrap())).encode().unwrap();
    assert!(router.handle_frame(ctx_none(), &bytes).await.unwrap());
}

#[tokio::test]
async fn handle_frame_dispatch_calls_handler() {
    let counter = Arc::new(AtomicUsize::new(0));
    let counter_clone = counter.clone();
    let mut router = Router::new().extractor(extract);
    let handler: Doer = Box::new(move |_ctx, _f, c| {
        let counter_clone = counter_clone.clone();
        Box::pin(async move {
            let command = TestCommand::decode(&c).unwrap();
            assert_eq!(command.key, 1);
            counter_clone.fetch_add(1, Ordering::SeqCst);
            Ok(true)
        })
    });
    router.on(1, handler, Vec::new());
    let bytes = frame(Some(cmd(1).encode().unwrap())).encode().unwrap();
    assert!(router.handle_frame(ctx_none(), &bytes).await.unwrap());
    assert_eq!(counter.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn handle_frame_handler_false_shortcircuits() {
    let mut router = Router::new().extractor(extract);
    router.on(1, doer(false), Vec::new());
    let bytes = frame(Some(cmd(1).encode().unwrap())).encode().unwrap();
    assert!(!router.handle_frame(ctx_none(), &bytes).await.unwrap());
}

#[tokio::test]
async fn handle_frame_handler_error_propagates() {
    let mut router = Router::new().extractor(extract);
    let handler: Doer =
        Box::new(|_ctx, _f, _c| Box::pin(async move { Err(anyhow::anyhow!("boom")) }));
    router.on(1, handler, Vec::new());
    let bytes = frame(Some(cmd(1).encode().unwrap())).encode().unwrap();
    assert!(router.handle_frame(ctx_none(), &bytes).await.is_err());
}

#[tokio::test]
async fn handle_frame_middlewares_run_in_order_then_handler() {
    let order = Arc::new(std::sync::Mutex::new(Vec::new()));
    let make = |name: &'static str, order: Arc<std::sync::Mutex<Vec<&'static str>>>| -> Doer {
        Box::new(move |_ctx, _f, _c| {
            let order = order.clone();
            Box::pin(async move {
                order.lock().unwrap().push(name);
                Ok(true)
            })
        })
    };
    let mut router = Router::new().extractor(extract);
    router.on(
        1,
        make("handler", order.clone()),
        vec![make("mw1", order.clone()), make("mw2", order.clone())],
    );
    let bytes = frame(Some(cmd(1).encode().unwrap())).encode().unwrap();
    assert!(router.handle_frame(ctx_none(), &bytes).await.unwrap());
    assert_eq!(*order.lock().unwrap(), vec!["mw1", "mw2", "handler"]);
}

#[tokio::test]
async fn handle_without_extractor_errors() {
    let router = Router::new();
    assert!(router.handle(ctx_none()).await.is_err());
}

#[tokio::test]
async fn handle_without_reader_returns_ok() {
    let router = Router::new().extractor(extract);
    assert!(router.handle(ctx_none()).await.is_ok());
}

#[tokio::test]
async fn handle_reads_single_frame() {
    let global = Arc::new(GlobalContext::new("127.0.0.1:0".parse().unwrap(), None));
    let (mut client, server) = tokio::io::duplex(8192);
    let mut router = Router::new().extractor(extract);
    router.on_simple(1, doer(true));
    let data = frame(Some(cmd(1).encode().unwrap())).encode().unwrap();
    client.write_all(&data).await.unwrap();
    drop(client);
    let ctx = Arc::new(Mutex::new(Context::new(
        Some(Box::new(BufReader::new(server))),
        None,
        global,
        "127.0.0.1:0".parse().unwrap(),
    )));
    assert!(router.handle(ctx).await.is_ok());
}

#[tokio::test]
async fn handle_reads_two_frames_sticky() {
    let counter = Arc::new(AtomicUsize::new(0));
    let counter_clone = counter.clone();
    let global = Arc::new(GlobalContext::new("127.0.0.1:0".parse().unwrap(), None));
    let (mut client, server) = tokio::io::duplex(8192);
    let mut router = Router::new().extractor(extract);
    router.on_simple(
        1,
        Box::new(move |_ctx, _f, _c| {
            let counter_clone = counter_clone.clone();
            Box::pin(async move {
                counter_clone.fetch_add(1, Ordering::SeqCst);
                Ok(true)
            })
        }),
    );
    let data = frame(Some(cmd(1).encode().unwrap())).encode().unwrap();
    client.write_all(&data).await.unwrap();
    client.write_all(&data).await.unwrap();
    drop(client);
    let ctx = Arc::new(Mutex::new(Context::new(
        Some(Box::new(BufReader::new(server))),
        None,
        global,
        "127.0.0.1:0".parse().unwrap(),
    )));
    assert!(router.handle(ctx).await.is_ok());
    assert_eq!(counter.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn handle_handler_false_stops_reading() {
    let counter = Arc::new(AtomicUsize::new(0));
    let counter_clone = counter.clone();
    let global = Arc::new(GlobalContext::new("127.0.0.1:0".parse().unwrap(), None));
    let (mut client, server) = tokio::io::duplex(8192);
    let mut router = Router::new().extractor(extract);
    router.on_simple(
        1,
        Box::new(move |_ctx, _f, _c| {
            let counter_clone = counter_clone.clone();
            Box::pin(async move {
                counter_clone.fetch_add(1, Ordering::SeqCst);
                Ok(false)
            })
        }),
    );
    let data = frame(Some(cmd(1).encode().unwrap())).encode().unwrap();
    client.write_all(&data).await.unwrap();
    client.write_all(&data).await.unwrap();
    drop(client);
    let ctx = Arc::new(Mutex::new(Context::new(
        Some(Box::new(BufReader::new(server))),
        None,
        global,
        "127.0.0.1:0".parse().unwrap(),
    )));
    assert!(router.handle(ctx).await.is_ok());
    assert_eq!(counter.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn handle_invalid_frame_drops_with_drop_policy() {
    let global = Arc::new(
        GlobalContext::new("127.0.0.1:0".parse().unwrap(), None)
            .with_frame_error_policy(FrameErrorPolicy::Drop),
    );
    let (mut client, server) = tokio::io::duplex(8192);
    let router = Router::new().extractor(extract);
    let bad = TestFrame {
        valid: false,
        cmd_data: None,
    };
    client.write_all(&bad.encode().unwrap()).await.unwrap();
    drop(client);
    let ctx = Arc::new(Mutex::new(Context::new(
        Some(Box::new(BufReader::new(server))),
        None,
        global,
        "127.0.0.1:0".parse().unwrap(),
    )));
    assert!(router.handle(ctx).await.is_ok());
}

#[tokio::test]
async fn handle_invalid_frame_closes_by_default() {
    let global = Arc::new(GlobalContext::new("127.0.0.1:0".parse().unwrap(), None));
    let (mut client, server) = tokio::io::duplex(8192);
    let router = Router::new().extractor(extract);
    let bad = TestFrame {
        valid: false,
        cmd_data: None,
    };
    client.write_all(&bad.encode().unwrap()).await.unwrap();
    drop(client);
    let ctx = Arc::new(Mutex::new(Context::new(
        Some(Box::new(BufReader::new(server))),
        None,
        global,
        "127.0.0.1:0".parse().unwrap(),
    )));
    assert!(router.handle(ctx).await.is_err());
}
