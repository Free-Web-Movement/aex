use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use bytes::Bytes;
use futures::future::BoxFuture;

use aex::connection::context::Context;
use aex::connection::global::GlobalContext;
use aex::http::types::Executor;
use aex::protocols::http1::Http11Detector;
use aex::unified::handler::{
    CommandKey, CommandRegistry, HandlerState, Outcome, ProtocolHandler, take_context,
};
use aex::unified::{Claim, DetectorMode, Protocol, Server, Transport, UnifiedServer};

fn test_ctx() -> Context {
    let addr: SocketAddr = "127.0.0.1:9000".parse().unwrap();
    let global = Arc::new(GlobalContext::new(addr, None));
    Context::new(None, None, global, addr)
}

fn exec(flag: Arc<AtomicBool>, result: bool) -> Arc<Executor> {
    Arc::new(move |_ctx: &mut Context| -> BoxFuture<'_, bool> {
        let flag = flag.clone();
        Box::pin(async move {
            flag.store(true, Ordering::SeqCst);
            result
        })
    })
}

#[test]
fn handler_state_records_path_and_current() {
    let mut state = HandlerState::new();
    assert!(state.current().is_none());
    state.enter("http");
    state.enter("websocket");
    assert_eq!(
        state.path,
        vec!["http".to_string(), "websocket".to_string()]
    );
    assert_eq!(state.current(), Some("websocket"));
    assert!(!state.closed);
    state.close();
    assert!(state.closed);
}

#[test]
fn handler_state_scratch_roundtrip() {
    let mut state = HandlerState::new();
    assert_eq!(state.get_scratch::<u32>(), None);
    state.set_scratch(7u32);
    assert_eq!(state.get_scratch::<u32>(), Some(7));
    if let Some(v) = state.get_scratch_mut::<u32>() {
        *v += 1;
    }
    assert_eq!(state.get_scratch::<u32>(), Some(8));
}

#[test]
fn outcome_is_finished_only_for_done_and_unhandled() {
    assert!(Outcome::Done.is_finished());
    assert!(Outcome::Unhandled.is_finished());
    assert!(!Outcome::Handoff("ws".to_string()).is_finished());
}

#[test]
fn command_key_labels_by_name_and_id() {
    assert_eq!(CommandKey::from("ping").label(), "ping");
    assert_eq!(CommandKey::from("ping".to_string()).label(), "ping");
    assert_eq!(CommandKey::from(42u32).label(), "42");
}

#[tokio::test]
async fn handler_run_records_state_path() {
    let handler: Arc<ProtocolHandler<Bytes>> = Arc::new(ProtocolHandler::new(
        "root",
        |_h: &ProtocolHandler<Bytes>, _ctx: &mut Context| -> BoxFuture<'_, Outcome> {
            Box::pin(async { Outcome::Done })
        },
    ));

    let mut ctx = test_ctx();
    ctx.local.set_value(HandlerState::new());
    assert_eq!(handler.run(&mut ctx).await, Outcome::Done);
    let state = ctx.local.get_ref::<HandlerState>().unwrap();
    assert_eq!(state.path, vec!["root".to_string()]);
}

#[tokio::test]
async fn middleware_false_short_circuits_handler() {
    let middleware_ran = Arc::new(AtomicBool::new(false));
    let handler_ran = Arc::new(AtomicBool::new(false));

    let h = handler_ran.clone();
    let handler: Arc<ProtocolHandler<Bytes>> = Arc::new(
        ProtocolHandler::new(
            "root",
            move |_h: &ProtocolHandler<Bytes>, _ctx: &mut Context| -> BoxFuture<'_, Outcome> {
                let h = h.clone();
                Box::pin(async move {
                    h.store(true, Ordering::SeqCst);
                    Outcome::Done
                })
            },
        )
        .with_middleware(exec(middleware_ran.clone(), false)),
    );

    let mut ctx = test_ctx();
    ctx.local.set_value(HandlerState::new());
    assert_eq!(handler.run(&mut ctx).await, Outcome::Done);
    assert!(middleware_ran.load(Ordering::SeqCst));
    assert!(!handler_ran.load(Ordering::SeqCst));
}

#[tokio::test]
async fn middleware_true_lets_handler_run() {
    let middleware_ran = Arc::new(AtomicBool::new(false));
    let handler_ran = Arc::new(AtomicBool::new(false));

    let h = handler_ran.clone();
    let handler: Arc<ProtocolHandler<Bytes>> = Arc::new(
        ProtocolHandler::new(
            "root",
            move |_h: &ProtocolHandler<Bytes>, _ctx: &mut Context| -> BoxFuture<'_, Outcome> {
                let h = h.clone();
                Box::pin(async move {
                    h.store(true, Ordering::SeqCst);
                    Outcome::Done
                })
            },
        )
        .with_middleware(exec(middleware_ran.clone(), true)),
    );

    let mut ctx = test_ctx();
    ctx.local.set_value(HandlerState::new());
    assert_eq!(handler.run(&mut ctx).await, Outcome::Done);
    assert!(middleware_ran.load(Ordering::SeqCst));
    assert!(handler_ran.load(Ordering::SeqCst));
}

#[tokio::test]
async fn handoff_descends_into_named_sub_handler() {
    let child: Arc<ProtocolHandler<Bytes>> = Arc::new(ProtocolHandler::new(
        "websocket",
        |_h: &ProtocolHandler<Bytes>, _ctx: &mut Context| -> BoxFuture<'_, Outcome> {
            Box::pin(async { Outcome::Done })
        },
    ));
    let parent: Arc<ProtocolHandler<Bytes>> = Arc::new(
        ProtocolHandler::new(
            "http",
            |_h: &ProtocolHandler<Bytes>, _ctx: &mut Context| -> BoxFuture<'_, Outcome> {
                Box::pin(async { Outcome::Handoff("websocket".to_string()) })
            },
        )
        .sub(child),
    );

    let mut ctx = test_ctx();
    ctx.local.set_value(HandlerState::new());
    assert_eq!(parent.run(&mut ctx).await, Outcome::Done);
    let state = ctx.local.get_ref::<HandlerState>().unwrap();
    assert_eq!(
        state.path,
        vec!["http".to_string(), "websocket".to_string()]
    );
}

#[tokio::test]
async fn handoff_to_unknown_sub_handler_finishes() {
    let parent: Arc<ProtocolHandler<Bytes>> = Arc::new(ProtocolHandler::new(
        "http",
        |_h: &ProtocolHandler<Bytes>, _ctx: &mut Context| -> BoxFuture<'_, Outcome> {
            Box::pin(async { Outcome::Handoff("missing".to_string()) })
        },
    ));

    let mut ctx = test_ctx();
    ctx.local.set_value(HandlerState::new());
    assert_eq!(parent.run(&mut ctx).await, Outcome::Done);
}

#[tokio::test]
async fn command_registry_dispatch_records_and_runs() {
    let calls = Arc::new(AtomicUsize::new(0));
    let c = calls.clone();
    let mut registry: CommandRegistry<Bytes> = CommandRegistry::new();
    registry.register(
        "ping",
        Arc::new(
            move |_ctx: &mut Context, payload: Bytes| -> BoxFuture<'_, Outcome> {
                let c = c.clone();
                Box::pin(async move {
                    assert_eq!(payload, Bytes::from_static(b"x"));
                    c.fetch_add(1, Ordering::SeqCst);
                    Outcome::Done
                })
            },
        ),
    );
    assert_eq!(registry.len(), 1);
    assert!(!registry.is_empty());
    assert!(registry.contains(&CommandKey::from("ping")));

    let mut ctx = test_ctx();
    ctx.local.set_value(HandlerState::new());
    let outcome = registry
        .dispatch(&mut ctx, CommandKey::from("ping"), Bytes::from_static(b"x"))
        .await;
    assert_eq!(outcome, Outcome::Done);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let state = ctx.local.get_ref::<HandlerState>().unwrap();
    assert_eq!(state.commands.get("ping"), Some(&1));
}

#[tokio::test]
async fn command_registry_dispatch_unknown_is_unhandled() {
    let registry: CommandRegistry<Bytes> = CommandRegistry::new();
    let mut ctx = test_ctx();
    ctx.local.set_value(HandlerState::new());
    assert_eq!(
        registry
            .dispatch(&mut ctx, CommandKey::from(9u32), Bytes::new())
            .await,
        Outcome::Unhandled
    );
}

#[test]
fn protocol_handler_reports_its_detector() {
    let handler: Arc<ProtocolHandler<Bytes>> = Arc::new(
        ProtocolHandler::new(
            "http",
            |_h: &ProtocolHandler<Bytes>, _ctx: &mut Context| -> BoxFuture<'_, Outcome> {
                Box::pin(async { Outcome::Done })
            },
        )
        .with_detector(Arc::new(Http11Detector)),
    );
    assert!(handler.detector().is_some());
    assert_eq!(handler.name(), "http");
    assert!(handler.middleware().is_empty());
    assert!(handler.sub_handlers().is_empty());
    assert!(handler.commands().is_empty());
}

#[test]
fn take_context_moves_reader_writer_and_local_map() {
    let mut ctx = test_ctx();
    ctx.local_addr = Some("127.0.0.1:9001".parse().unwrap());
    ctx.local.set_value(5u32);

    let taken = take_context(&mut ctx);
    assert_eq!(taken.local_addr, Some("127.0.0.1:9001".parse().unwrap()));
    assert_eq!(taken.local.get_ref::<u32>(), Some(&5));
    assert!(ctx.reader.is_none());
    assert!(ctx.writer.is_none());
    assert!(ctx.local.get_ref::<u32>().is_none());
}

#[test]
fn server_new_push_is_chainable_and_reachable() {
    let addr: SocketAddr = "127.0.0.1:9002".parse().unwrap();
    let globals = Arc::new(GlobalContext::new(addr, None));

    let a: Arc<ProtocolHandler<Bytes>> = Arc::new(ProtocolHandler::new(
        "proto-a",
        |_h: &ProtocolHandler<Bytes>, _ctx: &mut Context| -> BoxFuture<'_, Outcome> {
            Box::pin(async { Outcome::Done })
        },
    ));
    let b: Arc<ProtocolHandler<Bytes>> = Arc::new(ProtocolHandler::new(
        "proto-b",
        |_h: &ProtocolHandler<Bytes>, _ctx: &mut Context| -> BoxFuture<'_, Outcome> {
            Box::pin(async { Outcome::Done })
        },
    ));

    // Ergonomic form: `Server::new(gctx).push(..).push(..)`.
    let server = Server::new(globals.clone()).push(a).push(b);
    assert!(server.protocols.iter().any(|p| p.name() == "proto-a"));
    assert!(server.protocols.iter().any(|p| p.name() == "proto-b"));
    assert!(!server.protocols.iter().any(|p| p.name() == "proto-c"));

    // Legacy alias + `at(addr, globals)` still works.
    let legacy = UnifiedServer::at(addr, globals);
    assert!(!legacy.protocols.iter().any(|p| p.name() == "proto-a"));
}

#[test]
fn command_registry_default_is_empty() {
    let r: CommandRegistry<Bytes> = CommandRegistry::default();
    assert!(r.is_empty());
    assert_eq!(r.len(), 0);
}

#[test]
fn handler_is_mine_decides_by_default_detector_and_name() {
    let handler = ProtocolHandler::new(
        "http",
        |_h: &ProtocolHandler<Bytes>, _ctx: &mut Context| -> BoxFuture<'_, Outcome> {
            Box::pin(async { Outcome::Done })
        },
    );
    assert!(!handler.is_default());
    assert!(!handler.is_mine(None));

    let claim = Claim {
        detector: "http11".to_string(),
        protocol: "http".to_string(),
        mode: DetectorMode::Standard,
    };
    // Matches by plain protocol label.
    assert!(handler.is_mine(Some(&claim)));
    // Does not match when neither detector nor label agree.
    let other = Claim {
        detector: "tcp".to_string(),
        protocol: "raw".to_string(),
        mode: DetectorMode::Standard,
    };
    assert!(!handler.is_mine(Some(&other)));

    let defaulted = ProtocolHandler::new(
        "catchall",
        |_h: &ProtocolHandler<Bytes>, _ctx: &mut Context| -> BoxFuture<'_, Outcome> {
            Box::pin(async { Outcome::Done })
        },
    )
    .with_default(true);
    assert!(defaulted.is_default());
    assert!(defaulted.is_mine(None));
}

#[tokio::test]
async fn handler_datagram_flag_and_transport() {
    let handler: Arc<ProtocolHandler<Bytes>> = Arc::new(
        ProtocolHandler::new(
            "dtg",
            |_h: &ProtocolHandler<Bytes>, _ctx: &mut Context| -> BoxFuture<'_, Outcome> {
                Box::pin(async { Outcome::Done })
            },
        )
        .with_datagram(true),
    );
    assert!(handler.is_datagram());
    let p: &dyn Protocol = handler.as_ref();
    assert_eq!(p.transport(), Transport::Datagram);
}

#[tokio::test]
async fn handler_with_command_and_commands_mut_register() {
    let mut base = ProtocolHandler::new(
        "cmd",
        |_h: &ProtocolHandler<Bytes>, _ctx: &mut Context| -> BoxFuture<'_, Outcome> {
            Box::pin(async { Outcome::Done })
        },
    );
    base.commands_mut().register(
        "alpha",
        Arc::new(
            |_ctx: &mut Context, _payload: Bytes| -> BoxFuture<'_, Outcome> {
                Box::pin(async { Outcome::Done })
            },
        ),
    );
    let handler = base
        .with_command(
            "beta",
            Arc::new(
                |_ctx: &mut Context, _payload: Bytes| -> BoxFuture<'_, Outcome> {
                    Box::pin(async { Outcome::Done })
                },
            ),
        )
        .shared();

    assert!(handler.commands().contains(&CommandKey::from("alpha")));
    assert!(handler.commands().contains(&CommandKey::from("beta")));
    assert!(!handler.commands().contains(&CommandKey::from("gamma")));
    assert_eq!(handler.commands().len(), 2);
}

#[tokio::test]
async fn protocol_trait_delegates_whole_handler() {
    let mw_ran = Arc::new(AtomicBool::new(false));
    let child: Arc<ProtocolHandler<Bytes>> = Arc::new(ProtocolHandler::new(
        "kid",
        |_h: &ProtocolHandler<Bytes>, _ctx: &mut Context| -> BoxFuture<'_, Outcome> {
            Box::pin(async { Outcome::Done })
        },
    ));
    let handler = Arc::new(
        ProtocolHandler::new(
            "delegate",
            |_h: &ProtocolHandler<Bytes>, _ctx: &mut Context| -> BoxFuture<'_, Outcome> {
                Box::pin(async { Outcome::Done })
            },
        )
        .with_middleware(exec(mw_ran.clone(), true))
        .sub(child)
        .with_command(
            "k",
            Arc::new(
                |_ctx: &mut Context, _payload: Bytes| -> BoxFuture<'_, Outcome> {
                    Box::pin(async { Outcome::Done })
                },
            ),
        )
        .with_buffered_writer(true),
    );

    let p: &dyn Protocol = handler.as_ref();
    assert_eq!(p.name(), "delegate");
    assert_eq!(p.middleware().len(), 1);
    assert_eq!(p.sub_handlers().len(), 1);
    assert_eq!(p.commands().len(), 1);
    assert!(p.buffered_writer());
    assert_eq!(p.transport(), Transport::Stream);
    assert!(!p.is_default());
}
