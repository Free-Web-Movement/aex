//! 内网穿透（NAT traversal）类型定义。
//!
//! 本模块处于 aex 的**传输层以下**：直接操作 `TcpStream`/`UdpSocket`，
//! 不依赖 `aex::connection` 层的连接管理/会话/命令路由。

use std::time::Duration;

/// 穿透隧道默认保活间隔（NAT 映射寿命通常 30s~5min，取保守值 15~30s）。
pub const NAT_KEEPALIVE_INTERVAL: Duration = Duration::from_secs(15);

/// 保活超时（连续未收到 Pong 视为隧道失效）。
pub const NAT_KEEPALIVE_TIMEOUT: Duration = Duration::from_secs(45);

/// 穿透隧道错误。
#[derive(Debug, thiserror::Error)]
pub enum NatError {
    #[error("tunnel already registered for node {0}")]
    AlreadyRegistered(String),

    #[error("tunnel peer {0} not found")]
    PeerNotFound(String),

    #[error("tunnel peer {0} not reachable via relay")]
    NotReachable(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("protocol error: {0}")]
    Protocol(String),

    #[error("remote closed connection")]
    RemoteClosed,
}

pub type NatResult<T> = Result<T, NatError>;

/// 穿透协议帧类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, bincode::Encode, bincode::Decode)]
pub enum NatFrameType {
    /// 注册：内网节点向公网节点登记自己（携带节点身份）。
    Register,
    /// 注册确认：公网节点返回，携带内网节点在公网侧的映射地址。
    RegisterAck,
    /// 保活：周期发送，刷新 NAT 映射 + 探活。
    KeepAlive,
    /// 保活回复。
    KeepAliveAck,
    /// 数据帧：中继转发的数据。
    Data,
    /// 寻址帧：公网节点告诉内网节点"发往目标节点 X 的数据请先发给我"。
    Route,
}

/// 穿透协议帧。
#[derive(Debug, Clone, bincode::Encode, bincode::Decode)]
pub struct NatFrame {
    pub frame_type: NatFrameType,
    /// 源节点身份（node_id / 钱包地址字符串）。
    pub src: String,
    /// 目标节点身份（Data 帧使用）。
    pub dst: String,
    /// 公网映射地址（RegisterAck 携带，`ip:port` 字符串）。
    pub public_addr: String,
    /// 载荷（Data 帧为业务数据）。
    pub payload: Vec<u8>,
}

impl NatFrame {
    pub fn register(src: &str) -> Self {
        Self {
            frame_type: NatFrameType::Register,
            src: src.to_string(),
            dst: String::new(),
            public_addr: String::new(),
            payload: Vec::new(),
        }
    }

    pub fn register_ack(public_addr: &str) -> Self {
        Self {
            frame_type: NatFrameType::RegisterAck,
            src: String::new(),
            dst: String::new(),
            public_addr: public_addr.to_string(),
            payload: Vec::new(),
        }
    }

    pub fn keep_alive(src: &str) -> Self {
        Self {
            frame_type: NatFrameType::KeepAlive,
            src: src.to_string(),
            dst: String::new(),
            public_addr: String::new(),
            payload: Vec::new(),
        }
    }

    pub fn keep_alive_ack(src: &str) -> Self {
        Self {
            frame_type: NatFrameType::KeepAliveAck,
            src: src.to_string(),
            dst: String::new(),
            public_addr: String::new(),
            payload: Vec::new(),
        }
    }

    pub fn data(src: &str, dst: &str, payload: Vec<u8>) -> Self {
        Self {
            frame_type: NatFrameType::Data,
            src: src.to_string(),
            dst: dst.to_string(),
            public_addr: String::new(),
            payload,
        }
    }

    pub fn encode(&self) -> NatResult<Vec<u8>> {
        bincode::encode_to_vec(self, bincode::config::standard())
            .map_err(|e| NatError::Protocol(format!("encode failed: {e}")))
    }

    pub fn decode(bytes: &[u8]) -> NatResult<Self> {
        bincode::decode_from_slice(bytes, bincode::config::standard())
            .map(|(frame, _)| frame)
            .map_err(|e| NatError::Protocol(format!("decode failed: {e}")))
    }
}

/// 已登记的对端（公网中继节点视角）。
#[derive(Debug, Clone)]
pub struct TunnelPeer {
    /// 对端节点身份。
    pub node_id: String,
    /// 对端在公网侧的映射地址（`ip:port`）。
    pub public_addr: String,
    /// 最近一次活跃时间（UNIX 秒）。
    pub last_seen: u64,
}

impl TunnelPeer {
    pub fn new(node_id: String, public_addr: String) -> Self {
        Self {
            node_id,
            public_addr,
            last_seen: now_secs(),
        }
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nat_frame_roundtrip() {
        let frame = NatFrame::data("node_a", "node_b", b"hello".to_vec());
        let bytes = frame.encode().unwrap();
        let decoded = NatFrame::decode(&bytes).unwrap();
        assert_eq!(decoded.frame_type, NatFrameType::Data);
        assert_eq!(decoded.src, "node_a");
        assert_eq!(decoded.dst, "node_b");
        assert_eq!(decoded.payload, b"hello".to_vec());
    }

    #[test]
    fn nat_frame_register_ack() {
        let frame = NatFrame::register_ack("69.171.73.252:20260");
        let bytes = frame.encode().unwrap();
        let decoded = NatFrame::decode(&bytes).unwrap();
        assert_eq!(decoded.frame_type, NatFrameType::RegisterAck);
        assert_eq!(decoded.public_addr, "69.171.73.252:20260");
    }

    #[test]
    fn tunnel_peer_tracks_seen() {
        let peer = TunnelPeer::new("node_a".into(), "1.2.3.4:5000".into());
        assert_eq!(peer.node_id, "node_a");
        assert_eq!(peer.public_addr, "1.2.3.4:5000");
        assert!(peer.last_seen > 0);
    }
}
