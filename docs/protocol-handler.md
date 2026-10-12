# 协议处理器（Protocol Handler）方案（终版）

> 状态：**已实现**。
> 统一 server 完全协议无关，全部具体协议收敛到 `src/protocols/`。
> `cargo check --workspace --all-targets` 零错误；aex 全量 `cargo test` 通过
> （含 `--features proxy`）。

## 目标（Objective）

aex 的 server 收敛为**一个协议无关的核心**，连接分派只有一句话：
`for p in protocols { if p.is_mine(claim) { p.run(ctx); break } }`。
**`unified` 目录里不出现任何 http1/http2/websocket/tcp/udp 的名字**，也不会
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

数据报循环（`start`/`start_udp`）与流式分派共用同一个 `Context` 与同一个 `run`。

命令数据用**泛型占位** `C`，默认 `C = Bytes`；`Server` 本身**非泛型**，内部固定
用通用实例化，业务命令类型后期由上层通过适配层特化。

## 架构：unified 与 protocols 的分工

```
aex/src
├── unified/                    协议无关核心（不知道任何具体协议名）
│   ├── mod.rs                  Protocol trait、Transport、Server（分派=is_mine 循环）
│   ├── detect.rs               ProtocolDetector / DetectorRegistry / DetectionState /
│   │                           Claim / Verdict / run_pipeline（纯登记 + 流水线，零内置）
│   └── handler/
│       └── mod.rs              ProtocolHandler<C=Bytes>（递归处理器 + 通用实现）
└── protocols/                  具体协议（project-agnostic，统一用同一套 API）
    ├── classify.rs             ProtocolKind 后缀前缀分类（兼容旧调用方）
    ├── http1.rs                Http11Detector + Http1Config + protocol()
    ├── http2.rs                Http2Detector + Http2Config + protocol()
    ├── websocket.rs            WebsocketConfig + protocol()（http 的子处理器）
    ├── tcp.rs                  TcpConfig + protocol()（兜底）+ from_handler()
    ├── udp.rs                  UdpConfig + protocol()（Datagram）
    └── #[cfg(feature="proxy")]
        ├── socks.rs            protocol(authorizer)
        └── proxy.rs            http_proxy(cfg, authorizer)
```

- **`unified` 不依赖 `protocols`**：只有反向依赖（`protocols` 里的每个协议用
  unified 的积木组装自己）。分层与“aex 是项目无关底层框架”一致——具体协议仍是
  aex 自带的通用能力，但与核心解耦，新增/裁剪协议都不碰核心。
- **探测与分派**：检测流水线产出 `Claim`（命中哪个 detector、声称什么协议）。
  `Protocol::is_mine` 的**默认实现纯粹用协议自身的 name 与 detector 表述**，
  无需每个协议重写。Server 不解释 claim，只把它交给各协议的 `is_mine` 自述。

## 统一接口：`unified::Protocol`

```rust
pub trait Protocol: Send + Sync {
    fn name(&self) -> &str;                                    // 协议标签
    fn detector(&self) -> Option<Arc<dyn ProtocolDetector>>;   // 默认 None
    fn middleware(&self) -> &[Arc<Executor>];                  // 默认 &[]
    fn sub_handlers(&self) -> &[Arc<ProtocolHandler<Bytes>>];  // 默认 &[]
    fn commands(&self) -> &CommandRegistry<Bytes>;             // 必选
    fn transport(&self) -> Transport;                          // 默认 Stream
    fn is_default(&self) -> bool;                              // 默认 false（兜底）
    fn buffered_writer(&self) -> bool;                         // 默认 false
    // 默认实现：default → true；否则按自身 detector.name 或 name 匹配 claim。
    fn is_mine(&self, claim: Option<&Claim>) -> bool;
    fn run<'a>(&'a self, ctx: &'a mut Context) -> BoxFuture<'a, Outcome>;
}
```

`ProtocolHandler<C=Bytes>` 是**参照实现**（`impl Protocol for ProtocolHandler<Bytes>`），
所以任何具体协议 = 一个 `ProtocolHandler`，天然实现 trait，无需再手写 `run`/`is_mine`。

## 通用处理器：`unified::handler::ProtocolHandler`

```rust
pub struct ProtocolHandler<C = Bytes> {
    name: String,
    detector: Option<Arc<dyn ProtocolDetector>>,
    middleware: Vec<Arc<Executor>>,          // 与路由中间件同款，禁止 Next
    sub_handlers: Vec<Arc<ProtocolHandler<C>>>,
    commands: CommandRegistry<C>,
    handler: Arc<dyn HandlerFn<C>>,
    buf_writer: bool,   // with_buffered_writer：http1=true（保留 BufWriter 行为）
    default: bool,      // with_default：兜底（tcp）
    datagram: bool,     // with_datagram：UDP
}
```

`ProtocolHandler::run` 三步（子处理器递归走同一例程）：

1. 在 `ctx.local` 的 `HandlerState` 记录 `enter(name)`；
2. 顺序跑 `middleware`，任一返回 `false` → `Outcome::Done`；
3. 跑 `handler_fn`；得 `Outcome::Handoff(name)` → 查 `sub_handlers` 递归 `run`。

概念保留自首版方案：**state record**（`HandlerState`，随连接下传，含
`path`/`started_at`/`bytes_in`/`bytes_out`/`commands`/`closed`/`scratch`）、
**Outcome**（`Done`/`Unhandled`/`Handoff(String)`）、**CommandKey/CommandRegistry**
（名称或数值 id）、**HandlerFn**（闭包即 handler，可经传入的 handler 触达自身
命令注册表）。新增 `take_context(ctx)`：恢复一个拥有全部字段（reader/writer/local）
的 `Context`，供持有型 handler（raw TCP/UDP）用。

## Server：协议无关分派

```rust
Server::new(globals)                       // 只传 GlobalContext
    .push(http1::protocol(cfg))            // 单一口，挂多少协议都行
    .push(tcp::protocol(TcpConfig{..}))
    .listen(addr).await?;                  // 地址在 listen 时给出
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
- 保留：`at(addr, globals)`（旧构造器短名）、`UnifiedServer = Server` 兼容别名、
  `detector`/`detector_at`/`with_registry`/`detection`、`start`/`start_tcp`/
  `start_udp`/`listen`、`Clone`。
- **故意移除**：`http_router`/`http_handler`/`tcp_handler`/`udp_handler`/
  `custom_handlers`/`enable_http2` 等旧字段与 `enable_*` 链式方法，以及
  硬编码 `match claim.protocol` 的 `handle_http11`/`handle_http2`/`dispatch_tcp`/
  `handle_tcp` 私有方法——统一由 `protocols/.push()` 等价表达。

## 具体协议（`protocols/`，全部 project-agnostic）

| 协议 | 产物 | 要点 |
|---|---|---|
| http1 | `Http1Config{router, handler}` + `protocol(cfg)` | 自带 `Http11Detector`；绝对形式/CONNECT 反代（proxy feature）；**ws 升级 → `Handoff("websocket")`**；`with_buffered_writer(true)` |
| http2 | `Http2Config{handler}` + `protocol(cfg)` | 自带 `Http2Detector`；用 `Reunited` 等价承接握手 I/O |
| websocket | `WebsocketConfig{router}` + `protocol(cfg)` | http1 的递归子处理器，复刻 `is_ws → router.on_request → send_response/send_failure` |
| tcp | `TcpConfig{handler}` + `protocol(cfg)`；`from_handler(name, handler)` | `protocol` 是 default 兜底；`from_handler` 生成带名自定义协议 |
| udp | `UdpConfig{handler}` + `protocol(cfg)` | `with_datagram(true)`，走 Datagram 槽位 |
| socks（proxy） | `protocol(authorizer)` | `from_handler("socks", …)` + `with_detector(SocksDetector)` |
| proxy（proxy） | `http_proxy(cfg, authorizer)` | 自由函数，无旧 ProxyExt trait |
| classify | `ProtocolKind::{Http11,Http2,TCP,UDP,Unknown}` | 前缀快速分类，兼容旧调用方；新代码优先走注册表流水线 |

所有 `*Ext` trait（Http1Ext/Http2Ext/TcpExt/UdpExt/ProxyExt）**已删除**。旧的
`handler/{http1,websocket,http2,tcp,udp}.rs` 逐步演进为 `protocols/` 下同名文件
（早期版本先把 `src/unified/handler/` 作为协议宿，后按用户要求**协议全部移入
`src/protocols/`**，`unified/handler/` 只剩 `ProtocolHandler` 机制本身）。

### 挂载示例

```rust
use std::sync::Arc;
use aex::protocols::{http1, http2, tcp, udp};
use aex::unified::Server;

let mut cfg = http1::Http1Config::new();
cfg.router = Some(Arc::new(router));

Server::new(globals)
    .push(http1::protocol(cfg))
    .push(http2::protocol(http2::Http2Config::default()))
    .push(tcp::protocol(tcp::TcpConfig::default()))   // 兜底，自动排最后
    .push(udp::protocol(udp::UdpConfig::default()))   // 进 Datagram 槽位
    .listen(addr).await?;
```

有鉴权代理：

```rust
// 共享同一 authorizer；http 名去重，站点 handler 并入 proxy 配置
.push(aex::protocols::proxy::http_proxy(
    http1::Http1Config { handler: Some(website), ..Default::default() },
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