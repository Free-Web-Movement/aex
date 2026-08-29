use aex::connection::context::Context;
use aex::connection::global::GlobalContext;
use aex::tcp::router::{Doer, Router};
use aex::tcp::types::{Codec, Command, Frame};
use futures::future::BoxFuture;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncWriteExt, BufReader};
use tokio::sync::Mutex;

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

fn cmd(key: u32) -> TestCommand {
    TestCommand {
        key,
        data: vec![1, 2, 3],
    }
}
fn frame(cmd_data: Option<Vec<u8>>) -> TestFrame {
    TestFrame {
        valid: true,
        flat: false,
        cmd_data,
    }
}
fn ctx_none() -> Arc<Mutex<Context>> {
    let global = Arc::new(GlobalContext::new(
        "127.0.0.1:0".parse().unwrap(),
        None,
    ));
    Arc::new(Mutex::new(Context::new(
        None,
        None,
        global,
        "127.0.0.1:0".parse().unwrap(),
    )))
}
fn doer(result: bool) -> Doer<TestFrame, TestCommand> {
    Box::new(
        move |_ctx: Arc<Mutex<Context>>,
              _f: TestFrame,
              _c: TestCommand|
              -> BoxFuture<'static, anyhow::Result<bool>> {
            Box::pin(async move { Ok(result) })
        },
    )
}

#[test]
fn tcp_router_new_is_empty() {
    let router = Router::<TestFrame, TestCommand>::new();
    assert!(router.handlers.is_empty());
    assert!(router.get_extractor().is_none());
}

#[test]
fn tcp_router_extractor_set_and_get() {
    let router = Router::<TestFrame, TestCommand>::new().extractor(|c: &TestCommand| c.key);
    assert!(router.get_extractor().is_some());
}

#[test]
fn tcp_router_on_and_on_simple_register() {
    let mut router = Router::<TestFrame, TestCommand>::new();
    router.on(1, doer(true), Vec::new());
    router.on_simple(2, doer(true));
    assert_eq!(router.handlers.len(), 2);
    assert_eq!(router.handlers.get(&2).unwrap().len(), 1);
}

#[test]
fn tcp_router_on_middleware_chain_build() {
    let mut router = Router::<TestFrame, TestCommand>::new();
    router.on(1, doer(true), vec![doer(true), doer(true)]);
    assert_eq!(router.handlers.get(&1).unwrap().len(), 3);
}

#[tokio::test]
async fn handle_frame_without_extractor_errors() {
    let router = Router::<TestFrame, TestCommand>::new();
    let res = router
        .handle_frame(ctx_none(), frame(Some(cmd(1).encode().unwrap())))
        .await;
    assert!(res.is_err());
}

#[tokio::test]
async fn handle_frame_invalid_frame_returns_false() {
    let router = Router::<TestFrame, TestCommand>::new().extractor(|c: &TestCommand| c.key);
    let bad = TestFrame {
        valid: false,
        flat: false,
        cmd_data: Some(cmd(1).encode().unwrap()),
    };
    assert!(!router.handle_frame(ctx_none(), bad).await.unwrap());
}

#[tokio::test]
async fn handle_frame_no_command_returns_true() {
    let router = Router::<TestFrame, TestCommand>::new().extractor(|c: &TestCommand| c.key);
    let f = TestFrame {
        valid: true,
        flat: false,
        cmd_data: None,
    };
    assert!(router.handle_frame(ctx_none(), f).await.unwrap());
}

#[tokio::test]
async fn handle_frame_flat_skips_dispatch() {
    let counter = Arc::new(AtomicUsize::new(0));
    let c = counter.clone();
    let mut router = Router::<TestFrame, TestCommand>::new().extractor(|c: &TestCommand| c.key);
    let handler: Doer<TestFrame, TestCommand> = Box::new(move |_ctx, _f, _c| {
        let c = c.clone();
        Box::pin(async move {
            c.fetch_add(1, Ordering::SeqCst);
            Ok(true)
        })
    });
    router.on(1, handler, Vec::new());
    let f = TestFrame {
        valid: true,
        flat: true,
        cmd_data: Some(cmd(1).encode().unwrap()),
    };
    assert!(router.handle_frame(ctx_none(), f).await.unwrap());
    assert_eq!(counter.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn handle_frame_decode_failure_returns_false() {
    let router = Router::<TestFrame, TestCommand>::new().extractor(|c: &TestCommand| c.key);
    let f = frame(Some(vec![0xde, 0xad]));
    assert!(!router.handle_frame(ctx_none(), f).await.unwrap());
}

#[tokio::test]
async fn handle_frame_no_handler_returns_true() {
    let mut router = Router::<TestFrame, TestCommand>::new().extractor(|c: &TestCommand| c.key);
    router.on_simple(99, doer(true));
    assert!(
        router
            .handle_frame(ctx_none(), frame(Some(cmd(1).encode().unwrap())))
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn handle_frame_dispatch_calls_handler() {
    let counter = Arc::new(AtomicUsize::new(0));
    let counter_clone = counter.clone();
    let mut router = Router::<TestFrame, TestCommand>::new().extractor(|c: &TestCommand| c.key);
    let handler: Doer<TestFrame, TestCommand> = Box::new(move |_ctx, _f, _cmd| {
        let counter_clone = counter_clone.clone();
        Box::pin(async move {
            assert_eq!(_cmd.key, 1);
            counter_clone.fetch_add(1, Ordering::SeqCst);
            Ok(true)
        })
    });
    router.on(1, handler, Vec::new());
    assert!(
        router
            .handle_frame(ctx_none(), frame(Some(cmd(1).encode().unwrap())))
            .await
            .unwrap()
    );
    assert_eq!(counter.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn handle_frame_handler_false_shortcircuits() {
    let mut router = Router::<TestFrame, TestCommand>::new().extractor(|c: &TestCommand| c.key);
    router.on(1, doer(false), Vec::new());
    assert!(
        !router
            .handle_frame(ctx_none(), frame(Some(cmd(1).encode().unwrap())))
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn handle_frame_handler_error_propagates() {
    let mut router = Router::<TestFrame, TestCommand>::new().extractor(|c: &TestCommand| c.key);
    let handler: Doer<TestFrame, TestCommand> =
        Box::new(|_ctx, _f, _c| Box::pin(async move { Err(anyhow::anyhow!("boom")) }));
    router.on(1, handler, Vec::new());
    assert!(
        router
            .handle_frame(ctx_none(), frame(Some(cmd(1).encode().unwrap())))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn handle_frame_middlewares_run_in_order_then_handler() {
    let order = Arc::new(std::sync::Mutex::new(Vec::new()));
    let make =
        |name: &'static str, order: Arc<std::sync::Mutex<Vec<&'static str>>>| -> Doer<TestFrame, TestCommand> {
            Box::new(move |_ctx, _f, _c| {
                let order = order.clone();
                Box::pin(async move {
                    order.lock().unwrap().push(name);
                    Ok(true)
                })
            })
        };
    let mut router = Router::<TestFrame, TestCommand>::new().extractor(|c: &TestCommand| c.key);
    router.on(
        1,
        make("handler", order.clone()),
        vec![make("mw1", order.clone()), make("mw2", order.clone())],
    );
    assert!(
        router
            .handle_frame(ctx_none(), frame(Some(cmd(1).encode().unwrap())))
            .await
            .unwrap()
    );
    assert_eq!(*order.lock().unwrap(), vec!["mw1", "mw2", "handler"]);
}

#[tokio::test]
async fn handle_without_extractor_errors() {
    let router = Router::<TestFrame, TestCommand>::new();
    assert!(router.handle(ctx_none()).await.is_err());
}

#[tokio::test]
async fn handle_without_reader_returns_ok() {
    let router = Router::<TestFrame, TestCommand>::new().extractor(|c: &TestCommand| c.key);
    assert!(router.handle(ctx_none()).await.is_ok());
}

#[tokio::test]
async fn handle_reads_single_frame() {
    let global = Arc::new(GlobalContext::new(
        "127.0.0.1:0".parse().unwrap(),
        None,
    ));
    let (mut client, server) = tokio::io::duplex(8192);
    let mut router = Router::<TestFrame, TestCommand>::new().extractor(|c: &TestCommand| c.key);
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
    let c = counter.clone();
    let mut router = Router::<TestFrame, TestCommand>::new().extractor(|c: &TestCommand| c.key);
    let handler: Doer<TestFrame, TestCommand> = Box::new(move |_ctx, _f, _c| {
        let c = c.clone();
        Box::pin(async move {
            c.fetch_add(1, Ordering::SeqCst);
            Ok(true)
        })
    });
    router.on(1, handler, Vec::new());

    let global = Arc::new(GlobalContext::new(
        "127.0.0.1:0".parse().unwrap(),
        None,
    ));
    let (mut client, server) = tokio::io::duplex(16384);
    let f1 = frame(Some(cmd(1).encode().unwrap())).encode().unwrap();
    let f2 = frame(Some(cmd(1).encode().unwrap())).encode().unwrap();
    client.write_all(&f1).await.unwrap();
    client.write_all(&f2).await.unwrap();
    drop(client);
    let ctx = Arc::new(Mutex::new(Context::new(
        Some(Box::new(BufReader::new(server))),
        None,
        global,
        "127.0.0.1:0".parse().unwrap(),
    )));
    router.handle(ctx).await.unwrap();
    assert_eq!(counter.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn handle_stops_when_handler_returns_false() {
    let counter = Arc::new(AtomicUsize::new(0));
    let c = counter.clone();
    let mut router = Router::<TestFrame, TestCommand>::new().extractor(|c: &TestCommand| c.key);
    let handler: Doer<TestFrame, TestCommand> = Box::new(move |_ctx, _f, _c| {
        let c = c.clone();
        Box::pin(async move {
            c.fetch_add(1, Ordering::SeqCst);
            Ok(false)
        })
    });
    router.on(1, handler, Vec::new());

    let global = Arc::new(GlobalContext::new(
        "127.0.0.1:0".parse().unwrap(),
        None,
    ));
    let (mut client, server) = tokio::io::duplex(8192);
    let f1 = frame(Some(cmd(1).encode().unwrap())).encode().unwrap();
    let f2 = frame(Some(cmd(1).encode().unwrap())).encode().unwrap();
    client.write_all(&f1).await.unwrap();
    client.write_all(&f2).await.unwrap();
    drop(client);
    let ctx = Arc::new(Mutex::new(Context::new(
        Some(Box::new(BufReader::new(server))),
        None,
        global,
        "127.0.0.1:0".parse().unwrap(),
    )));
    router.handle(ctx).await.unwrap();
    assert_eq!(counter.load(Ordering::SeqCst), 1);
}
