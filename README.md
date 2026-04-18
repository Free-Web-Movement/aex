# Aex — Async-first, Executor-based Web/TCP/UDP Framework

> 一个轻量、可控、忠于 HTTP 本质的 Rust 多协议框架

[![Rust](https://img.shields.io/badge/rust-1.75%2B-blue.svg)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/license-GPL--3.0-green.svg)](LICENSE)
[![ crates.io version](https://img.shields.io/crates/v/aex.svg)](https://crates.io/crates/aex)
[![crates.io downloads](https://img.shields.io/crates/d/aex.svg)](https://crates.io/crates/aex)

## 版本

当前版本: **0.1.5**

```toml
[dependencies]
aex = "0.1.5"
tokio = { version = "1", features = ["full"] }
anyhow = "1"
```

## 核心特性

- **直觉的 HTTP 路由** - Trie 树路由，支持静态路径、参数路径、通配符
- **显式中间件链** - 线性执行顺序，可预测的控制流（非洋葱模型）
- **原生 WebSocket 支持** - 作为中间件自然集成，共享 HTTP 上下文
- **多协议支持** - HTTP/1.1、HTTP/2、TCP、UDP 服务器统一接口
- **TypeMap 扩展** - 灵活的请求/响应数据存储
- **端到端加密** - ChaCha20-Poly1305 会话加密
- **IPC 通信器** - Pipe、Spreader、Event 模式
- **P2P 框架** - 基于 IP 识别的去中心化网络

---

## HTTP 快速开始

### Hello World

```rust
use aex::http::router::{NodeType, Router as HttpRouter};
use aex::server::HTTPServer;
use aex::exe;
use std::net::SocketAddr;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let addr: SocketAddr = "0.0.0.0:8080".parse()?;
    let mut router = HttpRouter::new(NodeType::Static("root".into()));

    router.get("/", exe!(|ctx| {
        ctx.send("Hello, World!", None);
        true
    })).register();

    HTTPServer::new(addr, None)
        .http(router)
        .start()
        .await?;
    Ok(())
}
```

### HTTP 路由详解

```rust
use aex::http::router::{NodeType, Router as HttpRouter, PathParams};
use aex::exe;

// 1. 创建路由器
let mut router = HttpRouter::new(NodeType::Static("root".into()));

// 2. 静态路由
router.get("/api/health", exe!(|ctx| {
    ctx.send("OK", None);
    true
})).register();

// 3. 参数路由
router.get("/api/users/:id", exe!(|ctx| {
    let params = ctx.local.get_ref::<PathParams>();
    if let Some(p) = params {
        let id = p.get("id");
        ctx.send(format!("User: {}", id), None);
    }
    true
})).register();

// 4. 通配符路由
router.get("/api/files/*", exe!(|ctx| {
    let params = ctx.local.get_ref::<PathParams>();
    if let Some(p) = params {
        let path = p.get("*");
        ctx.send(format!("File: {}", path), None);
    }
    true
})).register();

// 5. 带中间件的路由
router.post("/api/users", exe!(|ctx| {
    ctx.send("Created", None);
    true
}).middleware(auth_middleware).register();
```

### HTTP/2 支持

HTTP/2 与 HTTP/1.1 共用同一个 router：

```rust
use aex::server::HTTPServer;
use aex::http::router::{NodeType, Router as HttpRouter};
use aex::tcp::types::RawCodec;
use aex::exe;
use std::net::SocketAddr;
use std::sync::Arc;

HTTPServer::new(addr, None)
    .http(router)    // HTTP/1.1
    .http2()         // HTTP/2 (使用同一个 router)
    .start()
    .await?;
```

### WebSocket 支持

WebSocket 作为中间件实现，共享 HTTP 上下文：

```rust
use aex::http::websocket::{TextHandler, BinaryHandler, WebSocket};
use aex::exe;

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

router.get("/ws", exe!(|_ctx| true))
    .middleware(WebSocket::to_middleware(ws))
    .register();
```

### 中间件

中间件是 Executor 的有序数组，按声明顺序执行：

```rust
router.get("/protected", exe!(|ctx| {
    ctx.send("Protected resource", None);
    true
}).middleware(auth_middleware).middleware(logging_middleware).register();
```

---

## 统一服务器架构

Aex 是目前 Rust 生态中**协议支持最全面**的 web 框架之一，一套代码同时支持多种协议。

### 支持的协议

```
┌─────────────────────────────────────────────────────────────┐
│                    Aex 协议支持                              │
├─────────────────────────────────────────────────────────────┤
│ 传输层协议      │ 应用层协议      │ 状态      │ 说明             │
├─────────────────────────────────────────────────────────────┤
│  HTTP/1.1     │ HTTP/WebSocket │ ✅ 已支持 │ 完整 HTTP 语义   │
│  HTTP/2       │ HTTP/WebSocket │ ✅ 已支持 │ 多路复用         │
│  WebSocket    │ WS             │ ✅ 已支持 │ 双向通信         │
│  Server-Sent  │ SSE            │ ✅ 已支持 │ 服务推送         │
├─────────────────────────────────────────────────────────────┤
│  TCP          │ 自定义协议       │ ✅ 已支持 │ 二进制帧编解码    │
│  UDP          │ 数据报          │ ✅ 已支持 │ 无连接通信       │
│  mDNS         │ 发现服务         │ ✅ 已支持 │ 局域网发现       │
└─────────────────────────────────────────────────────────────┘
```

### 多协议服务器

```rust
use aex::server::HTTPServer;
use aex::http::router::{NodeType, Router as HttpRouter};
use aex::tcp::router::Router as TcpRouter;
use aex::udp::router::Router as UdpRouter;
use aex::tcp::types::RawCodec;
use aex::exe;
use std::net::SocketAddr;
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let addr: SocketAddr = "0.0.0.0:8080".parse()?;

    // HTTP 路由
    let mut http_router = HttpRouter::new(NodeType::Static("root".into()));
    http_router.get("/", exe!(|ctx| {
        ctx.send("Hello!", None);
        true
    })).register();

    // TCP 路由 (命令 ID 10)
    let mut tcp_router = TcpRouter::new();
    tcp_router.on::<RawCodec, RawCodec>(
        10,
        Box::new(|_, _, _| Box::pin(async move { Ok(true) }).boxed()),
        vec![],
    );

    // UDP 路由 (命令 ID 20)
    let mut udp_router = UdpRouter::new();
    udp_router.on::<RawCodec, RawCodec, _, _>(20, |_, _, _, addr, socket| async move {
        socket.send_to(b"ok", addr).await?;
        Ok(true)
    });

    // 启动服务器 - 泛型在 tcp/udp 方法中指定
    HTTPServer::new(addr, None)
        .http(http_router)              // HTTP/1.1 路由
        .http2()                       // 启用 HTTP/2 (可选)
        .tcp(tcp_router, Arc::new(|c: &RawCodec| c.id()))   // TCP 路由 + extractor
        .udp(udp_router, Arc::new(|c: &RawCodec| c.id()))   // UDP 路由 + extractor
        .start()
        .await?;
    Ok(())
}
```

### API 设计

| 方法 | 说明 |
|------|------|
| `.http(router)` | 设置 HTTP/1.1 路由 |
| `.http2()` | 启用 HTTP/2 (使用同一个 HTTP router) |
| `.tcp(router, extractor)` | 设置 TCP 路由，泛型在方法参数中指定 |
| `.udp(router, extractor)` | 设置 UDP 路由，泛型在方法参数中指定 |
| `.start()` | 启动服务器 |

### 协议嗅探

Aex 自动检测连接类型，无需手动配置：

```
TCP 连接 → 自动嗅探 → HTTP/1.1+WebSocket / HTTP/2+WebSocket / TCP 处理器
```

### 适用场景

| 场景 | 使用的协议 |
|------|----------|
| REST API | HTTP/1.1, HTTP/2 |
| 实时聊天 | HTTP/1.1 + WebSocket, HTTP/2 + WebSocket |
| 游戏服务器 | TCP/UDP |
| 物联网网关 | HTTP + TCP + UDP |
| 局域网服务发现 | mDNS |
| 实时推送 | HTTP + SSE |

---

## TCP 协议

```rust
use aex::tcp::router::Router as TcpRouter;
use aex::tcp::types::{Codec, Command, Frame, RawCodec};
use aex::connection::global::GlobalContext;
use std::sync::Arc;
use std::net::SocketAddr;

// 1. 创建 TCP 路由器
let mut tcp_router = TcpRouter::new();

// 注册命令处理器 (命令 ID = 1)
tcp_router.on::<RawCodec, RawCodec, _, _>(1, |_global, _frame, cmd, _addr, _socket| {
    Box::pin(async move {
        println!("Received command: {}", cmd.id());
        Ok(true)
    })
});
```

### TCP 帧/命令定义

```rust
use aex::tcp::types::{Codec, Command, Frame};

#[derive(Clone, Debug)]
struct MyFrame {
    data: Vec<u8>,
}

impl Frame for MyFrame {
    fn payload(&self) -> Option<Vec<u8>> { Some(self.data.clone()) }
    fn validate(&self) -> bool { true }
    fn command(&self) -> Option<&Vec<u8>> { Some(&self.data) }
    fn is_flat(&self) -> bool { false }
}

impl Codec for MyFrame {}

#[derive(Clone, Debug)]
struct MyCommand {
    id: u32,
    data: Vec<u8>,
}

impl Command for MyCommand {
    fn id(&self) -> u32 { self.id }
    fn validate(&self) -> bool { true }
    fn data(&self) -> &Vec<u8> { &self.data }
}

impl Codec for MyCommand {}
```

---

## UDP 协议

```rust
use aex::udp::router::Router as UdpRouter;
use aex::tcp::types::{Codec, Command, Frame, RawCodec};
use std::sync::Arc;

// 创建 UDP 路由器
let mut udp_router = UdpRouter::new();

// 注册处理器
udp_router.on::<RawCodec, RawCodec, _, _>(100, |global, frame, cmd, addr, socket| {
    Box::pin(async move {
        println!("UDP packet from {}: cmd_id={}", addr, cmd.id());
        Ok(true)
    })
});
```

---

## P2P 框架

Aex 内置基于 **IP 识别** 的 P2P 框架，支持去中心化网络通信。

### 核心概念

```
┌─────────────────────────────────────────────────────────────┐
│                      P2P 节点                               │
├─────────────────────────────────────────────────────────────┤
│  Node {                                                    │
│    id: Vec<u8>,      // 节点 ID，通常是公钥哈希             │
│    version: u8,     // 协议版本                            │
│    started_at: u64, // 启动时间戳                          │
│    port: u16,       // 监听端口                            │
│    protocols: HashSet<Protocol>,  // 支持的协议列表          │
│    ips: Vec<(NetworkScope, IpAddr)>,  // 网络地址列表       │
│  }                                                         │
└──────────────────────────────────────��─��────────────────────┘
```

### 命令 ID 定义

| CommandId | 值 | 说明 |
|----------|-----|------|
| Hello | 1 | 握手请求 (含节点信息) |
| Welcome | 2 | 握手响应 (接受/拒绝) |
| Ack | 3 | 确认握手完成 |
| Reject | 4 | 拒绝连接 |
| Ping | 5 | 心跳请求 |
| Pong | 6 | 心跳响应 |

```rust
use aex::connection::commands::CommandId;

assert_eq!(CommandId::Hello.as_u32(), 1);
assert_eq!(CommandId::Pong.as_u32(), 6);
```

### 连接状态机

```
┌─────────────────────────────────────────────────────────────┐
│              连接状态机 (ConnectionStateMachine)          │
├─────────────────────────────────────────────────────────────┤
│  Initial ──→ Connecting ──→ Handshake ──→ Established      │
│     │           │              │              │                 │
│     │           │              │              ↓                 │
│     │           │              │         Active                │
│     │           │              │              │                   │
│     │           │              │              ↓                 │
│     │           │              └─────── Disconnecting          │
│     │           │                         │                   │
│     │           └─────────────────→ Disconnected           │
│     │                          ↑                          │
│     └──────────────────────────┘                          │
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

### 握手协议

```
┌─────────────────────────────────────────────────────────────┐
│                   P2P 握手协议流程                        │
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
│     │         [加密通道建立]                │                 │
└─────────────────────────────────────────────────────────────┘
```

### 心跳协议

```rust
use aex::connection::heartbeat::{HeartbeatManager, HeartbeatConfig};

let config = HeartbeatConfig::new()
    .with_interval(30)   // 30秒间隔
    .with_timeout(10)     // 10秒超时
    .on_timeout(|addr| {
        println!("连接超时: {}", addr);
    })
    .on_latency(|addr, latency| {
        println!("延迟 {} ms", latency);
    });

let manager = HeartbeatManager::new(local_node).with_config(config);
```

---

## 通信器 (Communicators)

### Pipe - N:1 管道

多个发送者 → 一个消费者（适用于日志、审计）：

```rust
server.globals.pipe::<String>("audit_log", Box::new(|msg| {
    async move { write_to_file(msg).await }
})).await;

server.globals.pipe.send("audit_log", "User logged in".to_string()).await;
```

### Spreader - 1:N 广播

一个发送者 → 多个消费者（适用于配置同步）：

```rust
server.globals.spread::<i32>("config_sync", Box::new(|val| {
    async move { update_config(val).await }
})).await;

server.globals.spread.publish("config_sync", 42).await;
```

### Event - M:N 事件系统

多个发送者 → 多个消费者（适用于业务事件）：

```rust
server.globals.event::<u32>("user_login", Arc::new(|uid| {
    async move { notify_admins(uid).await }
})).await;

server.globals.event.notify("user_login".to_string(), 888).await;
```

---

## 架构层面

### 多层架构设计

```
┌─────────────────────────────────────────────────────────────┐
│                    Application Layer                      │
│  ┌─────────────────────────────────────────────────────┐  │
│  │              Executor Chain                        │  │
│  │  [Middleware 1] → [Middleware 2] → [Handler]      │  │
│  └─────────────────────────────────────────────────────┘  │
├─────────────────────────────────────────────────────────────┤
│                      Router Layer                        │
│  ┌──────────────┐ ┌──────────────┐ ┌──────────────┐    │
│  │ HTTP Router  │ │ TCP Router   │ │ UDP Router   │    │
│  │  (Trie)      │ │  (Map)       │ │  (Map)       │    │
│  └──────────────┘ └──────────────┘ └──────────────┘    │
├─────────────────────────────────────────────────────────────┤
│                    Protocol Layer                        │
│  ┌──────────────┐ ┌──────────────┐ ┌──────────────┐    │
│  │ HTTP/1.1    │ │ HTTP/2      │ │ TCP Frame   │    │
│  │ WebSocket   │ │ WebSocket   │ │ Codec      │    │
│  └──────────────┘ └──────────────┘ └──────────────┘    │
│  ┌──────────────┐                                    │
│  │ UDP Packet  │                                    │
│  │ Codec      │                                    │
│  └──────────────┘                                    │
├─────────────────────────────────────────────────────────────┤
│                    Transport Layer                      │
│  ┌──────────────┐ ┌──────────────┐                      │
│  │ TCP Listener│ │ UDP Socket  │                      │
│  └──────────────┘ └──────────────┘                      │
└─────────────────────────────────────────────────────────────┘
```

### 核心组件

| 组件 | 职责 | 特点 |
|------|------|------|
| **Server** | 统一入口 | HTTP/TCP/UDP 共享 |
| **Router** | 路由匹配 | Trie 树 / HashMap |
| **Executor** | 处理器 | BoxFuture 异步 |
| **Context** | 请求上下文 | TypeMap 存储 |
| **ConnectionManager** | 连接池 | DashMap 并发 |
| **GlobalContext** | 全局共享状态 | 跨连接通信 |
| **SessionKeyManager** | 加密会话管理 | 端到端加密 |
| **Pipe** | N:1 消息管道 | 日志/审计 |
| **Spreader** | 1:N 广播 | 配置同步 |
| **Event** | M:N 事件系统 | 事件通知 |

---

## 与其他框架对比

### 协议支持对比

| 协议 | Aex | Axum | Actix-web |
|------|-----|------|----------|
| HTTP/1.1 + WebSocket | ✅ | ✅ | ✅ |
| HTTP/2 + WebSocket | ✅ | ✅ | ✅ |
| TCP 自定义 | ✅ | ❌ | ❌ |
| UDP | ✅ | ❌ | ✅ |
| mDNS | ✅ | ❌ | ❌ |

### Aex 设计理念

1. **显式优于隐式** - 线性中间件链，控制流可预测
2. **轻量优于重** - 最少依赖，直面核心问题
3. **性能优��** - ahash + Trie 树优化
4. **HTTP 本质** - 尊重 HTTP 协议设计

### 适用场景

- 高性能 API 服务
- WebSocket 应用
- TCP/UDP 混合服务
- 微服务架构
- 资源受限环境

---

## 模块结构

```
aex/
├── http/               # HTTP Web 框架
│   ├── router.rs      # Trie 树路由器
│   ├── types.rs       # Executor 类型定义
│   ├── meta.rs        # HTTP 元数据
│   ├── req.rs        # 请求解析
│   ├── res.rs        # 响应处理
│   ├── params.rs     # 路径/查询/表单参数
│   ├── websocket.rs   # WebSocket 支持
│   ├── macros.rs      # HTTP 方法宏
│   └── middlewares/   # 内置中间件
│
├── http2/             # HTTP/2 协议支持
│   └── mod.rs        # H2Codec 编解码器
│
├── tcp/               # TCP 协议支持
│   ├── router.rs      # 命令路由器
│   ├── types.rs       # Frame/Command trait
│   └── listeners.rs   # TCP 监听器
│
├── udp/               # UDP 协议支持
│   ├── router.rs      # 数据包路由器
│   └── types.rs       # UDP 类型
│
├── connection/         # 连接管理
│   ├── context.rs     # Per-request Context
│   ├── global.rs      # 全局上下文
│   ├── manager.rs     # 连接池管理
│   └── types.rs       # 连接类型
│
├── crypto/            # 加密支持
│   └── zero_trust_session_key.rs  # X25519 + ChaCha20Poly1305
│
├── communicators/     # IPC 模式
│   ├── spreader.rs    # Pub/Sub 广播
│   ├── event.rs      # 事件系统
│   └── pipe.rs       # 命名管道
│
└── server.rs          # 统一服务器入口
```

---

## License

GPL-3.0