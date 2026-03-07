use std::{
    collections::HashSet,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    time::{SystemTime, UNIX_EPOCH},
};

use crate::connection::{protocol::Protocol, types::NetworkScope};
use serde::{Deserialize, Serialize};

//
// 节点基本信息，同时用于记录本地与远程数据
// 1. 网络信息（同时包括内网与外网）
// 2. 启动时间
// 3. 支持协议
// 4. 协议版本
// 5. 认别ID，即本地公钥，用于数字签名
//


#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Node {
    pub id: Vec<u8>, // 节点 ID，通常是公钥的哈希, 基于这个id，能够与其它节点作出有效的签名
    pub version: u32, // 协议版本
    pub started_at: u64, // 启动时间戳
    pub port: u16,   // 监听端口,
    /// 💡 支持的协议列表，例如: ["tcp", "udp", "http", "ws"]
    pub protocols: HashSet<Protocol>,
    pub ips: Vec<(NetworkScope, IpAddr)>,
}

impl Node {
    /// 基础构造：手动传入所有信息
    pub fn new(port: u16, id: Vec<u8>, version: u32, ips: Vec<(NetworkScope, IpAddr)>) -> Self {
        Self {
            id,
            version,
            port,
            started_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            ips,
            protocols: Self::default_protocols(),
        }
    }

    pub fn from_addr(addr: SocketAddr, version: Option<u32>, id: Option<Vec<u8>>) -> Self {
        let ip = addr.ip();
        let port = addr.port();

        // 1. 自动计算 NetworkScope 并生成元组 Vec
        let scope = crate::connection::node::NetworkScope::from_ip(&ip);
        let ips = vec![(scope, ip)];

        // 2. 生成默认 ID (示例：使用随机或固定长度 ID)
        // 在实际应用中，这里可能需要持久化存储或硬件指纹
        let id = id.unwrap_or(vec![0u8; 32]);

        Self::new(port, id, version.unwrap_or(1), ips)
    }

    /// 默认支持的核心协议
    pub fn default_protocols() -> HashSet<Protocol> {
        let mut set = HashSet::new();
        set.insert(Protocol::Tcp);
        set.insert(Protocol::Udp);
        set.insert(Protocol::Http);
        set.insert(Protocol::Ws);
        set
    }

    /// 允许在构造时指定特定协议
    pub fn with_protocols(mut self, protocols: HashSet<Protocol>) -> Self {
        self.protocols = protocols;
        self
    }

    /// 自动化构造：从系统环境创建完整节点信息
    pub fn from_system(port: u16, id: Vec<u8>, version: u32) -> Self {
        let mut node = Self {
            id,
            version,
            port,
            started_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            ips: Vec::new(),
            protocols: Self::default_protocols(),
        };

        // 探测本地网卡
        if let Ok(interfaces) = get_if_addrs::get_if_addrs() {
            for interface in interfaces {
                let ip = interface.ip();
                if ip.is_loopback() {
                    continue;
                }

                let scope = NetworkScope::from_ip(&ip);
                node.ips.push((scope, ip));
            }
        }
        node
    }

    pub fn get_all(&self) -> Vec<IpAddr> {
        self.ips.iter().map(|(_, addr)| *addr).collect()
    }

    /// 根据 Scope 获取地址，可选匹配特定的地址族 (v4 或 v6)
    /// @param scope: 内网或外网
    /// @param version: 传入 None 表示不限版本
    ///                传入 Some(addr) 其中 addr 是 SocketAddr 类型，
    ///                函数将自动匹配与该 addr 相同协议族的地址。
    pub fn get_ips(&self, scope: NetworkScope, version: Option<IpAddr>) -> Vec<IpAddr> {
        self.ips
            .iter()
            .filter(|(s, addr)| {
                // 1. 匹配 Scope
                if *s != scope {
                    return false;
                }

                // 2. 匹配版本 (利用 SocketAddr 自身的类型特征)
                match version {
                    Some(v) => {
                        // 只有当两者同为 v4 或同为 v6 时才通过
                        (v.is_ipv4() && addr.is_ipv4()) || (v.is_ipv6() && addr.is_ipv6())
                    }
                    None => true, // 不限版本
                }
            })
            .map(|(_, addr)| *addr)
            .collect()
    }

    pub fn get_extranet_ips(&self) -> Vec<IpAddr> {
        self.get_ips(NetworkScope::Extranet, None)
    }

    pub fn get_extranet_ips_v4(&self) -> Vec<IpAddr> {
        self.get_ips(
            NetworkScope::Extranet,
            Some(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0))),
        )
    }

    pub fn get_extranet_ips_v6(&self) -> Vec<IpAddr> {
        self.get_ips(
            NetworkScope::Extranet,
            Some(IpAddr::V6(Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 0))),
        )
    }

    pub fn get_intranet_ips(&self) -> Vec<IpAddr> {
        self.get_ips(NetworkScope::Intranet, None)
    }

    pub fn get_intranet_v4(&self) -> Vec<IpAddr> {
        self.get_ips(
            NetworkScope::Intranet,
            Some(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0))),
        )
    }

    pub fn get_intranet_v6(&self) -> Vec<IpAddr> {
        self.get_ips(
            NetworkScope::Intranet,
            Some(IpAddr::V6(Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 0))),
        )
    }

    pub fn add_observed_ip(&mut self, scope: NetworkScope, addr: IpAddr) {
        if !self.ips.contains(&(scope, addr)) {
            self.ips.push((scope, addr));
        }
    }
}
