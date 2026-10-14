//! Ergonomic constructors for the built-in protocols.
//!
//! Each macro expands to the same `Struct::new(..).shared()` value you would
//! write by hand, so the result composes with [`Server::push`](crate::server::Server::push)
//! and [`Tcp::sub`](crate::protocols::tcp::Tcp::sub):
//!
//! ```rust,ignore
//! // HTTP-only server (the common case): `http!` already wraps `tcp!`.
//! Server::default().push(http!(router, ws!(ws_router))).start(addr).await?;
//!
//! // Full architecture: combine HTTP/1.1 with HTTP/2/UDP. Inside an explicit
//! // `tcp!`, use `http1!` for the bare HTTP protocol.
//! Server::default()
//!     .push(tcp!(http1!(router), http2!()))
//!     .push(udp!())
//!     .start(addr).await?;
//! ```

/// Build the bare HTTP/1.1 protocol (a `Tcp` sub-handler).
///
/// WebSocket is reached only through an HTTP upgrade, so the WebSocket
/// sub-handler is attached here — either the default one (built from the same
/// `router`) or an explicit one produced by [`ws!`].
///
/// - `http1!(router)` — serve the router; default WebSocket from the same router.
/// - `http1!(router, ws!(ws_router))` — explicit WebSocket sub-handler.
/// - `http1!(router, handler: h)` — explicit request handler (takes precedence).
/// - `http1!(handler: h)` — explicit handler, no router.
/// - `http1!()` — detector only, no router/handler.
///
/// For an HTTP-only server you normally use [`http!`] instead, which wraps this
/// in the top-level `Tcp`.
#[macro_export]
macro_rules! http1 {
    () => {
        $crate::protocols::http1::Http1::new(None, None).shared()
    };
    (handler: $handler:expr) => {
        $crate::protocols::http1::Http1::new(
            None,
            ::std::option::Option::Some(::std::sync::Arc::new($handler)),
        )
        .shared()
    };
    ($router:expr) => {
        $crate::protocols::http1::Http1::new(
            ::std::option::Option::Some(::std::sync::Arc::new($router)),
            None,
        )
        .shared()
    };
    ($router:expr, handler: $handler:expr) => {
        $crate::protocols::http1::Http1::new(
            ::std::option::Option::Some(::std::sync::Arc::new($router)),
            ::std::option::Option::Some(::std::sync::Arc::new($handler)),
        )
        .shared()
    };
    ($router:expr, $websocket:expr) => {
        $crate::protocols::http1::Http1::new(
            ::std::option::Option::Some(::std::sync::Arc::new($router)),
            None,
        )
        .with_websocket($websocket)
        .shared()
    };
}

/// Build a complete HTTP server protocol — `tcp!(http1!(...), http2!())`.
///
/// This is the default convenience form: you describe only the HTTP handling
/// and do **not** need to write `tcp!` yourself. It includes the top-level
/// `Tcp` stream fallback and enables **both HTTP/1.1 and HTTP/2** by default.
///
/// - `http!(router)` ≡ `tcp!(http1!(router), http2!())`
/// - `http!(router, ws!(ws_router))` — attaches the WebSocket sub-handler to
///   **both** HTTP/1.1 and HTTP/2.
/// - `http!(handler: h)`, `http!()` — same argument forms as [`http1!`];
///   HTTP/2 is always added.
///
/// When mounting HTTP alongside other protocols, use the explicit form
/// `tcp!(http1!(router), http2!(), ...)`.
#[macro_export]
macro_rules! http {
    () => {
        $crate::tcp!($crate::http1!(), $crate::http2!())
    };
    (handler: $handler:expr) => {
        $crate::tcp!($crate::http1!(handler: $handler), $crate::http2!())
    };
    ($router:expr, handler: $handler:expr) => {{
        // Share one router between HTTP/1.1 and HTTP/2.
        let __r = ::std::sync::Arc::new($router);
        $crate::tcp!(
            $crate::protocols::http1::Http1::new(
                ::std::option::Option::Some(__r.clone()),
                ::std::option::Option::Some(::std::sync::Arc::new($handler)),
            )
            .shared(),
            $crate::protocols::http2::Http2::new(::std::option::Option::Some(__r), None).shared()
        )
    }};
    ($router:expr, ws!($($w:tt)*)) => {{
        // Share one router and one WebSocket sub-handler between HTTP/1.1 and HTTP/2.
        let __r = ::std::sync::Arc::new($router);
        let __ws: ::std::sync::Arc<dyn $crate::protocol::Protocol> = $crate::ws!($($w)*);
        $crate::tcp!(
            $crate::protocols::http1::Http1::new(::std::option::Option::Some(__r.clone()), None)
                .with_websocket(__ws.clone())
                .shared(),
            $crate::protocols::http2::Http2::new(::std::option::Option::Some(__r), None)
                .with_websocket(__ws)
                .shared()
        )
    }};
    ($router:expr) => {{
        // Share one router between HTTP/1.1 and HTTP/2.
        let __r = ::std::sync::Arc::new($router);
        $crate::tcp!(
            $crate::protocols::http1::Http1::new(::std::option::Option::Some(__r.clone()), None)
                .shared(),
            $crate::protocols::http2::Http2::new(::std::option::Option::Some(__r), None).shared()
        )
    }};
}

/// Build an HTTP/2 protocol.
///
/// The argument forms mirror [`http1!`] exactly (same router mechanism):
///
/// - `http2!()` — no router/handler.
/// - `http2!(router)` — serve requests through `router`.
/// - `http2!(handler: h)` — explicit handler.
/// - `http2!(router, handler: h)` — handler takes precedence over the router.
/// - `http2!(ws!(ws_router))` — attach a WebSocket sub-handler (RFC 8441).
/// - `http2!(router, ws!(ws_router))` — router plus WebSocket sub-handler.
#[macro_export]
macro_rules! http2 {
    () => {
        $crate::protocols::http2::Http2::new(None, None).shared()
    };
    (handler: $handler:expr) => {
        $crate::protocols::http2::Http2::new(
            None,
            ::std::option::Option::Some(::std::sync::Arc::new($handler)),
        )
        .shared()
    };
    (ws!($($w:tt)*)) => {
        $crate::protocols::http2::Http2::new(None, None)
            .with_websocket($crate::ws!($($w)*))
            .shared()
    };
    ($router:expr, handler: $handler:expr) => {
        $crate::protocols::http2::Http2::new(
            ::std::option::Option::Some(::std::sync::Arc::new($router)),
            ::std::option::Option::Some(::std::sync::Arc::new($handler)),
        )
        .shared()
    };
    ($router:expr, ws!($($w:tt)*)) => {
        $crate::protocols::http2::Http2::new(
            ::std::option::Option::Some(::std::sync::Arc::new($router)),
            None,
        )
        .with_websocket($crate::ws!($($w)*))
        .shared()
    };
    ($router:expr) => {
        $crate::protocols::http2::Http2::new(
            ::std::option::Option::Some(::std::sync::Arc::new($router)),
            None,
        )
        .shared()
    };
}

/// Build a WebSocket protocol to pass into the HTTP protocol macro
/// ([`http1!`] / [`http!`]); the upgrade happens inside HTTP, so WebSocket is
/// never mounted on its own.
///
/// - `ws!()` — the default WebSocket.
/// - `ws!(router)` — a WebSocket driven by a specific router.
#[macro_export]
macro_rules! ws {
    () => {
        ::std::sync::Arc::new($crate::protocols::websocket::Websocket::new(None))
    };
    ($router:expr) => {
        ::std::sync::Arc::new($crate::protocols::websocket::Websocket::new(
            ::std::option::Option::Some(::std::sync::Arc::new($router)),
        ))
    };
}

/// Build the top-level TCP protocol, optionally with nested sub-handlers.
///
/// - `tcp!()` — raw TCP catch-all with no handler.
/// - `tcp!(http1!(router), http2!())` — nested stream protocols.
#[macro_export]
macro_rules! tcp {
    () => {
        $crate::protocols::tcp::Tcp::new(None).shared()
    };
    ($($sub:expr),+ $(,)?) => {
        $crate::protocols::tcp::Tcp::new(None)
            $(.sub($sub))+
            .shared()
    };
}

/// Build a TCP protocol with a raw-connection handler, plus optional sub-handlers.
///
/// `tcp_raw!(|ctx| tokio::spawn(handle(ctx)), http1!(router))`
#[macro_export]
macro_rules! tcp_raw {
    ($handler:expr $(, $sub:expr)* $(,)?) => {
        $crate::protocols::tcp::Tcp::new(::std::option::Option::Some(
            ::std::sync::Arc::new($handler),
        ))
        $(.sub($sub))*
        .shared()
    };
}

/// Build a UDP datagram protocol.
///
/// - `udp!()` — no handler.
/// - `udp!(handler)` — per-datagram handler.
#[macro_export]
macro_rules! udp {
    () => {
        $crate::protocols::udp::Udp::new(None).shared()
    };
    ($handler:expr) => {
        $crate::protocols::udp::Udp::new(::std::option::Option::Some(::std::sync::Arc::new(
            $handler,
        )))
        .shared()
    };
}
