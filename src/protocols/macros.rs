//! Ergonomic constructors for the built-in protocols.
//!
//! Each macro expands to the same `Struct::new(..).shared()` value you would
//! write by hand, so the result composes with [`Server::push`](crate::unified::Server::push)
//! and [`Tcp::sub`](crate::protocols::tcp::Tcp::sub):
//!
//! ```rust,ignore
//! // Full architecture: TCP as the stream fallback, HTTP/HTTP2 nested under it.
//! Server::solo(addr)
//!     .push(tcp!(http!(router), http2!()))
//!     .push(udp!())
//!     .start().await?;
//!
//! // HTTP-only server: mount the protocol directly. WebSocket rides along the
//! // HTTP upgrade, so it is passed *into* `http!`, never mounted separately.
//! Server::solo(addr).push(http!(router, ws!(ws_router))).start().await?;
//! ```

/// Build an HTTP/1.1 protocol.
///
/// WebSocket is reached only through an HTTP upgrade, so the WebSocket
/// sub-handler is attached here — either the default one (built from the same
/// `router`) or an explicit one produced by [`ws!`].
///
/// - `http!(router)` — serve the router; default WebSocket from the same router.
/// - `http!(router, ws!(ws_router))` — explicit WebSocket sub-handler.
/// - `http!(router, handler: h)` — explicit request handler (takes precedence).
/// - `http!(handler: h)` — explicit handler, no router.
/// - `http!()` — detector only, no router/handler.
#[macro_export]
macro_rules! http {
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

/// Alias of [`http!`] (named after the `Http1` struct).
#[macro_export]
macro_rules! http1 {
    ($($tokens:tt)*) => {
        $crate::http!($($tokens)*)
    };
}

/// Build an HTTP/2 protocol.
///
/// - `http2!()` — no router/handler.
/// - `http2!(handler)` — explicit handler.
#[macro_export]
macro_rules! http2 {
    () => {
        $crate::protocols::http2::Http2::new(None).shared()
    };
    ($handler:expr) => {
        $crate::protocols::http2::Http2::new(::std::option::Option::Some(
            ::std::sync::Arc::new($handler),
        ))
        .shared()
    };
}

/// Build a WebSocket protocol to pass into [`http!`] (the upgrade happens
/// inside HTTP, so WebSocket is never mounted on its own).
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
        $crate::protocols::udp::Udp::new(::std::option::Option::Some(
            ::std::sync::Arc::new($handler),
        ))
        .shared()
    };
}
