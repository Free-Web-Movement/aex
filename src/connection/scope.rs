use bincode::{Decode, Encode};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Encode, Decode)]
pub enum NetworkScope {
    Intranet, // 内网 (RFC1918, IPv6 LLA/ULA)
    Extranet, // 外网 (公网 IP)
}

impl NetworkScope {
    pub fn from_ip(ip: &std::net::IpAddr) -> Self {
        match ip {
            std::net::IpAddr::V4(v4) => {
                if v4.is_loopback() || v4.is_private() || v4.is_link_local() {
                    NetworkScope::Intranet
                } else {
                    NetworkScope::Extranet
                }
            }
            std::net::IpAddr::V6(v6) => {
                if v6.is_loopback()
                    || v6.is_unicast_link_local()
                    || (v6.segments()[0] & 0xfe00) == 0xfc00
                {
                    NetworkScope::Intranet
                } else {
                    NetworkScope::Extranet
                }
            }
        }
    }
}
