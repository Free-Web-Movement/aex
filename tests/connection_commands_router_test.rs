use aex::connection::commands::CommandId;
use aex::connection::commands::router::CommandRouter;
use aex::connection::context::Context;
use async_lock::Mutex;
use std::net::SocketAddr;
use std::sync::Arc;

#[test]
fn test_command_router_new() {
    let _router = CommandRouter::new();
}

#[test]
fn test_command_router_default() {
    let _router = CommandRouter::default();
}

#[test]
fn test_command_router_register() {
    let mut router = CommandRouter::new();
    router.register(CommandId::Ping, |_ctx, _data, _addr| Ok(()));
    router.register(CommandId::Pong, |_ctx, _data, _addr| Ok(()));
}

#[test]
fn test_command_router_dispatch() {
    let mut router = CommandRouter::new();
    router.register(CommandId::Ping, |_ctx, data, _addr| {
        assert_eq!(data, b"hello");
        Ok(())
    });

    let mut data = vec![0u8; 4];
    data[0..4].copy_from_slice(&CommandId::Ping.as_u32().to_le_bytes());
    data.extend_from_slice(b"hello");

    let ctx = Arc::new(Mutex::new(Context::new(
        None,
        None,
        Arc::new(aex::connection::global::GlobalContext::new(
            "127.0.0.1:0".parse().unwrap(),
            None,
        )),
        "127.0.0.1:0".parse().unwrap(),
    )));
    let addr: SocketAddr = "127.0.0.1:8080".parse().unwrap();

    router.dispatch(ctx, &data, addr).unwrap();
}

#[test]
fn test_command_router_dispatch_data_too_short() {
    let router = CommandRouter::new();
    let ctx = Arc::new(Mutex::new(Context::new(
        None,
        None,
        Arc::new(aex::connection::global::GlobalContext::new(
            "127.0.0.1:0".parse().unwrap(),
            None,
        )),
        "127.0.0.1:0".parse().unwrap(),
    )));
    let addr: SocketAddr = "127.0.0.1:8080".parse().unwrap();

    let result = router.dispatch(ctx, &[1, 2], addr);
    assert!(result.is_err());
}

#[test]
fn test_command_router_dispatch_unknown_command() {
    let router = CommandRouter::new();
    let ctx = Arc::new(Mutex::new(Context::new(
        None,
        None,
        Arc::new(aex::connection::global::GlobalContext::new(
            "127.0.0.1:0".parse().unwrap(),
            None,
        )),
        "127.0.0.1:0".parse().unwrap(),
    )));
    let addr: SocketAddr = "127.0.0.1:8080".parse().unwrap();

    let mut data = vec![0u8; 4];
    data[0..4].copy_from_slice(&99999u32.to_le_bytes());

    let result = router.dispatch(ctx, &data, addr);
    assert!(result.is_err());
}
