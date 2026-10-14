# 协议处理器（Protocol Handler）方案（终版）

> 状态：**已实现（struct 形态）**。
> 统一 server 完全协议无关，全部具体协议收敛到 `src/protocols/`。
> **协议差异由「每协议一个 struct」表达，不再有 `*Config` 配置层**；server 只
> 接收一个整体 `Arc<dyn Protocol>`。
> `cargo check --workspace --all-targets` 零错误；aex 全量 `cargo test` 通过
> （含 `--features proxy`）。

## 目标（Objective）

aex 的 server 收敛为**一个协议无关的核心**，连接分派只有一句话：
`for p in protocols { if p.is_mine(claim) { p.run(ctx); break } }`。
**核心代码（`server.rs` / `protocol.rs` / `detect.rs` / `handler.rs`）里不出现任何 http1/http2/websocket/tcp/udp 的名字**，也不会
在 `resolve`/`is_mine` 之外存在任何协议判断分支。每个协议是一组相同风格的
积木：

1. **探测器（detector）** — 如何认领流式连接（复用 `ProtocolDetector`）。
2. **处理器（handler）**，其下再含：
   - **中间件（middleware）** — 与路由 `Executor` 完全一致的
     `Fn(&mut Context) -> BoxFuture<bool>` 链；返回 `false` 即短路。
   - **递归子处理器（sub-handlers）** — 处理器可把连接移交给具名子处理器
     （HTTP → WebSocket），子处理器本身又是一个完整处理器。
   - **协议命令处理器（command handlers）** — 按名称/数值 id 分派已解码命令。
   - （HTTP 等）**路由器（router）** — 挂在 handler 里的请求路由。
3. **传输（transport）** — `Stream`（走检测 + `is_mine`）或 `Datagram`（UDP，
   每个数据报直接 `run`）。

数据报循环（由 `start` 在后台为 `Udp` 协议启动）与流式分派共用同一个 `Context` 与同一个 `run`。

命令数据用**泛型占位** `C`，默认 `C = Bytes`；`Server` 本身**非泛型**，内部固定
用通用实例化，业务命令类型后期由上层通过适配层特化。

## 架构：核心（server/protocol/detect/handler）与 protocols 的分工

```
aex/src
├── server.rs                   统一服务器 Server（分派 = is_mine 循环）
├── protocol.rs                 Protocol trait、Transport
├── detect.rs                   ProtocolDetector / DetectorRegistry / DetectionState /
│                               Claim / Verdict / run_pipeline（纯登记 + 流水线，零内置）
├── handler.rs                  ProtocolHandler<C=Bytes>（递归处理器 + 通用实现）
└── protocols/                  具体协议（project-agnostic，统一用同一套 API）
    ├── classify.rs             ProtocolKind 后缀前缀分类（兼容旧调用方）
    ├── http1.rs                Http11Detector + Http1（struct；内含 websocket 子协议）
    ├── http2.rs                Http2Detector + Http2（struct）
    ├── websocket.rs            Websocket（struct，http 的子处理器）
    ├── tcp.rs                  Tcp（struct，兜底）+ from_handler()（自定义闭包协议）
    ├── udp.rs                  Udp（struct，Datagram）
    └── #[cfg(feature="proxy")]
        ├── socks.rs            Socks（struct）+ protocol(authorizer) 便捷
        └── proxy.rs            http_proxy(router, handler, authorizer)
```

- **核心不依赖 `protocols`**：只有反向依赖（`protocols` 里的每个协议用
  unified 的积木组装自己）。分层与“aex 是项目无关底层框架”一致——具体协议仍是
  aex 自带的通用能力，但与核心解耦，新增/裁剪协议都不碰核心。
- **探测与分派**：检测流水线产出 `Claim`（命中哪个 detector、声称什么协议）。
  `Protocol::is_mine` 的**默认实现纯粹用协议自身的 name 与 detector 表述**，
  无需每个协议重写。Server 不解释 claim，只把它交给各协议的 `is_mine` 自述。

## 统一接口：`protocol::Protocol`

```rust
pub trait Protocol: Send + Sync {
    fn name(&self) -> &str;                                    // 必选
    fn detector(&self) -> Option<Arc<dyn ProtocolDetector>>;   // 默认 None
    fn router(&self) -> Option<Arc<HttpRouter>>;               // 默认 None
    fn middleware(&self) -> &[Arc<Executor>];                  // 默认 &[]
    fn sub_handlers(&self) -> &[Arc<dyn Protocol>];            // 默认 &[]
    fn commands(&self) -> &CommandRegistry<Bytes>;             // 必选
    fn transport(&self) -> Transport;                          // 默认 Stream
    fn is_default(&self) -> bool;                              // 默认 false（兜底）
    fn buffered_writer(&self) -> bool;                         // 默认 false
    // 默认实现：default → true；否则按自身 detector.name 或 name 匹配 claim。
    fn is_mine(&self, claim: Option<&Claim>) -> bool;
    // 唯一需要具体结构实现的方法：如何服务一条已被认领的连接/数据报。
    fn process<'a>(&'a self, ctx: &'a mut Context) -> BoxFuture<'a, Outcome>;
    // 统一机械：state record → middleware 链 → process → Handoff 查子协议递归。
    fn run<'a>(&'a self, ctx: &'a mut Context) -> BoxFuture<'a, Outcome> { … }
}
```

**trait 提供统一方法与机械**（路由、命令、中间件、子处理器、`run`），
**结构提供协议特有数据与特有方法**（`Http1::new(router, handler)`、`Http2::new(handler)`、
`Tcp::new(handler)`、各 struct 自己的 `pub async fn handle(&self, ctx)`）——
协议差异由结构表达，而非 `*Config`。具体结构只实现 `process`（通常是
`Box::pin(self.handle(ctx))`），其余全部走 trait 默认。

## 通用处理器：`handler::ProtocolHandler`

`ProtocolHandler<C=Bytes>` 现在**只服务于自定义闭包协议**（`tcp::from_handler`、
SOCKS 的便捷构造）。内置协议都是各自的结构体（`Http1`/`Http2`/`Websocket`/`Tcp`/
`Udp`/`Socks`/`Nat`），它们把 `process` 指向自己的 `handle`，共用 trait 默认的
`run` 机械。`ProtocolHandler` 仍是 `impl Protocol for ProtocolHandler<Bytes>` 的
参照实现。

```rust
pub struct ProtocolHandler<C = Bytes> {
    name: String,
    detector: Option<Arc<dyn ProtocolDetector>>,
    router: Option<Arc<HttpRouter>>,
    middleware: Vec<Arc<Executor>>,          // 与路由中间件同款，禁止 Next
    sub_handlers: Vec<Arc<dyn Protocol>>,
    commands: CommandRegistry<C>,
    handler: Arc<dyn HandlerFn<C>>,
    buf_writer: bool,   // with_buffered_writer：http1=true（保留 BufWriter 行为）
    default: bool,      // with_default：兜底（tcp）
    datagram: bool,     // with_datagram：UDP
}
```

`Protocol::run`（trait 默认，所有协议共用，子处理器递归走同一例程）：

1. 在 `ctx.local` 的 `HandlerState` 记录 `enter(name)`；
2. 顺序跑 `middleware`，任一返回 `false` → `Outcome::Done`；
3. 跑 `process(ctx)`；得 `Outcome::Handoff(name)` → 查 `sub_handlers` 递归 `run`。

概念保留自首版方案：**state record**（`HandlerState`，随连接下传，含
`path`/`started_at`/`bytes_in`/`bytes_out`/`commands`/`closed`/`scratch`）、
**Outcome**（`Done`/`Unhandled`/`Handoff(String)`）、**CommandKey/CommandRegistry**
（名称或数值 id）、**HandlerFn**（闭包即 handler，可经传入的 handler 触达自身
命令注册表）。新增 `take_context(ctx)`：恢复一个拥有全部字段（reader/writer/local）
的 `Context`，供持有型 handler（raw TCP/UDP）用。

## Server：协议无关分派

```rust
Server::new(globals)                       // 只传 GlobalContext
    .push(http!(router))             // 顶层 TCP，HTTP 作为子处理器
    .start(addr).await?;
```

- **`push` = 唯一挂载点**（`mount` 原地版）。挂载时：
  带 `detector` 的协议自动注册其探测器进 `registry`（重复名只告警）；
  `Datagram` 协议进 `datagram` 槽位；`Stream` 按名去重（同名后挂替换前者），
  **default 协议永远排最后**。
- **`handle_tcp_connection`**：检测阶段读首字节 → `run_pipeline` →
  `state.claim()`；随后**纯循环** `for p in protocols { p.is_mine(claim) }`，
  首个认领者 `run`。无认领 → 记录 warning。`detection(false)` 或空注册表时
  完全不 peek，流直接交兜底协议。
- writer 是否 `BufWriter` 由 `protocol.buffered_writer()` 决定，Server 不问协议名。
- 保留：`detector`/`detector_at`/`with_registry`/`detection`、`start(addr)`、`Clone`。
- **已删除**：`start_tcp`/`start_udp`（TCP/UDP 的收发循环由挂载的 `Tcp`/`Udp` 协议承载，
  `start` 会为 `Udp` 在后台起循环）；`at(addr, globals)` / `solo(addr)` / `listen(addr)`
  与 `UnifiedServer` 兼容别名。
- **故意移除**：`http_router`/`http_handler`/`tcp_handler`/`udp_handler`/
  `custom_handlers`/`enable_http2` 等旧字段与 `enable_*` 链式方法，以及
  硬编码 `match claim.protocol` 的 `handle_http11`/`handle_http2`/`dispatch_tcp`/
  `handle_tcp` 私有方法——统一由 `protocols/.push()` 等价表达。

## 具体协议（`protocols/`，全部 project-agnostic）

| 协议 | 产物（struct） | 要点 |
|---|---|---|
| http1 | `Http1::new(router, handler)` | 自带 `Http11Detector`；`router()`/`sub_handlers()`；绝对形式/CONNECT 反代（proxy feature，`with_proxy(authorizer)`）；**ws 升级 → `Handoff("websocket")`**；`buffered_writer()=true` |
| http2 | `Http2::new(handler)` | 自带 `Http2Detector`；用 `Reunited` 等价承接握手 I/O |
| websocket | `Websocket::new(router)` | http1 的递归子处理器，复刻 `is_ws → router.on_request → send_response/send_failure` |
| tcp | `Tcp::new(handler)`（兜底）；`from_handler(name, handler)` | `Tcp` 是 default 兜底；`from_handler` 生成带名自定义闭包协议 |
| udp | `Udp::new(handler)` | `transport()=Datagram`，走 Datagram 槽位 |
| socks（proxy） | `Socks::new(authorizer)`；便捷 `socks::protocol(authorizer)` | 自带 `SocksDetector`，持 `socks_tcp_handler` |
| proxy（proxy） | `http_proxy(router, handler, authorizer)` | = `Http1::new(..).with_proxy(authorizer).shared()` |
| nat | `Nat::new(service)`；便捷 `UnifiedServerExt::enable_nat(service)` | 自带 `NatDetector`，持共享 `NatRelayService` |
| classify | `ProtocolKind::{Http11,Http2,TCP,UDP,Unknown}` | 前缀快速分类，兼容旧调用方；新代码优先走注册表流水线 |

所有 `*Ext` trait（Http1Ext/Http2Ext/TcpExt/UdpExt/ProxyExt）**已删除**；`*Config`
（`Http1Config`/`Http2Config`/`TcpConfig`/`UdpConfig`/`WebsocketConfig`）与
`protocol(cfg)` 构造器**已删除**，改由各协议的 struct 承载。旧的
`handler/{http1,websocket,http2,tcp,udp}.rs` 已演进为 `protocols/` 下同名文件
（`handler.rs` 只剩 `ProtocolHandler` 机制本身）。

### 挂载示例

```rust
use std::sync::Arc;
use aex::{http, http2, tcp, udp};
use aex::server::Server;

Server::new(globals)
    .push(http!(router))  // 顶层 TCP 兜底，HTTP/1.1 + HTTP/2 作为子处理器
    .push(udp!())                          // 兜底 UDP 数据报
    .start(addr).await?;
```

> 协议宏（`http!`/`http1!`/`http2!`/`ws!`/`tcp!`/`tcp_raw!`/`udp!`，见
> `src/protocols/macros.rs`）等价于手写 `Http1::new(Some(Arc::new(router)), None).shared()`
> 等，只是把 `Struct::new(..).shared()` 缩短，便于组合。

有鉴权代理：

```rust
// 共享同一 authorizer；http 名去重，站点 handler 与 router 均可传入
.push(aex::protocols::proxy::http_proxy(
    None,
    Some(website),
    Some(authorizer.clone()),
))
.push(aex::protocols::socks::protocol(Some(authorizer)));
```

## 行为保证

- 检测字节读取、`initial_data` 拼接、`BufWriter`/裸 writer 差异、http2 握手 I/O、
  proxy 分支、ws 分支语义与迁移前一致。
- `DetectionState` 写入位置不变（http1 不写、tcp/custom 写）。
- `ProtocolKind::detect` 仅作兼容入口；挂载即自注册探测器，旧显式
  `.detector(Http11Detector)` 重复注册仅告警、不致命。

## 测试现状

- 核心：`tests/protocol_handler_test.rs`、`tests/unified_mod_gap_test.rs`、
  `tests/unified_detect_pipeline_test.rs`、`tests/unified_server_test.rs`、
  `tests/unified_listener_all_protocols_test.rs`、`tests/nat_unified_integration_test.rs`
  —— 全部迁移到 `push()` 风格并全绿。
- proxy：`tests/proxy_builtin_test.rs`、`tests/proxy_detectors_test.rs`
  （`cargo test -p aex --features proxy`）全绿。
- detective 能力：`ProtocolKind::detect` 边界与 HTTP 方法集由
  `unified_mod_gap_test` 覆盖；流水线前后置/未知兜底由 `unified_detect_pipeline_test`
  覆盖（原内嵌在 `detect.rs` 的 `#[cfg(test)]` 已删除，符合“测试一律放 tests/”）。

## 未纳入（后续）

- http3 / quic 迁移。
- 业务命令类型特化（`ProtocolHandler<C>` 业务侧实例化 + 适配层）。