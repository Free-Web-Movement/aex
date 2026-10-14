# Aex — Async-first, Executor-based Web/TCP/UDP Framework

**English** | [中文](README.zh.md)

> A lightweight, controllable Rust multi-protocol framework faithful to the essence of HTTP

[![Rust](https://img.shields.io/badge/rust-1.88%2B-blue.svg)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/license-GPL--3.0--or--later-green.svg)](LICENSE)
[![ crates.io version](https://img.shields.io/crates/v/aex.svg)](https://crates.io/crates/aex)
[![crates.io downloads](https://img.shields.io/crates/d/aex.svg)](https://crates.io/crates/aex)
[![codecov](https://img.shields.io/codecov/c/github/Free-Web-Movement/aex)](https://codecov.io/gh/Free-Web-Movement/aex)
[![CI](https://github.com/Free-Web-Movement/aex/actions/workflows/rust.yml/badge.svg)](https://github.com/Free-Web-Movement/aex/actions/workflows/rust.yml)
[![Dependencies](https://deps.rs/repo/github/Free-Web-Movement/aex/status.svg)](https://deps.rs/repo/github/Free-Web-Movement/aex)
[![Discord](https://img.shields.io/discord/823762790165774386?logo=discord&logoColor=white&label=chat)](https://discord.com/channels/823762790165774386/823762790165774389)

## Table of Contents

- [Get Started](#get-started)
- [Version](#version)
- [Core Features](#core-features)
- [Design Philosophy: A Universal Server State Machine Containing the Web Straight Line](#design-philosophy-a-universal-server-state-machine-containing-the-web-straight-line)
  - [The Universal Server State Machine](#the-universal-server-state-machine-the-tcp--udp-processing-logic)
  - [The Web Straight Line: the Acyclic Straight Line of the HTTP Branch in the State Machine](#the-web-straight-line-the-acyclic-straight-line-of-the-http-branch-in-the-state-machine)
  - [Stage Mapping of Protocols in the Same State Machine (HTTP / WebSocket / TCP / UDP)](#stage-mapping-of-protocols-in-the-same-state-machine-http--websocket--tcp--udp)
  - [Grammar Diagram: the Universal Server State Machine (Containing the Web Straight Line)](#grammar-diagram-the-universal-server-state-machine-containing-the-web-straight-line)
- [Unified Server Architecture](#unified-server-architecture)
  - [Supported Protocols](#supported-protocols)
  - [Unified Server (Server)](#unified-server-server)
  - [Server Startup Modes](#server-startup-modes)
  - [Protocol Macros in Detail (`tcp!` / `http!` / `udp!` / `ws!`)](#protocol-macros-in-detail-tcp--http--udp--ws)
  - [Protocol Detector System (DetectorRegistry)](#protocol-detector-system-detectorregistry)
  - [Handler Type Signatures](#handler-type-signatures)
- [HTTP Quick Start](#http-quick-start)
  - [Build a Web Server](#build-a-web-server)
  - [HTTP Routing in Detail](#http-routing-in-detail)
  - [Object-based Routing Methods (Core)](#object-based-routing-methods-core)
  - [HTTP/2 Support](#http2-support)
  - [WebSocket Support](#websocket-support)
  - [Static File Serving](#static-file-serving)
- [http-server Static Server](#http-server-static-server)
  - [Middleware](#middleware)
- [Unified Server API](#unified-server-api)
  - [Use Cases](#use-cases)
- [TCP Protocol](#tcp-protocol)
  - [TCP Frame/Command Definitions](#tcp-framecommand-definitions)
- [UDP Protocol](#udp-protocol)
- [P2P: Low-level Infrastructure](#p2p-low-level-infrastructure)
  - [Core Concepts](#core-concepts)
  - [Command ID Definitions](#command-id-definitions)
  - [Connection State Machine](#connection-state-machine)
  - [Handshake Protocol](#handshake-protocol)
  - [Heartbeat Protocol](#heartbeat-protocol)
- [Communicators](#communicators)
  - [Pipe - N:1 Pipeline](#pipe---n1-pipeline)
  - [Spreader - 1:N Broadcast](#spreader---1n-broadcast)
  - [Event - M:N Event System](#event---mn-event-system)
- [Architecture](#architecture)
  - [Multi-layer Architecture Design](#multi-layer-architecture-design)
  - [Core Components](#core-components)
- [Comparison with Other Frameworks](#comparison-with-other-frameworks)
  - [Protocol Support Comparison](#protocol-support-comparison)
  - [Aex Design Principles](#aex-design-principles)
  - [Performance Comparison (AEX vs Axum vs Actix-web)](#performance-comparison-aex-vs-axum-vs-actix-web)
  - [Use Cases](#use-cases-1)
- [Module Structure](#module-structure)
- [Testing](#testing)
- [License](#license)
- [Appendix: Web Straight Line Grammar and Images (docs/grammar.md)](docs/grammar.md)

## Get Started

Add the dependency with a single command:

```bash
cargo add aex
cargo add tokio --features full
```

**Spin up an HTTP service in 30 seconds**

```rust
use aex::http::router::Router as HttpRouter;
use aex::server::Server;
use aex::{_sync, http};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let addr: SocketAddr = "0.0.0.0:8080".parse()?;
    let mut router = HttpRouter::default();

    router.get("/", _sync!(|_| "OK"));

    Server::default()
        .push(http!(router))
        .start(addr)
        .await?;
    Ok(())
}
```

**For async handlers, just wrap them with `_async!`**

```rust
use aex::http::router::Router as HttpRouter;
use aex::server::Server;
use aex::{_async, http};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let addr: SocketAddr = "0.0.0.0:8080".parse()?;
    let mut router = HttpRouter::default();
    // Async closure: `_async!` supports await internally, and likewise needs no annotation
    router.get("/", _async!(|ctx| {
        let html = fetch_home_page().await; // any async logic
        ctx.html(&html);
        true
    }));

    Server::default()
        .push(http!(router))
        .start(addr)
        .await?;
    Ok(())
}
```

**Both forms are `Arc<Executor>` and can be mixed on the same router**

| | Synchronous | Asynchronous |
|---|---|---|
| Macro name | `_sync!` (primary), `now!` (alias) | `_async!` (primary), `then!` (alias) |
| Example | `_sync!(\|_\| "OK")` | `_async!(\|ctx\| { ...await... })` |
| Purpose | Pure synchronous handler / middleware | Async handler / middleware that needs `await` |
| `ctx` parameter | No annotation needed (inferred via generic function) | No annotation needed; can write `\|_\|` when ctx is unused |
| Can `await` internally | No | Yes |
| Product | `Arc<Executor>` | `Arc<Executor>` |

>`_sync!` restores closure parameter inference through the generic function `aex::http::types::sync`, so
>`|_| "OK"` and `|ctx| {...}` need no `&mut Context` annotation; passing them
>directly to `router.get(...)` (which goes through `IntoExecutor`) does require the annotation.

**Routing as methods: mount multiple URLs at once (recommended)**

No more writing `router.get(...)` one by one — write the routes directly on the methods, `push` a single instance, and they are all mounted:

```rust
use aex::http::router::Router as HttpRouter;
use aex::connection::context::Context;
use aex::http::meta::HttpMetadata;
use aex::http::protocol::header::HeaderKey;

struct User {
    name: String,
    api_key: String,
}

#[aex::routes]
impl User {
    // One attribute: two URLs; [auth] is object middleware, see the method of the same name below
    #[get(["/", "/profile"], [auth])]
    fn profile(&self, ctx: &mut Context) {
        ctx.text(&self.name); // Use instance state directly, no global variables needed
    }

    // Async handlers are supported just the same
    #[post("/resources")]
    async fn create(&self, ctx: &mut Context) {
        ctx.text("created");
    }

    // Object middleware: a bare identifier -> a &self method, seeing the same self as the handler
    fn auth(&self, ctx: &mut Context) -> bool {
        let key = HeaderKey::from_str("x-api-key").unwrap();
        ctx.get::<HttpMetadata>()
            .map(|m| m.headers.get(&key).is_some_and(|v| v == &self.api_key))
            .unwrap_or(false)
    }
}

let mut router = HttpRouter::default();
router.push(User {
    name: "aex".into(),
    api_key: "secret".into(),
});
```

> **`&self` is your state** — both route methods and object middleware naturally hold the same instance: no closure wrapping closures, no global variables, no manual parameter passing.

**`push` takes an "instance", not a "type"** — both forms are valid, depending on how the struct is defined:

```rust
// ① Unit struct (no fields): the class name is the instance, so push the class name directly
struct Health;
#[aex::routes(prefix = "/backend")]
impl Health {
    #[get("/ping", no_prefix)]
    fn ping(ctx: &mut Context) -> &'static str { "pong" }
}
router.push(Health);                       // Constructs a zero-field instance, equivalent to push(Health {})

// ② Struct with fields: you must explicitly construct an instance and pass the state in
struct User { name: String, api_key: String }
#[aex::routes]
impl User {
    #[get("/me")]
    fn me(&self, ctx: &mut Context) { ctx.text(&self.name); }
}
router.push(User { name: "aex".into(), api_key: "secret".into() });
```

- Internal implementation: `push` receives a **value**; `router.push<C: AexRoutes>(instance: C)` does `Arc::new(instance)`, and the macro-generated routes/middleware each `Arc::clone` to hold the same instance; `&self` methods read that instance's state directly.
- For the unit struct `struct Health;`, writing `push(Health)` is **constructing a value** (a zero-sized instance), not passing a type.
- No state needed → use a unit struct (less hassle); share state (connection pool, config, counters, etc.) → define fields and pass them in when pushing.

**Route group prefix (`prefix`)** — a group of routes sharing a single prefix, ideal for organizing by hierarchy/subsystem:

```rust
// #[aex::routes]               → no prefix (root path)
// #[aex::routes("/api")]       → string-literal prefix
#[aex::routes(prefix = "/backend")]
impl BackendApi {
    // The path in the attribute is joined with prefix: /backend/admin/users
    #[get("/admin/users")]
    async fn users(ctx: &mut Context) { /* ... */ }

    // Both "/" and "" work: /backend/ and /backend are both accessible
    #[get(["/", ""])]
    async fn home(ctx: &mut Context) { /* ... */ }
}

let mut router = HttpRouter::default();
router.push(BackendApi);
```

- `#[aex::routes(prefix = "/...")]` and `#[aex::routes("/...")]` are equivalent;
- The prefix is normalized automatically (leading `/` guaranteed, trailing slash removed), and the attribute path is appended as-is;
- An empty prefix (`#[aex::routes]`) is equivalent to no prefix, leaving paths unchanged;
- **Individual routes can skip the prefix**: append the `no_prefix` marker to the path attribute, and that route is registered at the root path as-is (`#[get("/health", no_prefix)]` → `/health`, not `/backend/health`).

**`no_prefix` and middleware are never confused** — the rule is clear: it only looks at whether it is a **direct positional argument in the attribute** (a bare identifier, not inside `[]`):

| Form | Meaning |
|------|------|
| `#[get("/health", no_prefix)]` | `no_prefix` is a direct argument → a **skip-prefix** marker |
| `#[get("/x", [auth], no_prefix)]` | middleware array first, marker after → mounts middleware and skips the prefix |
| `#[get("/x", no_prefix, [auth])]` | marker first → skips the prefix and mounts middleware |
| `#[get("/x", [no_prefix])]` | inside the `[]` **array** → it is **middleware** (named `no_prefix`), and the prefix still applies |

> Conclusion: **a bare `no_prefix` = skip the prefix**; **`[no_prefix]` = middleware named no_prefix**. The two are distinguished by "whether it is inside an array" and never clash.
> The only thing to watch: if your middleware happens to be named `no_prefix`, be sure to wrap it as `[no_prefix]`, otherwise it will be treated as a skip-prefix marker.

```rust
#[aex::routes(prefix = "/backend")]
impl Demo {
    // Skips the prefix → GET /ping (without /backend)
    #[get("/ping", no_prefix)]
    fn ping(ctx: &mut Context) -> &'static str { "pong" }

    // [no_prefix] is middleware, the prefix still applies → GET /backend/secure, going through the no_prefix middleware first
    #[get("/secure", [no_prefix])]
    fn secure(ctx: &mut Context) -> &'static str { "ok" }
}

// Global middleware function (named exactly no_prefix, so it only takes effect when wrapped as [no_prefix])
fn no_prefix(ctx: &mut Context) -> bool { /* authentication logic */ }
```

For complete HTTP routing, middleware, HTTP/2, and WebSocket usage, see [HTTP Quick Start](#http-quick-start).

---

## Version

Current version: **0.1.22**

- For dependency configuration, see [Get Started](#get-started) above.

## Core Features

- **Unified-port multi-protocol** - HTTP/1.1, HTTP/2, WebSocket, TCP, and UDP share a single port, with a pluggable protocol detector system
- **Intuitive HTTP routing** - a Trie-tree router supporting static paths, parameter paths, and wildcards
- **Dual synchronous / asynchronous handler forms** - synchronous closures and `_async!` async closures are both accepted uniformly by `IntoExecutor` and can be mixed
- **Explicit middleware chain** - linear execution order with predictable control flow (not an onion model)
- **Native WebSocket support** - integrates naturally as middleware, sharing the HTTP context
- **Multi-protocol support** - a unified interface for HTTP/1.1, HTTP/2, TCP, and UDP servers
- **TypeMap extension** - flexible request/response data storage
- **End-to-end encryption** - ChaCha20-Poly1305 session encryption
- **IPC communicators** - Pipe, Spreader, and Event patterns
- **Bundled `http-server`** - `cargo install aex` gives you an out-of-the-box static file server (nginx/apache style), see [http-server Static Server](#http-server-static-server)

---

## Design Philosophy: A Universal Server State Machine Containing the Web Straight Line

Aex (Rust) is a **universal server state machine**: it handles **all** protocols
(HTTP / WebSocket / TCP / UDP) with a unified "staged process + linear middleware".

**The Web Straight Line is a theory that specifically describes the HTTP request / response mechanism and is unrelated to other protocols**,
and it is **acyclic**: a request comes in, a response goes out, one straight line.
**It applies to all "request → response" style HTTP protocols** (HTTP/1.1, HTTP/2, and later HTTP of the same kind).
It was first proposed by [`calidion/aex`](https://github.com/calidion/aex)
([The Web Straight Line Theory](https://github.com/calidion/aex#what-is-the-web-straight-line)).
In aex, **the Web Straight Line corresponds to the HTTP branch of this universal server state machine — both `Http1` and `Http2` are inside it**:

> **It is not "extended from the Web Straight Line."** aex's universal server state machine **contains** the Web Straight Line —
> the Web Straight Line is the acyclic straight line on its internal HTTP branch, not its source or parent. The two stand in a
> "whole ⊇ part" relationship. For the complete EBNF and railroad diagrams, see [`docs/grammar.md`](docs/grammar.md).

### The Universal Server State Machine (the TCP / UDP Processing Logic)

The essence of the universal server state machine is precisely the **TCP / UDP processing logic**; it is a **loop**:

```
Receive (detect / command) → Parse → Route → Middleware processing → Command processing → Return data → New loop ↺
```

- **Receive (detect / command)**: accept the connection / datagram, detect the protocol (**detect**) or read the **command**.
- **Parse**: encode/decode, restoring bytes into frames / commands / requests.
- **Route**: HTTP uses `method + url`; TCP/UDP use `command + message`.
- **Middleware processing**: a linear `Executor` chain; returning `false` short-circuits.
- **Command processing**: business logic (handler).
- **Return data**: write back the response / reply.
- **New loop**: back to "Receive", to process the next message — this is precisely the most essential difference from HTTP.

**HTTP is a special case on this loop**: it ends as soon as the request is received and data is returned, without returning to receive —
that is the **acyclic** Web Straight Line below.

### The Web Straight Line: the Acyclic Straight Line of the HTTP Branch in the State Machine

The Web Straight Line is the concrete form of the above state machine on **HTTP request / response** — **acyclic**, applicable to all request→response HTTP (HTTP/1.1, HTTP/2):

```
Request start → Data reception & storage / parameter validation → Identity verification → Data analysis & processing → Business processing → Data sending → Request end
```

![The Web Straight Line](docs/Web_Straight_Line.png)

A single HTTP request flows **one-way, sequentially** through each stage along this line. Middleware is inserted between stages: it executes before the handler,
and returning `false` short-circuits (equivalent to `res.end`), terminating the request — **leaving no room to "return" for a flow that has already ended**.

**Why not the onion model**: mainstream middleware forces the linear stages into a recursive stack and keeps a `next` callback, making control flow travel in both the
"enter" and "return" directions. This does not match the real shape of an HTTP request, and it wrongly preserves a callback that should disappear and requires maintaining
two sets of middleware for the same functionality. The Web Straight Line rejects this model.

**The five principles of the Web Straight Line (HTTP) itself** (from `calidion/aex`):

1. **Separate web logic from business logic**, developing only for web logic itself;
2. **Focus on the web flow**, without overstepping into the business domain;
3. **Make writing good web projects simpler**;
4. **View web interaction as a straight line** — i.e. the Web Straight Line;
5. **No MVC**, focusing on the "web logic" architectural layer.

### Stage Mapping of Protocols in the Same State Machine (HTTP / WebSocket / TCP / UDP)

The Web Straight Line covers only the HTTP column; the state machine also includes other protocol branches, which **share the same set of stages and middleware semantics**,
differing only in the concrete implementation of each stage:

| Stage | HTTP/1.1 | WebSocket | TCP | UDP |
|---|---|---|---|---|
| Admission / protocol detection | claimed by `Http11Detector` | handed to the `Websocket` sub-handler after HTTP `Upgrade` | `Tcp` fallback | `Udp` datagram slot |
| Routing | method + url (Trie, params/wildcards) | reuses the HTTP path and `router` from before the upgrade | command + message (`Router` / `CommandRegistry`) | dispatch key per datagram |
| Authentication | cookie · session · http-auth | inherits HTTP headers | session key / handshake | session key / handshake |
| Data parsing | query · body · formdata · files | frames (text / binary) | frames + command codec | datagram unpacking |
| Access control / data control | middleware chain (linear `Executor`) | middleware chain | middleware chain | middleware chain |
| Business logic | route handler | `on_text` / `on_binary` | command handler | datagram handler |
| Output | html / xml / json / streaming | WebSocket frames | frames | datagrams |
| End | response + keep-alive | close / upgrade complete | close | connectionless (a single packet ends it) |

Key point: **the same middleware chain model runs through all four** — every processing unit returns `bool`, and `false` short-circuits;
none of them has a `next` callback, and none is an onion model. WebSocket is merely "middleware after an HTTP upgrade",
while TCP / UDP replace "routing / authentication / parsing" with the corresponding command and datagram implementations, with all other stages perfectly isomorphic.

### How Aex Implements This State Machine

- **Explicit linear middleware chain**: handlers are uniformly `Fn(&mut Context) -> BoxFuture<'_, bool>` (`Executor`),
  executed **linearly** in registration order; returning `false` short-circuits. No `next`, no onion model.
- **Context carries scope-style data**: stages pass data through `Context` (which contains a TypeMap),
  exactly corresponding to "data reception & storage → data analysis & processing" in the state machine.
- **No MVC**: routing is not made into a controller layer; instead `#[aex::routes]` declares routes on object methods,
  mounted by `Router::push` — the business object is the route carrier.
- **WebSocket is upgrade middleware**: WebSocket is reached only through an HTTP upgrade, so it shares the context and routing with HTTP,
  rather than being treated as a "special protocol".
- **Microservice-capable and full-featured**: the same state machine can be trimmed into a microservice, or extended into a
  full-featured server by composing protocols/middleware.

### Grammar Diagram: the Universal Server State Machine (Containing the Web Straight Line)

The state machine written as a grammar: the top level has only `Tcp` (stream) and `Udp` (datagram), and HTTP / HTTP2 / WebSocket / SOCKS
are all sub-handlers of `Tcp`; **the Web Straight Line is a straight line inside its HTTP branch (`Http1` / `Http2`)**.
For the complete EBNF (directly renderable in the [Railroad Diagram Generator](https://bottlecaps.de/rr/ui)) see
[`docs/grammar.md`](docs/grammar.md).

![Universal Server State Machine (AEX Rust)](docs/aex_architecture.png)

---

## Unified Server Architecture

Aex is one of the **most protocol-complete** web frameworks in the Rust ecosystem, and can run multiple protocols simultaneously on the **same port**.

### Supported Protocols

| Protocol type | Detector | Description |
|---|---|---|
| HTTP/1.1 | `Http11Detector` | Standard HTTP requests |
| HTTP/2 | `Http2Detector` | HTTP/2 protocol preface |
| WebSocket | Recognized within HTTP upgrade requests | `Upgrade` header |
| HTTP proxy | Split within `Http11Detector` | absolute-form / CONNECT (`feature = "proxy"`) |
| SOCKS4/4a/5 | `SocksDetector` (built-in) | CONNECT tunneling (`feature = "proxy"`) |
| Custom protocol | custom `ProtocolDetector` | trojan / private protocols |
| TCP | fallback handler | all other traffic |
| UDP | a separate UDP Socket | datagram communication |

#### Built-in HTTP / SOCKS Proxy (`feature = "proxy"`)

A single port carries multiple services simultaneously, **with the client's first byte deciding which service line to take**, with no server-side mode switching:

| Client behavior | Service line |
|---|---|
| `GET /path HTTP/1.1` (origin-form) | website routing / handler |
| `GET http://host/path HTTP/1.1` (absolute-form) | HTTP forward proxy → upstream |
| `CONNECT host:port HTTP/1.1` | HTTP tunnel → upstream |
| `\x05\x01\x00...` (SOCKS greeting) | SOCKS4/4a/5 CONNECT |
| TLS ClientHello | detector claims → custom handler |

```bash
# Cargo.toml
aex = { version = "0.1", features = ["proxy"] }
```

```rust
use aex::connection::global::GlobalContext;
use aex::protocols::{proxy, socks};
use aex::proxy::ProxyAuthorizer;
use aex::server::Server;
use std::sync::Arc;

let authorizer: ProxyAuthorizer = Arc::new(|user, pass| user == "alice" && pass == "secret");

let server = Server::new(Arc::new(GlobalContext::new(addr, None)))
    // The same HTTP/1.1 protocol serves both the website (origin-form) and the HTTP forward proxy
    // (absolute-form / CONNECT), subject to the same set of credentials.
    .push(proxy::http_proxy(None, Some(website), Some(authorizer.clone())))
    .push(socks::protocol(Some(authorizer)));
server.start(addr).await?;
```

- HTTP proxy: absolute-form requests are rewritten to origin-form and relayed (stripping hop-by-hop headers, injecting `Via`); after a CONNECT tunnel is established, raw bytes are forwarded bidirectionally.
- SOCKS proxy: CONNECT only (BIND / UDP ASSOCIATE reply "not supported"); authentication supports no-auth and RFC 1929 username/password (`proxy_authenticator` uniformly controls both the HTTP and SOCKS sides).
- No proxy logic is compiled by default (an open proxy is a risky asset); see `examples/proxy_demo`.
- Test: `cargo test --features proxy` (`tests/proxy_builtin_test.rs`: single-port website+proxy mixed operation, CONNECT tunnel, SOCKS4/4a/5, authentication success/failure).

### Unified Server (Server)

```rust
use aex::http::router::Router as HttpRouter;
use aex::server::Server;
use aex::{http, tcp_raw, udp, _async};
use std::net::SocketAddr;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let addr: SocketAddr = "0.0.0.0:8080".parse()?;

    // HTTP routing
    let mut http_router = HttpRouter::default();
    http_router.get("/", _async!(|ctx| { ctx.text("Hello from unified server!"); true }));

    // Create a unified server: only TCP / UDP are mounted at the top level, other protocols are sub-handlers
    let server = Server::default()
        .push(tcp_raw!(
            |ctx| tokio::spawn(handle_tcp_connection(ctx)),
            http!(http_router)
        ))
        .push(udp!(|ctx| tokio::spawn(handle_udp_packet(ctx))));

    // Start the server - all protocols share the same port
    server.start(addr).await?;
    Ok(())
}
```
### Server Startup Modes

The lifecycle of a unified server has three steps: **construct → mount protocols → start**.

#### 1. Construction: Shared or Independent Context

`GlobalContext` carries state shared across connections (connection manager, event bus, `PairedSessionKey`, business-registered data, etc.).

| Constructor | Context | Purpose |
|---|---|---|
| `Server::new(globals)` | **Reuses** the passed `Arc<GlobalContext>` | Production: share the same global state with other components |
| `Server::default()` | **Creates** an independent `GlobalContext` (with a new `PairedSessionKey`) | examples / tests: self-contained, sharing no global state |

**Servers within the same process that need to share state must pass the same `Arc<GlobalContext>` via `new`**;
only when "this server shares no state with the outside" should you use `default()`. Multiple servers each calling `default()`
will get **mutually isolated** connection tables, event buses, and session keys. The listen address is not bound here, but passed in at startup.

#### 2. Mounting: Top-level TCP / UDP (others as sub-handlers)

`push(protocol)` (the in-place version of `mount`) is the only mounting entry point. **The recommended top level is a `Tcp` fallback plus `Udp`**,
with HTTP / HTTP/2 / WebSocket / proxy etc. as **sub-handlers** of `Tcp`; when serving only one streaming protocol,
you may also `push` it directly at the top level (in which case there is no fallback for other protocols):

```rust
use aex::{http1, http2, tcp, udp};

// Multi-protocol: you must use `tcp!` to explicitly mount each streaming protocol; `http!` is a web-only shorthand
Server::new(globals)
    .push(tcp!(http1!(router), http2!(router)))  // top-level TCP + HTTP/1.1 + HTTP/2
    .push(udp!());                               // datagram protocol
```

> **Only when developing a web (HTTP) server** should you use the simplified `http!(router)` (equivalent to
> `tcp!(http1!(router), http2!(router))`); **as soon as you need to mount another protocol, you must use `tcp!`**.

- `Tcp` is the streaming fallback: after the detection layer claims a connection, it hands it to the **sub-handler** able to handle that protocol (HTTP → WebSocket).
- `Udp` enters the datagram slot; `start(addr)` will spawn a UDP receive loop for it in the background.
- Protocols with a `detector` automatically register their detector into the detection registry when mounted, usually removing the need for a manual `.detector(...)`.

#### 3. Startup: `start(addr)`

| Method | Behavior |
|---|---|
| `.start(addr).await` | Binds `addr`, runs the TCP accept loop; **if a `Udp` protocol is mounted, it also `spawn`s a UDP receive loop in the background**. Blocks and does not return. |

```rust
Server::new(globals).push(http!(router)).start(addr).await?;   // shared context
Server::default().push(http!(router)).start(addr).await?;      // independent context
```

`start(addr)` uses `tokio::spawn` for UDP, so the TCP main loop and the UDP loop run **concurrently within the same process**,
sharing the same port number (TCP / UDP ports are independent of each other). TCP and UDP protocol handling is entirely the responsibility of the
protocol handlers mounted for each (`Tcp` / `Udp`); Server no longer exposes separate `start_tcp` / `start_udp` entry points,
nor does it provide `at` / `solo` / `listen`.

### Protocol Macros in Detail (`tcp!` / `http!` / `udp!` / `ws!`)

The built-in protocol macros are syntactic sugar for `Struct::new(..).shared()`, compressing "construct → box as `Arc<dyn Protocol>`
→ mount" into one line, directly usable with `Server::push`. They are all `#[macro_export]`, located at the crate root:
after `use aex::{http, tcp, udp};` you can write `tcp!(…)` directly.

**Expansion reference table**

| Macro | Expands to | Returns |
|---|---|---|
| `tcp!()` | `Tcp::new(None).shared()` | `Arc<Tcp>` |
| `tcp!(sub, …)` | `Tcp::new(None).sub(sub)….shared()` | `Arc<Tcp>` |
| `tcp_raw!(h, sub, …)` | `Tcp::new(Some(Arc::new(h))).sub(sub)….shared()` | `Arc<Tcp>` |
| `http1!()` | `Http1::new(None, None).shared()` | `Arc<Http1>` |
| `http1!(router)` | `Http1::new(Some(Arc::new(router)), None).shared()` | `Arc<Http1>` |
| `http1!(handler: h)` | `Http1::new(None, Some(Arc::new(h))).shared()` | `Arc<Http1>` |
| `http1!(router, handler: h)` | `Http1::new(Some(..), Some(..)).shared()` | `Arc<Http1>` |
| `http1!(router, ws!(r2))` | `Http1::new(Some(..), None).with_websocket(ws!(r2)).shared()` | `Arc<Http1>` |
| `http2!(…)` | The **exact same** parameter forms as `http1!(…)` (router / `handler:` / `ws!`) | `Arc<Http2>` |
| `http!(…)` | **`tcp!(http1!(…), http2!(…))`** (a completeness wrapper, see below) | `Arc<Tcp>` |
| `ws!()` / `ws!(r)` | `Arc::new(Websocket::new(None / Some(Arc::new(r))))` | `Arc<Websocket>` |
| `udp!()` / `udp!(h)` | `Udp::new(None / Some(Arc::new(h))).shared()` | `Arc<Udp>` |

> `.shared()` is just `Arc::new(self)`; `ws!` returns an `Arc` directly, because it only serves as a parameter to the HTTP protocol.

**One by one**

- **`http!` — the web-only simplified entry point.** Expands to `tcp!(http1!(...), http2!())`:
  it gives you **top-level TCP fallback + HTTP/1.1 + HTTP/2** in one go, so you only describe HTTP handling.
  **Use it only when developing a web (HTTP) server alone**; **to develop multi-protocol (HTTP + TCP/UDP/…), you must
  combine explicitly with `tcp!`**. Also, HTTP/1.1 and HTTP/2 **share the same `router` and the same `ws!` sub-handler**:
  - `http!(router)` ≡ `tcp!(http1!(router), http2!(router))` (the same router)
  - `http!(router, ws!(ws_router))` ≡ both share this WebSocket sub-handler
  - `http!(handler: h)`, `http!(router, handler: h)`, `http!()` likewise pass the arguments to both protocols.
- **`http1!` / `http2!` — bare protocols.** They construct `Http1` / `Http2` respectively, with **identical** parameter forms
  (router, `handler:`, `ws!`). `handler:` takes precedence over `router`; the `router` mechanism is the same for both.
  They are usually explicitly composed as sub-handlers of `tcp!`: `tcp!(http1!(router), http2!(router))`.
  Use the bare protocols only when you need independent routers (by default `http!` already makes the two share).
- **`tcp!` — the top-level streaming fallback.** TCP is the **only** top-level streaming protocol: HTTP/1.1, HTTP/2, WebSocket,
  and SOCKS are all its **sub-handlers**; a connection claimed by no sub-handler goes to the optional raw handler inside `tcp!`,
  and is dropped if there is none. The registration order of sub-handlers is their priority at the same level.
  - `tcp!()`: no raw handler, pure fallback.
  - `tcp!(sub, …)`: `Tcp::new(None).sub(sub)` appends sub-handlers one by one (zero or more).
- **`tcp_raw!(handler, sub, …)` — TCP with a raw connection handler.** Use it when you must handle a bare connection that
  "no one claimed". The `handler` type is `Fn(Context) -> JoinHandle<()>` (takes over the `Context`,
  usually with `tokio::spawn`); it can be followed by any number of sub-handlers.
- **`ws!` — WebSocket.** It is **never mounted alone**: WebSocket is reached only through an HTTP upgrade, so it can only be
  passed as a parameter to `http!` / `http1!` / `http2!`. `ws!` returns `Arc<Websocket>`.
- **`udp!` — UDP datagrams.** `Udp` is the only top-level datagram protocol. Each datagram constructs a short-lived
  `Context` handed to the handler; if `None`, it is dropped. Independent of the TCP port, they can coexist with the same number.

**Composition and nesting rules**

```
// Web-only server: use the simplified entry point http!
Server::push
 └─ http!(router)            top-level TCP + HTTP/1.1 + HTTP/2 (sharing router / ws!)
      └─ ws!(…)              WebSocket (reached via HTTP upgrade, only at this level)

// Multi-protocol: must compose explicitly with tcp!
Server::push
 ├─ tcp!(http1!(router), http2!(router))   // top-level TCP (stream)
 │    └─ ws!(…)                              // WebSocket reached via HTTP upgrade
 └─ udp!( … )                                // top-level datagram (the only one)
```

- **The top level has only two protocols**: `Tcp` (stream) and `Udp` (datagram).
- **`http!` is only for web-only servers**; `http!(router)` already includes top-level `Tcp` and HTTP/1.1/HTTP/2.
- **To develop multi-protocol (HTTP + TCP/UDP/…), you must use `tcp!`**:
  `tcp!(http1!(router), http2!(router), …)`.
- WebSocket **must not** be `push`ed directly; it must be embedded in `http!` / `http1!` / `http2!`.
- Detectors register automatically with the protocol; the sub-handler order inside `tcp!` is their priority.

**Common mistakes**

| Form | Problem | Correct form |
|---|---|---|
| `push(tcp!(http!(r)))` | `http!` already contains `tcp!`, causing duplicate nesting | `push(http!(r))` |
| `push(ws!(r))` | WebSocket is not mounted under HTTP | `push(http!(r, ws!(wr)))` |
| `http!(router, handler: h, ws)` | illegal argument order/form | `http!(router, handler: h)` or `http!(router, ws!(wr))` |
| `tcp!(handler, …)` | `tcp!` does not accept a positional handler | use `tcp_raw!(handler, …)` |
| `await` a long task inside `udp!(handler)` | blocks the UDP receive loop | `udp!(|ctx| tokio::spawn(...))` |

**Related companion macros**

| Macro | Purpose |
|---|---|
| `_async!` / `then!` | wraps an async closure into an `Arc<Executor>` handler/middleware |
| `_sync!` / `now!` | wraps a synchronous closure into an `Arc<Executor>` (e.g. `_sync!(\|_\| "OK")`) |
| `validator!` / `v!` | constructs validation middleware from a DSL |
| `frame!` / `command!` / `codec!` | define TCP frames/commands and their codec implementations |
| `router!` | constructs a TCP `Router` from extractors + handlers |
| `#[aex::routes]` | declares HTTP routes on an `impl`, mounted with a `Router::push` instance |

```rust
use aex::{http, http1, http2, tcp, udp};

// Multi-protocol: use `tcp!` to explicitly mount each streaming protocol (HTTP/1.1, HTTP/2), plus UDP
Server::default()
    .push(tcp!(http1!(router), http2!(router)))       // one TCP fallback, with HTTP/1.1 + HTTP/2 underneath
    .push(udp!(|ctx| tokio::spawn(handle_ctx(ctx))))  // datagrams
    .start(addr)
    .await?;

// Web (HTTP) server only: use the simplified http! (equivalent to tcp!(http1!, http2!))
// WebSocket is reached via HTTP upgrade, so it is passed to http!/http1! (not mounted separately)
Server::default().push(http!(router, ws!(ws_router))).start(addr).await?;
```

### Protocol Detector System (DetectorRegistry)

Protocol identification is no longer logic hardcoded into the server, but an **ordered, pluggable, runtime-mutable** detector queue.
The registry is empty by default — HTTP/1.1, HTTP/2, WebSocket, trojan… all protocol detectors are added
manually by the user as needed, with the order being the priority:

```rust
use aex::protocols::{http1::Http11Detector, http2::Http2Detector};
use aex::detect::{
    DetectorMode, DetectorRegistry, Position,
    ProtocolDetector, DetectionState, Verdict,
};

// Custom detector: implement the ProtocolDetector trait
struct TrojanDetector;

impl ProtocolDetector for TrojanDetector {
    fn name(&self) -> &str { "trojan" }
    fn protocol(&self) -> &str { "trojan" }

    fn detect(&self, buf: &[u8], _state: &mut DetectionState) -> Verdict {
        if buf.starts_with(b"\x16\x03\x01") {
            Verdict::Match                       // claim the connection
        } else if !buf.is_empty() {
            Verdict::Pass                        // no match, hand to the next one
        } else {
            Verdict::NeedMore(3)                 // not enough bytes, read a bit more
        }
    }
}

let server = Server::new(Arc::new(GlobalContext::new(addr, None)))
    .detector(Arc::new(TrojanDetector))          // placed at the head of the queue, before HTTP is decided
    .detector(Arc::new(Http2Detector))           // built-in components can also be mounted manually
    .detector_at(Position::Front, Arc::new(Http11Detector))  // specify the insertion position
    .push(trojan_protocol);                      // handling after claiming is the job of the mounted Protocol
```

Core semantics:

| Mechanism | Description |
|------|------|
| **Manual registration** | `detector()` appends to the tail; `detector_at(Front / Back / Before(n) / After(n))` orders freely |
| **Runtime add/remove** | `registry.register() / unregister() / replace()` can be called while the service is running; each connection detects from a snapshot, with no tearing |
| **Conflict protection (dual)** | On registration, duplicate names and `conflicts_with` declared conflicts are rejected (bidirectional check); at runtime the "first Match wins" short-circuits the remaining detectors |
| **State marker DetectorMode** | `Standard` dispatches normally to the corresponding handler; `Forward` marks a stateful pass-through detector — a hit terminates all detection, and the connection is forwarded directly |
| **Chain state DetectionState** | Number of buffered bytes, claim result+mode, per-detector verdict trace (history), per-detector private scratch; after dispatch it is passed along the whole chain as a ctx attribute |

`Verdict` has three states: `Match` (claim) / `Pass` (no match) / `NeedMore(n)` (insufficient data, buffering at most 16 KiB).

#### Proxy Protocol Detection Example (examples/detectors_*)

The core keeps only mainstream Web protocols (HTTP/1.1, HTTP/2; HTTP/3/QUIC to be added later). The detectors and validation middleware
for proxy protocols such as TLS, SOCKS, trojan, and VLESS are placed in the examples directory as **development examples**,
all composed from the public framework API, directly runnable and usable as templates:

| Example | Content |
|------|------|
| `examples/detectors_common/mod.rs` | Shared implementation: `TlsDetector` (passive ClientHello parsing, extracting SNI/ALPN), `SocksDetector` (SOCKS4/5 greeting recognition), `TlsLoader` + `TlsMiddleware::accept` (certificate loading + rustls TLS termination), `TrojanMiddleware::validate`, `VlessMiddleware::validate` |
| `examples/detector_tls/` | SNI/ALPN routing: the detection layer claims "tls" and dispatches by ClientHello name |
| `examples/detector_socks5/` | Mixed SOCKS5/SOCKS4 listening |
| `examples/detector_trojan/` | Full single-port trojan-over-TLS chain: sniff → TLS termination → header validation and stripping |
| `examples/detector_vless/` | VLESS structure validation (UUID authentication left to the business layer) |

```bash
cargo test --test proxy_detectors_test   # parser unit tests + real socket end-to-end
cargo run --example detector_trojan cert.pem key.pem
```

Layering semantics:

```text
Raw bytes ──► sniffing detectors (TlsDetector / SocksDetector / Http*)
                 │ first Match wins
                 ▼
         custom_handler(protocol) dispatch
                 ▼
 TlsMiddleware::accept(cert,key) ──► the ctx stream becomes decrypted plaintext
                 ▼
 Trojan/VLESS validate ──► business handler / fallback
```

**Boundary note**: the request headers of VMess and Shadowsocks are encrypted under keys derived from UUID/PSK, so at the passive byte
level they cannot be distinguished from random traffic; hence there is no "passive detector" — supporting them requires a complete protocol
middleware that holds the user database and performs trial decryption (an application-layer responsibility). Clash is a client rather than a wire
protocol, so there is no independent detection for it. All validators have a built-in 10-second read timeout (`VALIDATION_TIMEOUT`), and a peer hanging mid-packet is cut off.

### Handler Type Signatures

```rust
pub type HttpHandler = Arc<dyn Fn(&mut Context) -> BoxFuture<'_, bool> + Send + Sync>;
pub type Http2Handler = Arc<dyn Fn(&mut Context) -> BoxFuture<'static, bool> + Send + Sync>;

// TCP/UDP handlers receive a Context and can exchange information within the aex system
pub type TCPHandler = Arc<dyn Fn(Context) -> JoinHandle<()> + Send + Sync>;
pub type UDPHandler = Arc<dyn Fn(Context) -> JoinHandle<()> + Send + Sync>;
```

---

## HTTP Quick Start

### Build a Web Server

Three steps: **create a Router → register routes → hand it to the unified server (`Server`) to start**.
Only TCP/UDP are mounted at the top level; HTTP is loaded as a sub-handler of TCP.

Handlers come in two forms, both `Arc<Executor>`, uniformly accepted by the `IntoExecutor` trait and mixable:
- **Synchronous** `_sync!(|ctx| R)` (alias `now!`): no annotation, cannot `await`; equivalent in effect to passing `|ctx: &mut Context| R` directly;
- **Asynchronous** `_async!(|ctx| R)` (alias `then!`): can `await` internally, no annotation.

First, the simplest synchronous form:

```rust
use aex::http::router::{NodeType, Router as HttpRouter};
use aex::server::Server;
use aex::{_sync, http};
use std::net::SocketAddr;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let addr: SocketAddr = "0.0.0.0:8080".parse()?;
    let mut router = HttpRouter::default();

    router.get("/", _sync!(|_| "Hello, World!"));

    Server::default()
        .push(http!(router))
        .start(addr)
        .await?;
    Ok(())
}
```

When HTTP/2, WebSocket, TCP, and UDP need to share the same port, `http!(router)` already enables
HTTP/1.1 and HTTP/2 simultaneously by default (sharing the same router); if you need independent configuration for each,
write `tcp!(http1!(router), http2!(router2))` explicitly.

### HTTP Routing in Detail

> **The two forms, synchronous and asynchronous, are essentially two different handler types** (uniformly accepted by the `IntoExecutor` trait):
> - **Synchronous** `_sync!(|ctx| R)`: R is `bool`/`()`/`String`/`&str`, no annotation, cannot `await`
> - **Asynchronous** `_async!(|ctx| R)`: can `await` internally, wrapped into `Arc<Executor>` by `_async!`, no annotation

```rust
use aex::http::router::{NodeType, Router as HttpRouter};
use aex::http::params::Params;
use aex::_sync;

// 1. Create a router
let mut router = HttpRouter::default();

// 2. Static route (synchronous closure: _sync! needs no annotation)
router.get("/api/health", _sync!(|_| "OK"));

// 3. Parameter route (synchronous closure)
router.get("/api/users/:id", _sync!(|ctx| {
    let params = ctx.local.get_ref::<Params>();
    if let Some(p) = params {
        let id = p.data.as_ref().and_then(|d| d.get("id").cloned());
        ctx.send(format!("User: {}", id.unwrap_or_default()), None);
    }
}));

// 4. Wildcard route (async closure: wrapped by _async!, can await internally)
router.get("/api/files/*", _async!(|ctx| {
    let params = ctx.local.get_ref::<Params>();
    if let Some(p) = params {
        let path = p.data.as_ref().and_then(|d| d.get("*").cloned());
        ctx.send(format!("File: {}", path.unwrap_or_default()), None);
    }
    true
}));

// 5. Route with middleware (synchronous form + chained middleware)
router.post("/api/users", _sync!(|ctx| {
    ctx.text("Created");
})).middleware(auth_middleware);
```

Route handlers support several equivalent forms: returning a string directly, the `ctx.text(...)`/`ctx.json(...)`/`ctx.html(...)` convenience methods, or the raw `ctx.send(content, mime)`:

```rust
// Synchronous form: _sync! needs no annotation
router.get("/a", _sync!(|_| "Hello world!"));                       // return a string directly
router.get("/b", _sync!(|ctx| { ctx.text("Hello"); }));             // convenience method, no need for true
router.get("/c", _sync!(|ctx| {                                     // raw form
    ctx.send("Hello world!", None);
    true
}));
```

> If you don't use `_sync!`, passing the synchronous closure directly to `router.get(...)` (which goes through `IntoExecutor`) is also equivalent,
> except that you must annotate the parameter type: `|ctx: &mut Context| R`.

### Object-based Routing Methods (Core)

The recommended way: write **one controller / one business module** as a `struct` (class), with routes as its `&self`
methods and middleware also as its `&self` methods — the handler and middleware see the **same instance**, so their state is naturally consistent.

```rust
use aex::http::router::Router as HttpRouter;
use aex::connection::context::Context;

// Module: a single instance (class) carrying routes and state
struct User {
    name: String,
    db: String,      // simulated dependency; in a real project this could hold a connection pool, config, etc.
}

#[aex::routes]
impl User {
    // Multiple paths + object middleware; &self methods can use instance state directly
    #[get(["/", "/profile"], [auth])]
    fn profile(&self, ctx: &mut Context) {
        ctx.text(&self.name);
    }

    // async handler
    #[post("/resources")]
    async fn create(&self, ctx: &mut Context) {
        ctx.text("created");
    }

    // Object middleware: a &self method returning bool (true passes / false intercepts)
    fn auth(&self, ctx: &mut Context) -> bool {
        // Authenticate using the config/dependencies on self
        self.is_admin(ctx)
    }

    fn is_admin(&self, ctx: &mut Context) -> bool {
        let _ = ctx;
        self.name == "admin"
    }
}

let mut router = HttpRouter::default();
router.push(User { name: "aex".into(), db: "...".into() });
```

- Supports the `get`/`post`/`put`/`delete`/`patch`/`options`/`head`/`all` attributes.
- The first parameter is the path: a string, or an array of strings (`["/", "/profile"]`); the second parameter is the middleware array.
- Handlers support async and synchronous, and their return value follows `HandlerOutput` (`bool`/`()`/`String`/`&'static str`).
- The mounted instance is captured into each route closure, and `&self` methods can read and write that instance's state.
- Attributes such as `#[get]` only declare; the actual binding is done by `router.push(instance)`; push takes an **instance** — for a unit struct you can write the class name directly, while a struct with fields needs `router.push(User { ... })` (see [push takes an instance, not a type](#get-started)).
- Associated functions without `&self` (`fn f(ctx: &mut Context)`) can likewise be used as routes or middleware.
- A whole group of routes can take a prefix: `#[aex::routes(prefix = "/backend")]` (or `#[aex::routes("/backend")]`), and every path inside the block is automatically joined with that prefix (see [route group prefix](#get-started)).

### HTTP/2 Support

HTTP/2 shares the same router as HTTP/1.1:

```rust
use aex::{http, http2, tcp};
use aex::http::router::{NodeType, Router as HttpRouter};
use aex::server::Server;
use std::net::SocketAddr;

Server::default()
    .push(http!(router)) // HTTP/1.1 + HTTP/2 share the same router
    .start(addr)
    .await?;
```

### WebSocket Support

WebSocket is implemented as middleware, sharing the HTTP context:

```rust
use aex::http::websocket::{TextHandler, BinaryHandler};
use aex::http::middlewares::websocket::WebSocket;

let text_handler: TextHandler = Arc::new(|ws, ctx, text| {
    Box::pin(async move {
        println!("Received: {}", text);
        ws.send_text("pong").await;
        true
    })
});

let ws = WebSocket {
    on_text: Some(text_handler),
    on_binary: None,
};

router.get("/ws", _sync!(|_| true))
    .middleware(WebSocket::to_middleware(ws));
```

### Static File Serving

One line to register, with automatic MIME type detection, supporting basic website assets such as html/css/js/json/images by default:

```rust
use aex::http::router::Router as HttpRouter;

let mut router = HttpRouter::default();
router.static_files("/static", "public");
// GET /static/app.js      -> public/app.js      (application/javascript)
// GET /static/img.png     -> public/img.png     (image/png)
// GET /static             -> public/index.html  (text/html)
```

- **Automatic MIME**: detected by extension, covering html/htm, css, js/mjs, json, txt/md, csv, xml, pdf, zip, wasm, png/jpg/gif/webp/svg/ico; unrecognized types fall back to `application/octet-stream`. For text types (txt/md/html/css/csv), the `Content-Type` includes `charset=utf-8` by default.
- **Directory listing**: when a directory has no entry file, an HTML file listing page is generated (similar to `python -m http.server` / nginx autoindex), with directories sorted first, file sizes shown, subdirectories recursively enterable, and a parent directory to return to.
- **Directory redirect**: accessing a directory without a trailing slash returns a 301 to `path/` (not losing the query), ensuring correct relative-link resolution, consistent with nginx/apache behavior.
- **Recursive directories**: all **subdirectories** under the prefix are accessible (`/static/assets/img/...`); a directory request falls back to that directory's `index.html` first.
- **No escaping**: `..` path segments are always rejected; and every access resolves symlinks to verify the target is still within the root directory (symlinks cannot escape), making it impossible to ever access any file outside the root.
- **Size limit**: **100 MiB** by default, covering the tens-of-megabyte file downloads common on internal networks; over-limit returns 404 to prevent huge files from dragging down the server — use a dedicated download service for larger files.
- **Customization**: `static_files_with` can adjust the limit and entry filename:

```rust
use aex::http::static_files::StaticFiles;

router.static_files_with("/static", StaticFiles::new("public").max_file_size(5 * 1024 * 1024));
```

## http-server Static Server

`aex` ships with an `http-server` binary by default — an **out-of-the-box nginx/apache-style static file server**. Just `cargo install aex` (or `cargo install --path .` during development), and you can publish an HTTP static-asset site from any directory with no code at all:

```bash
cargo install aex
http-server                 # current directory, port 8080
http-server 3000            # current directory, port 3000
http-server ./public        # publish ./public
http-server ./public 3000   # publish ./public, port 3000
```

**Current capabilities** (exactly the same as `Router::static_files` above):

- Automatic MIME (text types with `charset=utf-8`)
- Directory listing page: **file-type-specific icons** (directory=📁, Rust=🦀, Go=🐹, Python=🐍, Ruby=💎, C=🅲, TS=🔷, JS=🟨, HTML=🌐, CSS=🎨, image=🖼️/🌅, text=📄/📝, audio=🎵/🎼, video=🎬/🎥, archive=📦/🗜️, PDF=📕, installer=⚙️/🤖 etc., with dozens of extensions each distinct) + file size + directories sorted first
- Recursively enter all subdirectories, `..` to go up, 301 redirect to add a trailing slash to directories
- Directory fallback to `index.html`
- No escaping: reject `..` segments + symlink resolution validation, so files outside the root are never accessible
- Single-file limit of 100 MiB (over-limit 404, use a dedicated download service for large files)
- **Automatic port increment on conflict**: if occupied, retry with +1 until available
- Print the local machine's IPv4 addresses on startup for convenient internal-network access

**Continuously improving** (benchmarked against nginx/apache's common static capabilities, advanced by priority):

- [x] Directory listing page (autoindex) and file-type icons
- [ ] Byte-range requests (resumable downloads / video seeking)
- [ ] Conditional requests: `ETag` / `Last-Modified` (304 cache negotiation)
- [ ] gzip/brotli content-negotiated compression
- [ ] Hidden files (`.git` etc.) not listed by default, with a configurable switch
- [ ] Custom 404 page
- [ ] Customizable directory index page template / sort-order configuration
- [ ] Multi-entry `index` fallback (index.html → index.htm → default.htm)
- [ ] Upload support (optionally enabled)
- [ ] `--auth` simple Basic Auth protection
- [ ] `--prefix` to specify the publish prefix, `--host` to bind a network interface

> The above capabilities evolve in sync across `Router::static_files_with` + `StaticFiles` configuration items, keeping the library and the binary consistent.

### Middleware

Middleware is a link in the processing chain, located before the handler and executed linearly in declaration order. Its essence is just an `Executor`:

```rust
pub type Executor = dyn for<'a> Fn(&'a mut Context) -> BoxFuture<'a, bool> + Send + Sync;
```

- Returning `true` → pass through, continue to the next middleware / handler
- Returning `false` → intercept, terminate the request (status code defaults to 400, can be overridden first with `ctx.status(...)`)

There are two forms, corresponding to two needs:
- **Functional (stateless)**: closures, free functions, reusable `Arc<Executor>`, see the preceding sections;
- **Object-based (stateful)**: `&self` methods that can directly access the instance's `self`, see [Scenario 2: Attribute Macros](#scenario-2-attribute-macros-get--post---inside-impl-block).

#### Writing the Simplest Middleware (Synchronous)

```rust
use aex::connection::context::Context;
use aex::http::protocol::status::StatusCode;

let auth = _sync!(|ctx| {
    if ctx.req().query("token").as_deref() == Some("secret") {
        true
    } else {
        ctx.status(StatusCode::Unauthorized).text("forbidden");
        false
    }
});
```

#### Async Middleware: Just Use `_async!`

```rust
let rate_limiter = _async!(|ctx| {
    // any async logic: querying a DB, counting, IO……
    true
});
```

#### Sync vs Async: Writing Differences at a Glance

| | `_sync!(\|ctx\| { ... })` / `now!(...)` | `_async!(\|ctx\| { ... })` / `then!(...)` |
|---|---|---|
| Purpose | Pure synchronous handler / middleware | Async handler / middleware that needs `await` |
| `ctx` parameter | No annotation needed (inferred via generic function) | No annotation needed; can write `\|_\|` when ctx is unused |
| Return value | Returns `bool`/`()`/`String`/`&str` directly | Returns `bool` (can be omitted; return true after awaiting in the body) |
| Can `await` internally | No | Yes (query a DB / IO / call an async fn) |
| Product type | `Arc<Executor>` | `Arc<Executor>` |
| Where to use | `.middleware(...)` / `router.get(...)` | Exactly the same as synchronous, mixable |

> No annotation is a convenience unique to `_sync!`/`_async!`: `_sync!` restores closure parameter inference through the generic function
> `aex::http::types::sync`. If you pass a synchronous closure **directly** to `router.get(...)` (via `IntoExecutor`),
> you must annotate `|ctx: &mut Context|` — equivalent in effect, just a few more characters.

**In one sentence**: use `_sync!` (or its alias `now!`) for synchronous work, and `_async!` (or its alias `then!`) when you need `await`; both are `Arc<Executor>`, and a route's middleware chain can **mix sync + async**:
`.middleware(_sync!(auth)).middleware(_async!(async_log))`.

#### What Middleware Can Do

- **Read the request**: `ctx.req().method()` / `.param("id")` / `.query("token")` / `.form("name")`, or `ctx.local.get_ref::<HttpMetadata>()` to read request headers and metadata
- **Write the response**: `ctx.text()` / `ctx.json()` / `ctx.html()` / `ctx.send()`, `ctx.status(...)`, `ctx.redirect(url)`
- **Pass state**: `ctx.set(data)` / `ctx.get::<T>()` to share data between middleware and handlers

#### Two Usage Scenarios, Different Middleware Forms

##### Scenario 1: Chained API (`router.get` / `.middleware(...)`)

The chained API has no `self` context, so middleware **only accepts global functions** and **anonymous closures** (returning `Arc<Executor>`):

```rust
use aex::connection::context::Context;
use aex::http::types::IntoExecutor;

// Anonymous closures (sync / async both fine)
router.get("/x", _sync!(|_| true)).middleware(_sync!(|ctx| {
    // synchronous logic
    true
}));
router.get("/x", _async!(|_| { true })).middleware(_async!(|ctx| {
    do_async().await;
    true
}));

// Global functions
fn auth(ctx: &mut Context) -> bool {
    ctx.req().header("x-api-key").is_some()
}
router.get("/secret", handler).middleware(auth);

// Config builder
router.get("/x", handler).middleware(RateLimitConfig::new(100, 60).by_ip().build());
```

Inline forms such as `router.get_with(...)` / `router.post_with(...)` likewise only accept `Arc<Executor>`:

```rust
router.get_with("/x", [_sync!(|_| true), auth], |_ctx: &mut Context| true);
```

##### Scenario 2: Attribute Macros (`#[get]` / `#[post]` / ..., inside impl block)

The middleware array of an attribute macro has **richer resolution capabilities** — because the macro knows which methods are in the impl block:

| Form | Category | Lookup order |
|------|------|----------|
| `self.auth` | SelfMethod | forces an instance method (`&self`, returning `bool`); if not found → compile error |
| `auth` | BareIdent | cascade: ① self method (`&self`, `→ bool`) → ② associated function (no self, `→ bool`) → ③ global function (sync only) |
| `_sync!(...)` / `_async!(...)` | Expr | converted via `IntoExecutor`, equivalent to the chained API |
| `logger!()` / `v!(...)` | Expr | built-in middleware, same as above |

```rust
struct Api {
    api_key: String,
}

#[aex::routes]
impl Api {
    // self.auth → explicit instance method
    #[get("/admin", [self.auth])]
    fn admin(&self, ctx: &mut Context) {
        ctx.text("admin-only");
    }

    // auth → cascade: first looks for self.auth (found, uses the instance method)
    #[get("/secret", [auth])]
    fn secret(&self, ctx: &mut Context) {
        ctx.text("secret-ok");
    }

    // global_auth → cascade: not in the impl block → global function
    #[get("/public", [global_auth])]
    fn public(&self, ctx: &mut Context) {
        ctx.text("public-ok");
    }

    // check → cascade: finds the associated function check (no self, returns bool)
    #[get("/checked", [check])]
    fn checked(&self, ctx: &mut Context) {
        ctx.text("checked-ok");
    }

    // Instance method
    fn auth(&self, ctx: &mut Context) -> bool { /* ... */ }

    // Associated function (no self)
    fn check(ctx: &mut Context) -> bool { /* ... */ }
}

// Global function
fn global_auth(ctx: &mut Context) -> bool { /* ... */ }
```

**Key differences**:
- The chained API has no `self` context, so instance methods cannot be called → only global functions + closures.
- The attribute macro knows the impl block's method list, so it supports cascade lookup of **instance methods** and **associated functions**.
- The bare identifier `auth` is a **cascade lookup** (self → associated → global) in the attribute macro, whereas in the chained API, passing a function name goes straight through `IntoExecutor` (no cascade).

#### Mounting

Both approaches ultimately go through `Router::push(instance)`:

```rust
// Chained: middleware is appended via .middleware()
router.get("/x", _sync!(|_| true)).middleware(auth);
router.get_with("/x", [_sync!(|_| true), rate_limiter], |_ctx: &mut Context| true);

// Attribute macro: routes + middleware are declared in #[get/post], and router.push(instance) mounts them all at once
#[get(["/", "/profile"], [self.auth, RateLimitConfig::new(100, 60).build()])]
```

`post_with`/`put_with`/`delete_with`/`patch_with`/`options_with`/`head_with`/`all_with`
provide the same inline form.

> Note: the registration code generated by the attribute macro is within the scope of the `impl` block, so the middleware array can only reference **instance methods**,
> **associated functions**, **global functions**, and **module-level items** (free functions, `logger!()`/`v!(...)` macro calls,
> self-contained expressions such as `RateLimitConfig::new(...).build()`), and cannot reference local variables inside `main`/closures.

#### Built-in Middleware

| Middleware | Description | Usage |
|--------|------|------|
| `logger!()` | Request logging | `logger!()` |
| `v!(...)` | DSL parameter validation | `v!(name => "required")` |
| `RateLimitConfig` | Rate limiting (by IP / header / path) | `RateLimitConfig::new(100, 60).by_ip().build()` |
| `CorsConfig` | CORS | `CorsConfig::new().allow_origin_all(true).build()` |
| `WebSocket` | WebSocket upgrade | `WebSocket::to_middleware(ws)` |

#### Low Investment, High Return, One-time Effort

- The contract has **never changed** since the framework was born: one `Executor`, one `bool`, two rules.
- Functional and object-based middleware **share the same contract**; write once and all three mounting styles — chained / inline / attribute macro — **work everywhere**, with no need to adapt for each.
- No tower/layer, no extractor, no rejection — **no extra abstraction to learn**; if you can write a closure and a method, you can write middleware.
- Middleware depends only on the stable `Context` and the optional `&self`, unaffected by the evolution of the routing API; once written, it can be reused long-term and moved across projects.

---

## Unified Server API

| Method | Description |
|------|------|
| `.new(globals)` / `Server::default()` | Construct (shared context / independent context) |
| `.push(protocol)` | Mount a protocol implementing `Protocol` (HTTP / HTTP2 / WebSocket / TCP / UDP / custom) |
| `.mount(protocol)` | Same as `push`, mounting in `&mut self` form |
| `.detector(d)` / `.detector_at(pos, d)` | Register a protocol detector (tail / specified position) |
| `.with_registry(registry)` | Share an externally managed detector registry (runtime add/remove) |
| `.detection(bool)` | Toggle the detection phase; when off, connections go straight to the default TCP handler with no sniffing |
| `.start(addr)` | Start the unified server |

> The recommended top-level handlers are `Tcp` (stream) and `Udp` (datagram); HTTP, HTTP/2, WebSocket etc. are usually
> sub-handlers of `Tcp` (written with the macros as `http!(router)`). When serving only streaming protocols you may also directly `push(http!(router))`.

> Connection attributes: before dispatching, the unified server writes `ConnectionFd` (the raw file descriptor) into the ctx,
> so the split read/write halves can still perform fd-level operations (such as querying the original destination `SO_ORIGINAL_DST`).

### Use Cases

| Scenario | Protocols used |
|------|----------|
| REST API | HTTP/1.1, HTTP/2 |
| Real-time chat | HTTP/1.1 + WebSocket, HTTP/2 + WebSocket |
| Game server | TCP/UDP |
| IoT gateway | HTTP + TCP + UDP |
| Real-time push | HTTP + SSE |

---

## TCP Protocol

```rust
use aex::bytes::Bytes;
use aex::tcp::router::Router as TcpRouter;
use futures::FutureExt;

// 1. Create a TCP router and install a byte-level extractor.
//    extractor signature: Fn(&[u8]) -> anyhow::Result<Option<(usize, u32, Bytes, Bytes)>>
//      Ok(Some((consumed_bytes, dispatch_key, frame_bytes, command_bytes))) —— parsed one complete frame
//      Ok(None) —— half packet / not enough, wait for more bytes
//      Err(..)  —— parsed successfully but validation failed, meaning it is not this protocol (the GlobalContext policy decides drop or disconnect)
let mut tcp_router = TcpRouter::new().extractor(|buf: &[u8]| {
    // Parse buf here into (consumed, key, frame_bytes, cmd_bytes)
    Ok(None)
});

// 2. Register a handler (command ID = 1). The handler receives Bytes directly and parses them itself.
tcp_router.on(
    1,
    Box::new(|_ctx, _frame: Bytes, _cmd: Bytes| async move { Ok(true) }.boxed()),
    vec![],
);
```

> The router itself **contains no business types**: everything sent and received is `Bytes`. The protocol's encoding/decoding (`Bytes → Codec →
> Frame/Command`) is done by the extractor and handler.

### TCP Frame/Command Definitions

```rust
use aex::tcp::types::{Codec, Command, Frame};
use serde::{Deserialize, Serialize};
use bincode::{Decode, Encode};

#[derive(Clone, Debug, Serialize, Deserialize, Encode, Decode)]
struct MyFrame {
    data: Vec<u8>,
}

impl Frame for MyFrame {
    fn payload(&self) -> Option<Vec<u8>> { Some(self.data.clone()) }
    fn validate(&self) -> bool { true }
    fn command(&self) -> Option<&Vec<u8>> { Some(&self.data) }
    fn is_flat(&self) -> bool { false }
}

#[derive(Clone, Debug, Serialize, Deserialize, Encode, Decode)]
struct MyCommand {
    id: u32,
    data: Vec<u8>,
}

impl Command for MyCommand {
    fn id(&self) -> u32 { self.id }
    fn data(&self) -> &Vec<u8> { &self.data }
}
```

---

## UDP Protocol

```rust
use aex::bytes::Bytes;
use aex::udp::router::Router as UdpRouter;

// Create a UDP router and install an extractor.
//   extractor signature: Fn(&[u8]) -> anyhow::Result<Option<(u32, Bytes, Bytes)>>
//     each datagram is one complete message; returns (dispatch_key, frame_bytes, command_bytes).
let mut udp_router = UdpRouter::new().extractor(|buf: &[u8]| {
    // Parse buf to get (key, frame_bytes, cmd_bytes); return Ok(None) if it cannot be parsed
    Ok(None)
});

// Register a handler (the handler receives Bytes directly and parses them itself)
udp_router.on(100, |_global, _frame: Bytes, _cmd: Bytes, addr, _socket| {
    Box::pin(async move {
        println!("UDP packet from {}", addr);
        Ok(true)
    })
});
```

---

## P2P: Low-level Infrastructure

> **P2P is not a protocol, but a low-level infrastructure / service capability**: it targets nodes, providing
> generic capabilities such as NAT traversal, handshaking, and connection management for upper-layer services to reuse. It does not occupy a protocol branch,
> nor does it appear in the protocol list of the [universal server state machine](#design-philosophy-a-universal-server-state-machine-containing-the-web-straight-line).

Aex has a built-in **IP-identification-based** P2P infrastructure, supporting decentralized network communication.

### Core Concepts

```
┌─────────────────────────────────────────────────────────────┐
│                          P2P node                           │
├─────────────────────────────────────────────────────────────┤
│  Node {                                                    │
│    id: Vec<u8>,      // node ID, usually a public-key hash  │
│    version: u8,     // protocol version                     │
│    started_at: u64, // startup timestamp                    │
│    port: u16,       // listen port                          │
│    protocols: HashSet<Protocol>,  // list of supported protocols │
│    ips: Vec<(NetworkScope, IpAddr)>,  // list of network addresses │
│  }                                                         │
└─────────────────────────────────────────────────────────────┘
```

### Command ID Definitions

| CommandId | Value | Description |
|----------|-----|------|
| Hello | 1 | Handshake request (includes node info) |
| Welcome | 2 | Handshake response (accept/reject) |
| Ack | 3 | Acknowledge handshake complete |
| Reject | 4 | Reject connection |
| Ping | 5 | Heartbeat request |
| Pong | 6 | Heartbeat response |

```rust
use aex::connection::commands::CommandId;

assert_eq!(CommandId::Hello.as_u32(), 1);
assert_eq!(CommandId::Pong.as_u32(), 6);
```

### Connection State Machine

```
┌─────────────────────────────────────────────────────────────┐
│                  Connection State Machine                   │
├─────────────────────────────────────────────────────────────┤
│  Initial ──→ Connecting ──→ Handshake ──→ Established        │
│     │           │              │              │             │
│     │           │              │              ↓             │
│     │           │              │         Active             │
│     │           │              │              │             │
│     │           │              │              ↓             │
│     │           │              └─────── Disconnecting        │
│     │           │                         │                  │
│     │           └─────────────────→ Disconnected             │
│     │                          ↑                               │
│     └──────────────────────────┘                              │
└─────────────────────────────────────────────────────────────┘
```

```rust
use aex::connection::state_machine::{ConnectionStateMachine, ConnectionState};

let sm = ConnectionStateMachine::new();
sm.transition(ConnectionState::Connecting);
sm.transition(ConnectionState::Handshake);
sm.transition(ConnectionState::Established);
sm.transition(ConnectionState::Active);

assert!(sm.is_active());
assert!(sm.should_heartbeat());
```

### Handshake Protocol

```
┌─────────────────────────────────────────────────────────────┐
│                     P2P handshake flow                      │
├─────────────────────────────────────────────────────────────┤
│   Client                                Server               │
│     │                                     │                 │
│     │───────── CMD_HELLO ─────────────────→│                 │
│     │  (version, node, ephemeral_pub)      │                 │
│     │                                     │                 │
│     │←──────── CMD_WELCOME ────────────────│                 │
│     │  (version, node, accepted, pub)     │                 │
│     │                                     │                 │
│     │───────── CMD_ACK ─────────────────→│                 │
│     │  (accepted, session_key_id?)        │                 │
│     │                                     │                 │
│     │         [encrypted channel]         │                  │
└─────────────────────────────────────────────────────────────┘
```

### Heartbeat Protocol

```rust
use aex::connection::heartbeat::{HeartbeatManager, HeartbeatConfig};

let config = HeartbeatConfig::new()
    .with_interval(30)   // 30-second interval
    .with_timeout(10)     // 10-second timeout
    .on_timeout(|addr| {
        println!("connection timeout: {}", addr);
    })
    .on_latency(|addr, latency| {
        println!("latency {} ms", latency);
    });

let manager = HeartbeatManager::new(local_node).with_config(config);
```

---

## Communicators

### Pipe - N:1 Pipeline

Multiple senders → one consumer (suitable for logging, auditing):

```rust
use futures::FutureExt;

server.globals.pipe::<String>("audit_log", Box::new(|msg| {
    async move { write_to_file(msg).await }.boxed()
})).await;

server.globals.pipe.send("audit_log", "User logged in".to_string()).await;
```

### Spreader - 1:N Broadcast

One sender → multiple consumers (suitable for config synchronization):

```rust
use futures::FutureExt;

server.globals.spread::<i32>("config_sync", Box::new(|val| {
    async move { update_config(val).await }.boxed()
})).await;

server.globals.spread.publish("config_sync", 42).await;
```

### Event - M:N Event System

Multiple senders → multiple consumers (suitable for business events):

```rust
use aex::communicators::event::Event;
use futures::FutureExt;

server.globals.event::<u32>("user_login", Arc::new(|uid| {
    async move { notify_admins(uid).await }.boxed()
})).await;

Event::<u32>::notify(&server.globals.event, "user_login".to_string(), 888).await;
```

---

## Architecture

### Multi-layer Architecture Design

```
┌─────────────────────────────────────────────────────────────┐
│                    Application Layer                       │
│  ┌─────────────────────────────────────────────────────┐   │
│  │              Executor Chain                         │   │
│  │  [Middleware 1] → [Middleware 2] → [Handler]       │   │
│  └─────────────────────────────────────────────────────┘   │
├─────────────────────────────────────────────────────────────┤
│                      Router Layer                           │
│  ┌──────────────┐ ┌──────────────┐ ┌──────────────┐         │
│  │ HTTP Router │ │ TCP Router   │ │ UDP Router   │         │
│  │  (Trie)     │ │  (Map)       │ │  (Map)       │         │
│  └──────────────┘ └──────────────┘ └──────────────┘         │
├─────────────────────────────────────────────────────────────┤
│                    Protocol Layer                           │
│  ┌──────────────┐ ┌──────────────┐ ┌──────────────┐         │
│  │ HTTP/1.1     │ │ HTTP/2       │ │ TCP Frame    │         │
│  │ WebSocket    │ │ WebSocket    │ │ Codec        │         │
│  └──────────────┘ └──────────────┘ └──────────────┘         │
│  ┌──────────────┐                                           │
│  │ UDP Packet   │                                           │
│  └──────────────┘                                           │
├─────────────────────────────────────────────────────────────┤
│                    Transport Layer                          │
│  ┌──────────────────────────────────────────────────┐        │
│  │ Unified TCP Listener (Protocol Detection)        │        │
│  │ + UDP Socket                                       │        │
│  └──────────────────────────────────────────────────┘        │
└─────────────────────────────────────────────────────────────┘
```

### Core Components

| Component | Responsibility | Feature |
|------|------|------|
| **Server** | Unified multi-protocol entry | HTTP/TCP/H2 share a port |
| **Router** | Route matching | Trie tree / HashMap |
| **Executor** | Handler | BoxFuture async |
| **Context** | Request context | TypeMap storage |
| **ConnectionManager** | Connection pool | DashMap concurrency |
| **GlobalContext** | Globally shared state | Cross-connection communication |
| **SessionKeyManager** | Encrypted session management | End-to-end encryption |
| **Pipe** | N:1 message pipeline | Logging/auditing |
| **Spreader** | 1:N broadcast | Config synchronization |
| **Event** | M:N event system | Event notification |

---

## Comparison with Other Frameworks

### Protocol Support Comparison

| Protocol | Aex | Axum | Actix-web |
|------|-----|------|----------|
| HTTP/1.1 + WebSocket | ✅ | ✅ | ✅ |
| HTTP/2 + WebSocket | ✅ | ✅ | ✅ |
| **Unified-port multi-protocol** | ✅ | ❌ | ❌ |
| Custom TCP | ✅ | ❌ | ❌ |
| UDP | ✅ | ❌ | ✅ |
| mDNS | ✅ | ❌ | ❌ |

### Aex Design Principles

  1. **Explicit over implicit** - linear middleware chain with predictable control flow
  2. **Lightweight over heavy** - minimal dependencies, facing the core problem directly
  3. **Performance first** - ahash + Trie-tree optimization
  4. **The essence of HTTP** - respecting HTTP's design
  5. **Unified architecture** - all protocols on the same port

### Performance Comparison (AEX vs Axum vs Actix-web)

wrk benchmark (release build, `t4-c500` high-concurrency 5s):

| Route | AEX QPS | Axum 0.8.9 QPS | Actix-web 4 QPS | AEX vs Axum | AEX vs Actix |
|------|---------|-----------------|-----------------|-------------|-------------|
| `/` | 130,455 | 118,448 | 141,612 | **1.10x** | 0.92x |
| `/api/users` | 132,157 | 114,872 | 128,422 | **1.15x** | **1.02x** |
| `/api/users/{id}` | 116,345 | 97,831 | 110,414 | **1.18x** | **1.05x** |

AEX outperforms Axum on both static and dynamic routes, with the biggest advantage on dynamic parameter routes (1.18x). Compared with Actix-web, AEX performs better on routes with path parameters.

### Use Cases

- High-performance API services
- WebSocket applications
- TCP/UDP hybrid services
- Microservice architectures
- Resource-constrained environments

---

## Module Structure

```
aex/
├── protocols/          # all protocol handlers (`Protocol` trait implementations)
│   ├── http/           # HTTP/1.1 framework
│   │   ├── router.rs   # Trie-tree router
│   │   ├── types.rs    # Executor type definitions
│   │   ├── meta.rs     # HTTP metadata
│   │   ├── req.rs      # request parsing
│   │   ├── res.rs      # response handling
│   │   ├── params.rs   # path/query/form parameters
│   │   ├── websocket.rs# WebSocket frames
│   │   ├── macros.rs   # HTTP method macros / validator!
│   │   └── middlewares/# built-in middleware (cors/logger/rate_limit/validator/websocket)
│   ├── http1.rs        # Http1 protocol (HTTP/1.1 + default WebSocket sub-handler)
│   ├── http2/          # HTTP/2
│   │   ├── mod.rs      # H2Codec codec
│   │   └── protocol.rs # Http2 protocol
│   ├── tcp/            # TCP
│   │   ├── protocol.rs # Tcp protocol + router_handler/from_handler
│   │   ├── router.rs   # command router (extractor + handler)
│   │   ├── types.rs    # Frame/Command trait
│   │   ├── listeners.rs# TCP listeners
│   │   └── macros.rs   # frame!/command!/codec!/router!
│   ├── udp/            # UDP
│   │   ├── protocol.rs # Udp protocol
│   │   ├── router.rs   # datagram router
│   │   └── types.rs    # UDP types
│   ├── websocket.rs    # WebSocket protocol (HTTP upgrade middleware form)
│   ├── socks.rs        # SOCKS4/5 protocol (feature = "proxy")
│   ├── proxy.rs        # proxy protocol entry
│   └── classify.rs     # protocol classification helpers (HTTP methods, H2 preface…)
│
├── p2p/                # P2P low-level infrastructure (NAT traversal etc., not a protocol)
│   └── nat/            # NAT traversal (below the transport layer, operating sockets directly)
│       ├── client.rs   # client/tunnel
│       ├── server.rs   # relay service
│       ├── punch.rs    # hole-punching coordination
│       ├── service.rs  # NatDetector + unified service registration
│       └── types.rs    # frame/error types
│
├── server.rs           # unified server ⭐ (Server::push/start)
├── protocol.rs         # Protocol trait + Transport (unified protocol interface)
├── detect.rs           # pluggable protocol detector system (registration/ordering/conflict protection)
├── handler.rs          # ProtocolHandler / CommandRegistry / HandlerState
│
├── connection/         # connection management
│   ├── context.rs      # per-request Context
│   ├── global.rs       # global context (including FrameErrorPolicy)
│   ├── manager.rs      # connection pool management
│   └── types.rs        # connection types
│
├── crypto/             # crypto support
│   └── session_key_manager.rs  # X25519 + ChaCha20Poly1305
│
├── communicators/      # IPC patterns
│   ├── spreader.rs     # Pub/Sub broadcast
│   ├── event.rs        # event system
│   └── pipe.rs         # named pipes
│
├── proxy/              # HTTP / SOCKS proxy implementation (feature = "proxy")
├── main.rs             # `http-server` executable entry
└── lib.rs              # top-level modules and re-exports (http/http2/tcp/udp/nat)
```

---

## Testing

```bash
cargo test --all          # all unit + integration tests
cargo test --lib unified  # detector system unit tests
```

Test files related to the unified server and detector system:

- `tests/unified_server_test.rs` - coexistence of HTTP/1.1, WebSocket, HTTP/2, and TCP on a unified server
- `tests/unified_listener_all_protocols_test.rs` - multi-protocol detection on the same listening port (explicit detector registration)
- `tests/unified_detect_pipeline_test.rs` - end-to-end detection pipeline: custom protocol dispatch, Forward-mode direct forwarding, pure TCP passthrough with an empty registry, runtime detector unregistration
- Built-in unit tests in `src/detect.rs` - registration ordering/add/remove/replace, conflict rejection (bidirectional), first-Match short-circuit, multi-round NeedMore decisions, abandoning at the 16 KiB limit, Forward termination semantics

---

## License

GPL-3.0
