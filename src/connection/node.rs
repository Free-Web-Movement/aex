use std::{
    collections::HashSet,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    time::{SystemTime, UNIX_EPOCH},
};

use crate::connection::{protocol::Protocol, scope::NetworkScope};
use bincode::{Decode, Encode};
use serde::{Deserialize, Serialize};

//
// 节点基本信息，同时用于记录本地与远程数据
// 1. 网络信息（同时包括内网与外网）
// 2. 启动时间
// 3. 支持协议
// 4. 协议版本
// 5. 认别ID，即本地公钥，用于数字签名
//

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Encode, Decode)]
pub struct Node {
    pub id: Vec<u8>, // 节点 ID，通常是公钥的哈希, 基于这个id，能够与其它节点作出有效的签名
    pub version: u8, // 协议版本
    pub started_at: u64, // 启动时间戳
    pub port: u16,   // 监听端口,
    /// 💡 支持的协议列表，例如: ["tcp", "udp", "http", "ws"]
    pub protocols: HashSet<Protocol>,
    pub ips: Vec<(NetworkScope, IpAddr)>,
    /// NAT 转发地址表：每个地址携带独立的 `ip:port`（不只是 IP）。
    ///
    /// NAT 场景下，一个节点可能同时有：
    /// - 私网监听地址（`192.168.x.x:port`，仅内网可达）
    /// - 公网映射地址（中继观察到的 `public ip:port`，NAT 打洞/转发目标）
    ///
    /// `ips` 只存 IP + 单一 `port`，无法表达「私网端口 ≠ 公网映射端口」。
    /// 本字段按 scope 记录每个地址最终的 `ip:port`，供打洞、选路、
    /// 中继转发使用。
    pub nat_addrs: Vec<(NetworkScope, SocketAddr)>,
}

impl Node {
    /// 基础构造：手动传入所有信息
    pub fn new(port: u16, id: Vec<u8>, version: u8, ips: Vec<(NetworkScope, IpAddr)>) -> Self {
        // nat_addrs 初始 = 每个 IP 拼上主监听端口（与 ips+port 等价）。
        let nat_addrs = ips
            .iter()
            .map(|(scope, ip)| (*scope, SocketAddr::new(*ip, port)))
            .collect();
        Self {
            id,
            version,
            port,
            started_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            ips,
            nat_addrs,
            protocols: Self::default_protocols(),
        }
    }

    pub fn from_addr(addr: SocketAddr, version: Option<u8>, id: Option<Vec<u8>>) -> Self {
        let ip = addr.ip();
        let port = addr.port();

        // 1. 自动计算 NetworkScope 并生成元组 Vec
        let scope = crate::connection::node::NetworkScope::from_ip(&ip);
        let ips = match scope {
            NetworkScope::Intranet => Self::system_ips(),
            NetworkScope::Extranet => vec![(scope, ip)],
        };

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
    pub fn from_system(port: u16, id: Vec<u8>, version: u8) -> Self {
        let ips = Self::system_ips();
        let nat_addrs = ips
            .iter()
            .map(|(scope, ip)| (*scope, SocketAddr::new(*ip, port)))
            .collect();
        Self {
            id,
            version,
            port,
            started_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            ips,
            nat_addrs,
            protocols: Self::default_protocols(),
        }
    }

    pub fn system_ips() -> Vec<(NetworkScope, IpAddr)> {
        let mut ips = vec![];
        // 探测本地网卡
        if let Ok(interfaces) = get_if_addrs::get_if_addrs() {
            for interface in interfaces {
                let ip = interface.ip();
                if ip.is_loopback() {
                    continue;
                }

                let scope = NetworkScope::from_ip(&ip);
                ips.push((scope, ip));
            }
        }
        ips
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
        let sa = SocketAddr::new(addr, self.port);
        if !self.nat_addrs.contains(&(scope, sa)) {
            self.nat_addrs.push((scope, sa));
        }
    }

    /// 添加/更新一个 NAT 转发地址（自带独立端口，公网映射地址等）。
    pub fn add_nat_addr(&mut self, scope: NetworkScope, addr: SocketAddr) {
        if let Some(slot) = self.nat_addrs.iter_mut().find(|(s, a)| *s == scope && *a == addr) {
            let _ = slot; // 已存在
        } else {
            self.nat_addrs.push((scope, addr));
        }
    }

    /// 设置某 scope 的 NAT 转发地址（替换同 scope 旧值）。
    pub fn set_nat_addr(&mut self, scope: NetworkScope, addr: SocketAddr) {
        let mut replaced = false;
        let mut to_remove = Vec::new();
        for (s, a) in self.nat_addrs.iter() {
            if *s == scope {
                if *a == addr {
                    replaced = true;
                    break;
                }
                to_remove.push(*a);
            }
        }
        if replaced {
            return;
        }
        for old in to_remove {
            self.nat_addrs
                .retain(|(s, a)| !(*s == scope && *a == old));
        }
        self.nat_addrs.push((scope, addr));
    }

    /// 按 scope 获取 NAT 转发地址（ip:port）。
    pub fn nat_addrs_of(&self, scope: NetworkScope) -> Vec<SocketAddr> {
        self.nat_addrs
            .iter()
            .filter(|(s, _)| *s == scope)
            .map(|(_, a)| *a)
            .collect()
    }

    /// 获取全部 NAT 转发地址（ip:port）。
    pub fn all_nat_addrs(&self) -> Vec<SocketAddr> {
        self.nat_addrs.iter().map(|(_, a)| *a).collect()
    }
}
