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

/// NAT 服务协议魔数：连接首字节，供 unified server 检测层识别 NAT 服务。
/// 与 HTTP/HTTP2/SOCKS 等在同一端口共存，检测层凭魔数分派。
pub const NAT_MAGIC: &[u8] = b"ZZNAT";
/// 魔数长度。
pub const NAT_MAGIC_LEN: usize = 5;

/// 帧体（body）长度前缀占位大小（u32 LE）。
pub const NAT_LEN_PREFIX_BYTES: usize = 4;

/// 帧体（body）最大允许长度。解帧时从帧头解析出的 `len` 必须在
/// `[0, NAT_MAX_FRAME_BODY]` 内，否则视为协议错误并断开——
/// 防止异常/错位的帧头触发巨大内存分配（如 512MB 中继节点分配 1.3GB 崩溃）。
pub const NAT_MAX_FRAME_BODY: usize = 16 * 1024 * 1024; // 16 MiB

/// 从线上帧头（`魔数 + 长度前缀`）解析并校验 body 长度。
///
/// 返回 `Err(NatError::Protocol)` 当长度前缀缺失或超出 [`NAT_MAX_FRAME_BODY`]，
/// 以防任意巨大 `len` 导致不受控的内存分配。
pub fn frame_len_from_header(head_buf: &[u8]) -> NatResult<usize> {
    if head_buf.len() < NAT_MAGIC_LEN + NAT_LEN_PREFIX_BYTES {
        return Err(NatError::Protocol(
            "frame header too short".to_string(),
        ));
    }
    let len = u32::from_le_bytes(
        head_buf[NAT_MAGIC_LEN..NAT_MAGIC_LEN + NAT_LEN_PREFIX_BYTES]
            .try_into()
            .unwrap_or([0u8; 4]),
    ) as usize;
    if len > NAT_MAX_FRAME_BODY {
        return Err(NatError::Protocol(format!(
            "frame body length {len} exceeds max {}",
            NAT_MAX_FRAME_BODY
        )));
    }
    Ok(len)
}

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
    /// 打洞请求：内网节点 A 请求与 B 直连打洞（src=A, dst=B）。
    PunchRequest,
    /// 打洞提示：公网节点把对端公网映射地址告知内网节点（extra=对端公网地址）。
    PunchHint,
    /// 打洞开始：公网节点通知双方同时向对方打洞。
    PunchStart,
    /// 在线对端列表：中继广播当前登记的隧道 peer（payload = 编码的 peer 列表）。
    Peers,
}

/// 穿透协议帧。
#[derive(Debug, Clone, bincode::Encode, bincode::Decode)]
pub struct NatFrame {
    pub frame_type: NatFrameType,
    /// 源节点身份（node_id / 钱包地址字符串）。
    pub src: String,
    /// 目标节点身份（Data / PunchRequest 使用）。
    pub dst: String,
    /// 公网映射地址（RegisterAck / PunchHint 携带，`ip:port` 字符串）。
    pub public_addr: String,
    /// 附加地址（PunchHint 携带对端公网映射地址，`ip:port` 字符串）。
    pub extra: String,
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
            extra: String::new(),
            payload: Vec::new(),
        }
    }

    pub fn register_ack(public_addr: &str) -> Self {
        Self {
            frame_type: NatFrameType::RegisterAck,
            src: String::new(),
            dst: String::new(),
            public_addr: public_addr.to_string(),
            extra: String::new(),
            payload: Vec::new(),
        }
    }

    pub fn keep_alive(src: &str) -> Self {
        Self {
            frame_type: NatFrameType::KeepAlive,
            src: src.to_string(),
            dst: String::new(),
            public_addr: String::new(),
            extra: String::new(),
            payload: Vec::new(),
        }
    }

    pub fn keep_alive_ack(src: &str) -> Self {
        Self {
            frame_type: NatFrameType::KeepAliveAck,
            src: src.to_string(),
            dst: String::new(),
            public_addr: String::new(),
            extra: String::new(),
            payload: Vec::new(),
        }
    }

    pub fn data(src: &str, dst: &str, payload: Vec<u8>) -> Self {
        Self {
            frame_type: NatFrameType::Data,
            src: src.to_string(),
            dst: dst.to_string(),
            public_addr: String::new(),
            extra: String::new(),
            payload,
        }
    }

    pub fn punch_request(src: &str, dst: &str) -> Self {
        Self {
            frame_type: NatFrameType::PunchRequest,
            src: src.to_string(),
            dst: dst.to_string(),
            public_addr: String::new(),
            extra: String::new(),
            payload: Vec::new(),
        }
    }

    pub fn punch_hint(src: &str, peer_public_addr: &str) -> Self {
        Self {
            frame_type: NatFrameType::PunchHint,
            src: src.to_string(),
            dst: String::new(),
            public_addr: String::new(),
            extra: peer_public_addr.to_string(),
            payload: Vec::new(),
        }
    }

    pub fn punch_start(src: &str, dst: &str) -> Self {
        Self {
            frame_type: NatFrameType::PunchStart,
            src: src.to_string(),
            dst: dst.to_string(),
            public_addr: String::new(),
            extra: String::new(),
            payload: Vec::new(),
        }
    }

    /// 在线对端列表广播（payload = bincode 编码的 `Vec<(node_id, public_addr)>`）。
    pub fn peers(src: &str, peers: &[(String, String)]) -> Self {
        let payload = bincode::encode_to_vec(peers, bincode::config::standard())
            .unwrap_or_default();
        Self {
            frame_type: NatFrameType::Peers,
            src: src.to_string(),
            dst: String::new(),
            public_addr: String::new(),
            extra: String::new(),
            payload,
        }
    }

    /// 编码为纯 body（bincode，不含魔数/长度前缀）。
    pub fn encode(&self) -> NatResult<Vec<u8>> {
        bincode::encode_to_vec(self, bincode::config::standard())
            .map_err(|e| NatError::Protocol(format!("encode failed: {e}")))
    }

    /// 编码为线上帧：`魔数(5) + 长度(4, u32 LE) + body`。
    /// 魔数置于最前，供 unified server 检测层识别 NAT 服务。
    pub fn encode_wire(&self) -> NatResult<Vec<u8>> {
        let body = self.encode()?;
        let mut out = Vec::with_capacity(NAT_MAGIC_LEN + 4 + body.len());
        out.extend_from_slice(NAT_MAGIC);
        out.extend_from_slice(&(body.len() as u32).to_le_bytes());
        out.extend_from_slice(&body);
        Ok(out)
    }

    /// 从完整线上帧解码（自动校验并剥离魔数 + 长度前缀）。
    pub fn decode(bytes: &[u8]) -> NatResult<Self> {
        let body = if bytes.len() >= NAT_MAGIC_LEN && &bytes[..NAT_MAGIC_LEN] == NAT_MAGIC {
            if bytes.len() >= NAT_MAGIC_LEN + 4 {
                let len = u32::from_le_bytes(
                    bytes[NAT_MAGIC_LEN..NAT_MAGIC_LEN + 4]
                        .try_into()
                        .unwrap_or([0u8; 4]),
                ) as usize;
                let end = (NAT_MAGIC_LEN + 4 + len).min(bytes.len());
                &bytes[NAT_MAGIC_LEN + 4..end]
            } else {
                &bytes[NAT_MAGIC_LEN..]
            }
        } else {
            bytes
        };
        bincode::decode_from_slice(body, bincode::config::standard())
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
