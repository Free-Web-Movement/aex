use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum Protocol {
    Tcp,
    Udp,
    Http,
    Ws,    // WebSocket
    Custom(String), // 扩展接口
}

impl Protocol {
    /// 转换为标准字符串标识，便于跨语言兼容
    pub fn as_str(&self) -> &str {
        match self {
            Protocol::Tcp => "tcp",
            Protocol::Udp => "udp",
            Protocol::Http => "http",
            Protocol::Ws => "ws",
            Protocol::Custom(s) => s.as_str(),
        }
    }
}

/// 允许方便地从字符串转换回枚举（例如解析配置或握手数据时）
impl From<&str> for Protocol {
    fn from(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "tcp" => Protocol::Tcp,
            "udp" => Protocol::Udp,
            "http" => Protocol::Http,
            "ws" => Protocol::Ws,
            other => Protocol::Custom(other.to_string()),
        }
    }
}