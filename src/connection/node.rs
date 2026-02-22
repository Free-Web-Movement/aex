use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    time::{SystemTime, UNIX_EPOCH},
};

use crate::connection::types::NetworkScope;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    pub id: Vec<u8>,     // 节点 ID，通常是公钥的哈希
    pub version: u32,    // 协议版本
    pub started_at: u64, // 启动时间戳
    pub port: u16,       // 监听端口,
    pub(crate) ips: Vec<(NetworkScope, IpAddr)>,
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
        }
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
        };

        // 探测本地网卡
        if let Ok(interfaces) = get_if_addrs::get_if_addrs() {
            for interface in interfaces {
                let ip = interface.ip();
                if ip.is_loopback() {
                    continue;
                }

                let scope = Self::get_scope(ip);
                node.ips.push((scope, ip));
            }
        }
        node
    }

    pub fn get_scope(ip: IpAddr) -> NetworkScope {
        let is_internal = match ip {
            IpAddr::V4(v4) => {
                // IPv4: 检查回环、私有地址 (RFC1918)、链路本地 (169.254.x.x)
                v4.is_loopback() || v4.is_private() || v4.is_link_local()
            }
            IpAddr::V6(v6) => {
                // IPv6: 检查回环 (::1)、链路本地 (fe80::/10)
                // 注意：v6.is_private() 目前在稳定版 Rust 中可能不可用
                // 我们通过检查是否是 Unique Local Address (fc00::/7) 来判定私网
                v6.is_loopback()
                    || v6.is_unicast_link_local()
                    || (v6.segments()[0] & 0xfe00) == 0xfc00
            }
        };

        if is_internal {
            NetworkScope::Intranet
        } else {
            NetworkScope::Extranet
        }
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

    // fn get_ips(&self, scope: NetworkScope) -> Vec<SocketAddr> {
    //     self.ips
    //         .iter()
    //         .filter_map(|(s, addr)| if *s == scope { Some(*addr) } else { None })
    //         .collect()
    // }

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
