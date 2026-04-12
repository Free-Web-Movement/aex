# AEX — Async-first, Executor-based Web/TCP/UDP Framework

> 一个轻量、可控、忠于 HTTP 本质的 Rust 多协议框架

[![Rust](https://img.shields.io/badge/rust-1.75%2B-blue.svg)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/license-GPL--3.0-green.svg)](LICENSE)

## 核心特性

- **直觉的 HTTP 路由** - Trie 树路由，支持静态路径、参数路径、通配符
- **显式中间件链** - 线性执行顺序，可预测的控制流（非洋葱模型）
- **原生 WebSocket 支持** - 作为中间件自然集成，共享 HTTP 上下文
- **多协议支持** - HTTP、TCP、UDP 服务器统一接口
- **TypeMap 扩展** - 灵活的请求/响应数据存储

---

## 快速开始

### 安装依赖

```toml
[dependencies]
aex = { git = "https://github.com/your-org/aex" }
tokio = { version = "1", features = ["full"] }
anyhow = "1"
```

### Hello World

```rust
use aex::http::router::{NodeType, Router as HttpRouter};
use aex::server::HTTPServer;
use aex::tcp::types::{Command, RawCodec};
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
        ctx.send("Hello, World!");
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
    ctx.send("Response body");
    true  // 继续执行
});
```

### 3. Context - 上下文

Context 在请求生命周期内传递数据和发送响应：

```rust
use aex::connection::context::TypeMapExt;
use aex::http::meta::HttpMetadata;

// 发送响应（推荐方式）
ctx.send("Hello, World!");
ctx.send(format!("User: {}", name));

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
    ctx.send("Protected resource");
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

route!(router, get!("/ws", exe!(|_ctx| true), [ws_middleware]));
```

---

## 宏参考

### exe! 宏

`exe!` 宏用于创建 Executor（处理函数），支持两种语法：

```rust
// 基础用法（同步执行）
exe!(|ctx| {
    ctx.send("response");
    true
})

// 支持 move 闭包（捕获外部变量）
exe!(move |ctx| {
    let data = captured_value;
    ctx.send(format!("{}", data));
    true
})

// 支持 pre 处理（分离同步和异步逻辑）
exe!(|ctx, data| {
    async move {
        // 异步逻辑
        ctx.send("ok");
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
