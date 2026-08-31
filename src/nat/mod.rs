//! 内网穿透（NAT traversal）底层模块。
//!
//! 本模块处于 aex 的**传输层以下**：直接操作 `TcpStream`/`UdpSocket`，
//! **不依赖** `aex::connection` 层的连接管理/会话/命令路由。
//!
//! 提供：
//! - [`server::NatRelayServer`]：公网中继节点，登记内网节点并转发数据。
//! - [`client::NatTunnelClient`]：内网节点隧道客户端，主动出站连公网并注册。
//! - [`types`]：穿透协议帧与登记表类型。
//!
//! 降级路径（ICE 模型）：私网直连 → 公网打洞 → 中继。当前先实现最可靠
//! 的**中继**路径；打洞（[`super::nat`] 后续 `punch` 子模块）留待扩展。

pub mod client;
pub mod server;
pub mod types;

pub use client::{NatTunnelClient, TunnelData, TunnelState};
pub use server::NatRelayServer;
pub use types::{NatError, NatFrame, NatFrameType, NatResult, TunnelPeer};
