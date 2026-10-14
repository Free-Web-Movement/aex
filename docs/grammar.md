# 文法图 / Grammar Diagram

> **Web 直线（The Web Straight Line）** 的最初文法与图像来自
> [`calidion/aex`](https://github.com/calidion/aex/blob/master/docs/grammar.md)。

**关键区分（务必分清）：**

- **Web 直线**是**针对 HTTP 请求 / 响应机制**的理论——**无环（acyclic）**：请求进、
  响应出，一条直线。**它适用于所有「请求 → 响应」型的 HTTP 协议**（HTTP/1.1、HTTP/2 等）。
- **通用服务器状态机**（aex Rust）**允许循环**：状态机内部的消息可以循环往复
  （命令触发命令、消息再生成消息……），因此它的文法是**带环**的。
- 二者的关系是「**整体 ⊇ 部分**」：通用服务器状态机是整体，**Web 直线是它内部 HTTP
  分支（`Http1` / `Http2`）上那条无环的直线**。**不是「从 Web 直线扩展」**。

对应图像（均为铁路图 / railroad diagram）：

| 图 | 说明 |
|---|---|
| `docs/Web_Straight_Line.png` | HTTP 请求 / 响应：**无环**的直线（适用于所有请求→响应型 HTTP） |
| `docs/aex_architecture.png` | 通用服务器状态机：**允许循环**，包含 Web 直线作为 HTTP 分支（`Http1` / `Http2`） |

## 如何重新生成

**本地（离线，本仓库自带渲染器）：**

```bash
python3 docs/railroad.py docs
# 重新生成 docs/Web_Straight_Line.png 与 docs/aex_architecture.png
```

**在线（Railroad Diagram Generator）：** <https://bottlecaps.de/rr/ui> ——
把下面的 EBNF 粘进去，Parse 后导出 PNG/SVG。

---

## 一、Web 直线文法（HTTP 请求 / 响应，无环）

一条 HTTP 请求沿固定阶段单向推进，**没有循环**；中间件插在阶段之间，返回 `false` 即短路。
适用于**所有请求 → 响应型 HTTP**（HTTP/1.1、HTTP/2；以及经 HTTP 升级的 WebSocket）。

``` grammar
Web_Straight_Line
   ::= ( 'Request' | http | websocket )
       ( 'routing' | method url )
       ( 'authorization' | cookie | session | http-auth )
       ( 'data parsing' | query | body | formdata | files )
       ( 'deeper access control' | ( ( 'policy' | isLogined | isOwner | isAdmin | '...' ) ( 'fallbacks' ) ) )
       ( 'data control' | pagination | filter | cache | i18n )
       ( 'error processing' | http | application | database | business )
       ( 'business logic' | ( 'command' process ) | ( 'modelling' database ) )
       ( 'output' | ( 'displayable' | html | xml ) ( 'api' | json | rest | graphql ) ( 'streaming' ) '...' )
       ( 'middleware' | 'linear executor' | 'false => short-circuit' )
       'Response'
```

**说明：** 本文明**只描述 HTTP**，且**全程无环**；它对应通用服务器状态机中的 **HTTP 分支**
（`Http1` 与 `Http2` 都适用），不涉及 TCP / UDP。

---

## 二、通用服务器状态机文法（允许循环，包含 Web 直线）

aex（Rust）是**协议无关**的**通用服务器状态机**，其本质就是 **TCP / UDP 的处理逻辑**：
一个**循环**——`接收（侦 / 命令）→ 解析 → 路由 → 中间件处理 → 命令处理 → 返回数据 →
新的循环`（文法中的 `Machine+`）。
顶层只有 `Tcp`（流）与 `Udp`（数据报）两种协议，HTTP / HTTP2 / WebSocket / SOCKS
都是 `Tcp` 的**子处理器**。

**与 HTTP 不同，状态机内部的消息可以循环**：`Machine+`、`SubProtocol*`、`Middleware*`、
`Command*` 都是可重复（带环）的结构。**Web 直线（HTTP 分支 `Http1` / `Http2`）是这条循环上的无环特例**
——收到请求、回写响应即结束，被**包含**在状态机之中。

``` grammar
Aex_Server_State_Machine
   ::= 'Server' DetectorRegistry Machine+ 'start' address

Machine
   ::= Receive Parse Route Middleware Command WriteBack   (* Machine+ 即“新的循环” ↺ *)

Receive
   ::= 'detect' | 'command'                   (* 接收：侦 / 命令 *)
Parse
   ::= 'decode'                               (* 解析：编解码为帧/命令/请求 *)
Route
   ::= ( 'method' 'url' ) | ( 'command' 'message' )  (* 路由 *)
Middleware
   ::= 'Executor'                             (* 中间件处理：线性；false => 短路 *)
Command
   ::= 'command process'                      (* 命令处理 *)
WriteBack
   ::= 'response' | 'reply'                   (* 返回数据 *)

Protocol
   ::= Tcp | Udp

Tcp
   ::= 'Tcp' RawHandler? SubProtocol* Middleware* Command*

Udp
   ::= 'Udp' DatagramHandler? Middleware* Command*

SubProtocol
   ::= Http1 | Http2 | Websocket | Socks

Http1
   ::= 'Http1' ( Router | Handler ) Websocket? Middleware* Command*
Http2
   ::= 'Http2' Handler? Middleware* Command*
Websocket
   ::= 'Websocket' Router? Middleware* Command*
Socks
   ::= 'Socks' Authorizer? Middleware* Command*

Router
   ::= HttpRouter | TcpRouter
HttpRouter
   ::= ( 'get' | 'post' | 'put' | 'delete' | 'patch' | 'options' | 'head' | 'all' ) path Handler
TcpRouter
   ::= Extractor Handler+

Shared
   ::= GlobalContext | Context | Codec | SessionKey | Communicators
```

**与 Web 直线的关系（包含，而非扩展）：**

- 通用服务器状态机 = 整体；Web 直线 = HTTP 分支（`Http1` / `Http2`）的**无环直线**。
- `Tcp` / `Udp` 及其它子协议是状态机中的**其它分支**，与 Web 直线**并列**，且**可含循环**。
- 所有分支共享同一条 `Middleware`（`Executor`）语义与「`false` 即短路」规则。

## 参考

- 上游文法：<https://github.com/calidion/aex/blob/master/docs/grammar.md>
- Web 直线理论：<https://github.com/calidion/aex#what-is-the-web-straight-line>
- 本地渲染器：`docs/railroad.py`
