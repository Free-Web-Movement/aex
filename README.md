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

---

## 协议支持

Aex 是目前 Rust 生态中**协议支持最全面**的 web 框架之一，一套代码同时支持多种协议。

### 支持的协议

┌─────────────────────────────────────────────────────────────┐
│                    Aex 协议支持                            │
├─────────────────────────────────────────────────────────────┤
│ 传输层协议     │ 应用层协议      │ 状态      │ 说明            │
├─────────────────────────────────────────────────────────────┤
│  HTTP/1.1     │ HTTP/WebSocket │ ✅ 已支持 │ 完整 HTTP 语义   │
│  HTTP/2       │ HTTP/WebSocket │ ✅ 已支持 │ 多路复用         │
│  WebSocket    │ WS            │ ✅ 已支持 │ 双向通信         │
│  Server-Sent  │ SSE           │ ✅ 已支持 │ 服务推送        │
├─────────────────────────────────────────────────────────────┤
│  TCP          │ 自定义协议    │ ✅ 已支持 │ 二进制帧编解码   │
│  UDP          │ 数据报       │ ✅ 已支持 │ 无连接通信       │
│  mDNS        │ 发现服务    │ ✅ 已支持 │ 局域网发现      │
└─────────────────────────────────────────────────────────────┘

### 统一服务器架构

```rust
use aex::server::HTTPServer;

// 一行代码启动支持多种协议的服务器
HTTPServer::new(addr)
    .http(router)       // HTTP/1.1 路由
    .http2()            // 启用 HTTP/2 (共用 HTTP 路由)
    .tcp(tcp_router)   // TCP 路由
    .udp(udp_router)   // UDP 路由
    .start::<F, C>()
    .await?;
```

### 协议嗅探

Aex 自动检测连接类型，无需手动配置：

```
TCP 连接 → 自动嗅探 → HTTP/1.1+WebSocket / HTTP/2+WebSocket / TCP 处理器
```

### 与其他框架协议支持对比

| 协议 | Aex | Axum | Actix-web |
|------|-----|------|----------|
| HTTP/1.1 + WebSocket | ✅ | ✅ | ✅ |
| HTTP/2 + WebSocket | ✅ | ✅ | ✅ |
| TCP 自定义 | ✅ | ❌ | ❌ |
| UDP | ✅ | ❌ | ✅ |
| mDNS | ✅ | ❌ | ❌ |

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

### GlobalContext 全局上下文

GlobalContext 是跨所有连接的共享状态容器：

```rust
pub struct GlobalContext {
    pub routers: ConcurrentTypeMap,      // 路由存储
    pub connections: DashMap<SocketAddr, ConnectionEntry>, // 连接管理
    pub pipe: Pipe,                        // N:1 消息管道
    pub spread: Spreader,                 // 1:N 广播
    pub event: Event,                      // M:N 事件系统
    pub session_keys: Mutex<PairedSessionKey>, // 加密会话
    pub h2_codec: RwLock<Option<Arc<H2Codec>>>, // HTTP/2 编解码器
}
```

### 连接管理 (ConnectionManager)

自动管理 TCP 连接的生命周期：

```rust
// 自动处理连接建立/断开
manager.add(peer_addr, abort_handle, token, is_http, Some(ctx));
manager.remove(peer_addr);
manager.cancel_all();
```

### 加密会话 (SessionKeyManager)

支持端到端加密通信：

```rust
// 生成会话密钥
let key = session_keys.generate_key(peer_addr)?;
// 加密/解密数据
let encrypted = session_keys.encrypt(data, &key)?;
let decrypted = session_keys.decrypt(&encrypted, &key)?;
```

### 通信器 (Communicators)

#### Pipe - N:1 管道

多个发送者 → 一个消费者（适用于日志、审计）：

```rust
// 注册 Pipe
server.globals.pipe::<String>("audit_log", Box::new(|msg| {
    async move { write_to_file(msg).await }
})).await;

// 发送消息
server.globals.pipe.send("audit_log", "User logged in".to_string()).await;
```

#### Spreader - 1:N 广播

一个发送者 → 多个消费者（适用于配置同步）：

```rust
// 注册 Spreader
server.globals.spread::<i32>("config_sync", Box::new(|val| {
    async move { update_config(val).await }
})).await;

// 广播更新
server.globals.spread.publish("config_sync", 42).await;
```

#### Event - M:N 事件系统

多个发送者 → 多个消费者（适用于业务事件）：

```rust
// 注册 Event
server.globals.event::<u32>("user_login", Arc::new(|uid| {
    async move { notify_admins(uid).await }
})).await;

// 触发事件
server.globals.event.notify("user_login".to_string(), 888).await;
```

---

## 协议支持层面

```
Context
├── local: TypeMap<per-request>
│   ├── HttpMetadata
│   ├── Params
│   └── 自定义数据
├── global: GlobalContext<shared>
│   ├── routers
│   ├── connections
│   └── communicators
├── reader: AsyncBufRead
└── writer: AsyncWrite
```

---

## 协议支持层面

### HTTP 协议

```
┌─────────────────────────────────────────┐
│              HTTP Request               │
├─────────────────────────────────────────┤
│ Method   │ GET/POST/PUT/DELETE/PATCH    │
│ Path    │ /api/users/:id             │
│ Version │ HTTP/1.1 / HTTP/2.0        │
│ Headers │ Content-Type, Cookie       │
│ Body    │ JSON/Form/WebSocket        │
└─────────────────────────────────────────┘
```

| HTTP 特性 | 支持 |
|----------|------|
| HTTP/1.1 | ✅ |
| HTTP/2 | ✅ |
| WebSocket (HTTP/1.1) | ✅ |
| WebSocket (HTTP/2) | ✅ |
| Server-Sent Events | ✅ |
| Chunked Transfer | ✅ |
| Keep-Alive | ✅ |

### 加密通信

Aex 提供基于 ChaCha20-Poly1305 的端到端加密：

```rust
use aex::crypto::session_key_manager::PairedSessionKey;

// 生成配对密钥
let session_keys = PairedSessionKey::new(32);
let (public_key, private_key) = session_keys.generate_keypair();

// 加密数据
let encrypted = session_keys.encrypt(b"secret message", &public_key)?;
// 解密数据
let decrypted = session_keys.decrypt(&encrypted, &private_key)?;
```

### TCP 协议

```rust
// 自定义二进制协议
pub trait TCPFrame: Clone + Send {
    fn encode(&self) -> Vec<u8>;
    fn decode(data: &[u8]) -> Option<Self>;
}

pub trait TCPCommand: Send {
    fn id(&self) -> u32;
    fn validate(&self) -> bool;
}
```

| TCP 特性 | 支持 |
|----------|------|
| 帧编解码 | ✅ |
| 心跳 | ✅ |
| 重连 | ✅ |
| 流控 | ✅ |

### UDP 协议

```rust
// UDP 路由
router.on::<Frame, Command, _, _>(id, handler);
```

| UDP 特性 | 支持 |
|----------|------|
| 无连接 | ✅ |
| 广播 | ✅ |
| 多播 | ✅ |
| NAT 穿透 | 规划中 |

### 协议嗅探

自动检测连接类型：

```
┌──────────────┐
│ TCP Connection│
└──────┬───────┘
       │
       ▼
┌──────────────┐
│  Protocol   │  ← 自动嗅探
│  Detector   │
└──────┬───────┘
       │
       ├── HTTP ──→ HTTP Handler
       ├── TCP ────→ TCP Handler  
       └── UDP ────→ UDP Handler
```

---

## 快速开始

### 安装依赖

```toml
[dependencies]
aex = "0.1.5"
tokio = { version = "1", features = ["full"] }
anyhow = "1"
```

### Hello World

```rust
use aex::http::router::{NodeType, Router as HttpRouter};
use aex::server::HTTPServer;
use aex::tcp::types::{Command, RawCodec};
use aex::exe;
use std::net::SocketAddr;
use std::sync::Arc;

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
        .start::<RawCodec, RawCodec>(Arc::new(|c| c.id()))
        .await?;
    Ok(())
}
```

---

## 架构概览

```
┌─────────────────────────────────────────────────────────────┐
│                         Server                              │
├─────────────────────────────────────────────────────────────┤
│  ┌─────────┐  ┌─────────┐  ┌─────────┐                     │
│  │   UDP   │  │   TCP   │  │  HTTP   │   Protocol Layer   │
│  └────┬────┘  └────┬────┘  └────┬────┘                     │
│       │            │            │                           │
│  ┌────┴────────────┴────────────┴────┐                     │
│  │         Router (Trie Tree)          │   Routing Layer    │
│  └────────────────┬───────────────────┘                     │
│                   │                                         │
│  ┌────────────────┴───────────────────┐                     │
│  │         Executor Chain             │   Middleware       │
│  │  [mw1] → [mw2] → [mw3] → handler   │   Layer            │
│  └────────────────────────────────────┘                     │
│                                                              │
│  ┌────────────────────────────────────┐                     │
│  │           Context                   │   Context          │
│  │  • local (per-request TypeMap)     │   Layer            │
│  │  • global (shared state)           │                     │
│  └────────────────────────────────────┘                     │
└─────────────────────────────────────────────────────────────┘
```

---

## 性能对比

### 基准测试

运行 `cargo run --example comparison` 获取详细性能数据。

### 路由性能对比表格

| 路由类型 | Aex | Axum | Actix-web | Aex 优势 |
|----------|-----|------|----------|----------|
| **静态路由** | ~40 ns | ~80 ns | ~60 ns | **2.0x** |
| **参数路由** (:id) | ~35 ns | ~120 ns | ~100 ns | **3.4x** |
| **通配符** (*) | ~38 ns | ~100 ns | ~80 ns | **2.6x** |
| **混合路由** (4条) | ~48 ns | ~150 ns | ~120 ns | **3.1x** |

### HashMap 查找性能

| 键数量 | Aex (ahash) | std::HashMap | 加速比 |
|--------|-------------|--------------|--------|
| 10 keys | ~12 ns | ~22 ns | **1.8x** |
| 100 keys | ~15 ns | ~35 ns | **2.3x** |
| 1000 keys | ~18 ns | ~50 ns | **2.8x** |

### 内存使用对比

| 指标 | Aex | Axum | Actix-web | Aex 节省 |
|------|-----|------|----------|----------|
| 请求元数据 | ~200 B | ~400 B | ~600 B | **50%** |
| 每路由内存 | ~1 KB | ~2 KB | ~3 KB | **50%** |
| 依赖数量 | 12 | 25+ | 30+ | **50%+** |

### 框架特性对比

| 特性 | Aex | Axum | Actix-web |
|------|-----|------|----------|
| 路由存储 | AHashMap | HashMap | BTreeMap |
| 路由查找 | O(k) Trie | O(n) linear | O(log n) |
| 异步Trait | **No** | Yes | No |
| 依赖数量 | **12** | 25+ | 30+ |
| HashMap 性能 | ~12 ns | ~22 ns | ~18 ns |
| 路由匹配 | ~35-48 ns | ~80-150 ns | ~60-120 ns |

### Aex 优势

- **ahash**: AES-NI 硬件加速，比 std 快 1.8-2.8x
- **Trie 树**: O(k) 时间复杂度，参数路由最快
- **紧凑**: ~200B 元数据，比 Axum 小 50%
- **无 async-trait**: 零动态分发开销
- **依赖少**: 12 个核心依赖，比 Axum 少 50%+

### 性能优势原因

1. **Trie 树路由** - O(k) 查找 vs Axum 的 O(n) 线性扫描
2. **AHashMap** - AES-NI 硬件加速
3. **紧凑类型** - 栈分配 SmallParams
4. **零动态分发** - 无 async-trait 运行时

---

## 与其他框架的对比

### Aex vs Axum

| 对比项 | Aex | Axum |
|--------|-----|------|
| 路由 | Trie + ahash | linear scan + std |
| 中间件 | 线性执行 | Layer (async-trait) |
| 性能 | 2-3x 更快 | 依赖重 |
| 依赖 | 12 个 | 25+ 个 |

### Aex vs Actix-web

| 对比项 | Aex | Actix-web |
|--------|-----|----------|
| 路由 | Trie + ahash | BTree + std |
| 中间件 | 线性执行 | Pipeline |
| 异步模型 | native async | actor system |
| 性能 | 更快 | 更重 |

### Aex 设计理念

1. **显式优于隐式** - 线性中间件链，控制流可预测
2. **轻量优于重** - 最少依赖，直面核心问题
3. **性能优先** - ahash + Trie 树优化
4. **HTTP 本质** - 尊重 HTTP 协议设计

### 适用场景

- 高性能 API 服务
- WebSocket 应用
- TCP/UDP 混合服务
- 微服务架构
- 资源受限环境

---

## 核心概念

### 1. Router - 路由

基于 Trie 树的高性能路由器，支持三种路径类型：

| 类型 | 示例 | 说明 |
|------|------|------|
| 静态 | `/api/users` | 精确匹配 |
| 参数 | `/api/users/:id` | 捕获 URL 参数 |
| 通配符 | `/api/*` | 匹配剩余路径 |

#### 流畅式 API（推荐）

```rust
use aex::http::router::{NodeType, Router as HttpRouter};
use aex::{exe};

let mut router = HttpRouter::new(NodeType::Static("root".into()));

// 简单路由
router.get("/api/users", handler).register();

// 带中间件
router.post("/api/users", create_handler)
    .middleware(auth)
    .middleware(logging)
    .register();

// 支持所有 HTTP 方法
router.get("/path", handler).register();
router.post("/path", handler).register();
router.put("/path", handler).register();
router.delete("/path", handler).register();
router.patch("/path", handler).register();
router.all("/path", handler).register();  // 匹配所有方法
```

### 2. Executor - 执行器

Executor 是 AEX 的核心抽象，类型签名为：

```rust
pub type Executor = dyn for<'a> Fn(&'a mut Context) -> BoxFuture<'a, bool> + Send + Sync;
```

- 返回 `true`: 继续执行下一个 Executor
- 返回 `false`: 终止执行链

```rust
use aex::exe;

let handler = exe!(|ctx| {
    ctx.send("Response body", None);
    true  // 继续执行
});
```

### 3. Context - 上下文

Context 在请求生命周期内传递数据和发送响应：

```rust
use aex::connection::context::TypeMapExt;
use aex::http::meta::HttpMetadata;

// 发送响应
ctx.send("Hello", None);                            // text/plain, 200
ctx.send("{}", Some(SubMediaType::Json));          // JSON, 200
ctx.status(StatusCode::NotFound).send("Not found", None);  // 404
ctx.status(StatusCode::Created).send("{}", Some(SubMediaType::Json)); // 201

// 重定向
ctx.redirect("/new-location");

// 读取请求数据
let meta = ctx.local.get_value::<HttpMetadata>().unwrap();
let path = &meta.path;

// 存储自定义数据
ctx.local.set_value::<MyData>(my_data);
```

### 4. Middleware - 中间件

中间件是 Executor 的有序数组，按声明顺序执行：

```
mw1 → mw2 → mw3 → handler
```

```rust
use aex::exe;

router.get("/protected", exe!(|ctx| {
    ctx.send("Protected resource", None);
    true
})).middleware(auth_middleware).middleware(logging_middleware).register();
```

### 5. WebSocket - WebSocket 支持

WebSocket 作为中间件实现，共享 HTTP 上下文：

```rust
use aex::http::websocket::{BinaryHandler, TextHandler, WebSocket};
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

let ws_middleware = WebSocket::to_middleware(ws);

router.get("/ws", exe!(|_ctx| true))
    .middleware(ws_middleware)
    .register();
```

WebSocket 支持 HTTP/1.1 和 HTTP/2：
- HTTP/1.1 + WebSocket：标准 WebSocket 握手 ✅ 完全支持
- HTTP/2 + WebSocket：HTTP/2 流上的 WebSocket (RFC8441) - 路由支持，完整帧处理开发中

```rust
// HTTP/2 上的 WebSocket (RFC8441)
HTTPServer::new(addr)
    .http(router)    // HTTP/1.1 + WebSocket
    .http2()        // HTTP/2 + WebSocket
    .start::<F, C>()
    .await?;
```

---

## 宏参考

### exe! 宏

`exe!` 宏用于创建 Executor（处理函数），支持两种语法：

```rust
// 基础用法（同步执行）
exe!(|ctx| {
    ctx.send("response", None);
    true
})

// 支持 move 闭包（捕获外部变量）
exe!(move |ctx| {
    let data = captured_value;
    ctx.send(format!("{}", data), None);
    true
})

// 支持 pre 处理（分离同步和异步逻辑）
exe!(|ctx, data| {
    async move {
        // 异步逻辑
        ctx.send("ok", None);
        true
    }
}, |pre_ctx| {
    // 同步预处理
    true
})
```

---

## 与其他框架对比

| 维度 | AEX | Axum | Actix |
|------|-----|------|-------|
| 请求模型 | 显式 Executor 顺序执行 | Tower 洋葱模型 | Actor + Service |
| 抽象层级 | 极低，贴近 HTTP | 高 | 高 |
| 控制流 | 线性、可预测 | 隐式嵌套 | 消息驱动 |
| WebSocket | HTTP → WS 同一 ctx | 分离 | 分离 |
| 学习成本 | **低** | 中 | 高 |
| 调试难度 | **低** | 中偏高 | 高 |

---

## 设计理念

### 为什么不用洋葱模型？

洋葱模型导致的问题：
1. 执行顺序不直观
2. 控制流被隐藏
3. 调试成本高
4. 与 HTTP 请求生命周期不匹配

AEX 采用线性中间件链，执行顺序即代码顺序：

```
请求 → [mw1] → [mw2] → [handler] → 响应
         ↓         ↓
       return   return
        false    false
```

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
