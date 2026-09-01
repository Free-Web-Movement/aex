# AGENTS.md — aex 编码规则

本文件定义 AI 编码助手在 aex 仓库（异步传输层框架）中必须遵守的规则。

---

## 项目定位（最重要，不得违背）

**aex 是一个项目无关（project-agnostic）的底层框架。**

> 注：aex 的代码托管在某个 GitHub 组织账号下，但这只是**托管位置**，不影响
> 其项目无关的**实质定位**。aex 本身不依赖、不耦合、不引用任何上层应用的业务
> 类型；上层应用单向依赖 aex，反向禁止。

- aex 扩展新能力（如 NAT traversal）的动机是**为上层应用提供通用的底层能力**，
  **不是**为某个具体项目定制功能。
- 因此：能力要**通用、可配置、可复用**，不写死任何具体业务语义、地址、端口、
  身份格式或业务字段；具体业务适配放在上层应用完成。
- 涉及 aex 的文档/注释/命名**不得出现任何具体业务项目的信息**（如项目名、
  业务地址前缀、业务端口约定等）。
- 公网 IP、端口等运行期参数**不写死在代码里**，一律由调用方/CLI 传入。

## 分层与服务模型（核心）

1. **aex 是传输层框架**：NAT traversal 等穿透能力属于**传输层以下**的代码，
   作为 aex 的内部底层模块实现（直接操作 `TcpStream`/`UdpSocket`），**不依赖**
   `aex::connection` 层的 manager/context/router。
2. **udp/tcp/http/ws/proxy/nat 都是服务**：通过**统一的服务注册机制**实现注册
   —— unified server 的 `DetectorRegistry`（检测层）+ `custom_handler`（处理层）协作：
   - **注册**：`enable_xxx()` 在 `UnifiedServer` 上注册 detector + handler。
   - **检测**：`ProtocolDetector` 识别连接首字节（魔数/greeting/请求行），
     `Verdict::Match` 认领连接。
   - **分派**：检测命中后按 `protocol()` 标签查找 `custom_handler` 处理。
   - **单端口**：所有服务共用同一个监听端口，靠检测层区分。
3. **proxy 是服务注册的参考范式**：`enable_socks_proxy`/`enable_http_proxy` +
   `SocksDetector` + `socks_tcp_handler` 是标准的「注册 + 检测 + handler」结构。
   nat 等服务照此实现。

## 测试规则（不得违背）

1. **测试不与源码混放**：**所有**测试代码一律放 `tests/` 目录（`tests/<module>_test.rs`
   或 `tests/<module>/`），**禁止在 `src/` 下写任何 `#[cfg(test)]` / `mod tests` / `#[test]`**。
   `src/` 只包含生产逻辑，不得出现任何测试代码。存量 `src/` 内已有的 `#[cfg(test)]`
   一律迁移到 `tests/` 对应文件，不得新增。
2. **独立运行**：`cargo test --test <module>_test` 可单独跑该模块测试。
3. **引用方式**：集成测试通过 `use aex::<module>::...` 引用公开 API，不访问私有项。
4. **覆盖**：每个公开函数、每个 `Err` 分支、边界条件（空输入/非法输入/并发）都要有测试。
5. **不依赖外部服务**：用本地 `127.0.0.1`、临时文件、mock，不依赖公网/DB。
6. **命名**：`snake describe_what` 格式，如 `nat_detector_matches_magic`。

## NAT 模块（aex::nat）约定

- 位于 `aex/src/nat/`，传输层以下，直接操作 socket。
- `server::NatRelayService`：共享登记表 + 连接处理（unified handler 与独立
  listener 复用同一 `handle_split`，接受 `BoxReader`/`BoxWriter` trait object）。
- `service::NatDetector` + `nat_tcp_handler` + `UnifiedServerExt::enable_nat`：
  作为 unified server 服务注册，与 HTTP/HTTP2/SOCKS 共用端口。
- 帧协议：`NatFrame` 编码带魔数前缀（`NAT_MAGIC`），供检测层识别。
- **通用性**：NAT 是 aex 提供的通用穿透能力，不绑定任何具体业务；节点身份、
  端口、映射地址等均为运行时参数，由调用方提供，不写死在代码里。
- **公网 IP 不写死在代码里**：一律由调用方/CLI 传入（公网 IP 是临时的）。

## 参考

- 测试范式：`aex/tests/` 下既有测试（`tcp_listener_test.rs`、`proxy_*_test.rs`、
  `nat_tunnel_test.rs`、`nat_punch_test.rs`、`nat_service_test.rs`）。
