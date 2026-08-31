//! NAT 服务：在 unified server 检测层注册，与 HTTP/HTTP2/SOCKS/P2P 共用端口。
//!
//! 注册与检测相互协作：
//! - [`NatDetector`]：注册进 DetectorRegistry，检测连接首字节是否为 NAT 魔数
//!   （[`NAT_MAGIC`]），识别本连接归属 NAT 服务。
//! - [`nat_tcp_handler`]：作为 `custom_handler("nat")` 注册，处理被检测层
//!   认领的 NAT 连接（中继登记/转发/打洞信令）。
//! - [`UnifiedServerExt::enable_nat`]：一键注册两者。

use std::sync::Arc;

use crate::connection::context::Context;
use crate::unified::detect::{DetectionState, ProtocolDetector, Verdict};
use crate::unified::{DetectorMode, TCPHandler};

use super::server::NatRelayService;
use super::types::{NAT_MAGIC, NAT_MAGIC_LEN};

/// NAT 服务协议标签（unified 检测层 claim 后按此查找 handler）。
pub const NAT_PROTOCOL: &str = "nat";

/// 检测层识别 NAT 服务：连接首字节为 NAT 魔数即认领。
pub struct NatDetector;

impl ProtocolDetector for NatDetector {
    fn name(&self) -> &str {
        "nat-detector"
    }

    fn protocol(&self) -> &str {
        NAT_PROTOCOL
    }

    fn max_need(&self) -> Option<usize> {
        Some(NAT_MAGIC_LEN)
    }

    /// NAT 服务是状态型转发器：命中即终止检测，直接交给 nat handler。
    fn mode(&self) -> DetectorMode {
        DetectorMode::Forward
    }

    fn detect(&self, buf: &[u8], _state: &mut DetectionState) -> Verdict {
        if buf.is_empty() {
            return Verdict::NeedMore(NAT_MAGIC_LEN);
        }
        if buf.len() < NAT_MAGIC_LEN {
            return Verdict::NeedMore(NAT_MAGIC_LEN - buf.len());
        }
        if &buf[..NAT_MAGIC_LEN] == NAT_MAGIC {
            Verdict::Match
        } else {
            Verdict::Pass
        }
    }
}

/// 处理被检测层认领的 NAT 连接（中继服务入口）。
pub fn nat_tcp_handler(service: NatRelayService) -> TCPHandler {
    Arc::new(move |ctx: Context| {
        let service = service.clone();
        tokio::spawn(async move {
            let peer_addr = ctx.addr;
            let mut ctx = ctx;
            let reader = match ctx.reader.take() {
                Some(r) => r,
                None => {
                    tracing::warn!("[NAT] no reader in context for {}", peer_addr);
                    return;
                }
            };
            let writer = match ctx.writer.take() {
                Some(w) => w,
                None => {
                    tracing::warn!("[NAT] no writer in context for {}", peer_addr);
                    return;
                }
            };
            if let Err(e) = service.handle_split(reader, writer, peer_addr).await {
                tracing::debug!("[NAT] connection {} ended: {:?}", peer_addr, e);
            }
        })
    })
}

/// 在 unified server 上注册 NAT 服务（检测器 + handler）。
pub trait UnifiedServerExt {
    fn enable_nat(self, service: NatRelayService) -> Self;
}

impl UnifiedServerExt for crate::unified::UnifiedServer {
    fn enable_nat(mut self, service: NatRelayService) -> Self {
        if let Err(e) = self.registry.register(Arc::new(NatDetector)) {
            tracing::warn!("[Unified] nat detector registration failed: {}", e);
        }
        self.custom_handlers
            .entry(NAT_PROTOCOL.to_string())
            .or_insert_with(|| nat_tcp_handler(service));
        self
    }
}
