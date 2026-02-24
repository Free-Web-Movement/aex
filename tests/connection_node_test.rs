#[cfg(test)]
mod tests {
    use std::{
        collections::HashSet,
        net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    };

    use aex::connection::{node::Node, protocol::Protocol, types::NetworkScope};

    // --- 1. 基础构造函数测试 ---
    #[test]
    fn test_node_new_and_from_addr() {
        let id = vec![1, 2, 3];
        let addr: SocketAddr = "1.1.1.1:8080".parse().unwrap();

        // 测试 from_addr
        let node = Node::from_addr(addr, Some(2), Some(id.clone()));
        assert_eq!(node.port, 8080);
        assert_eq!(node.version, 2);
        assert_eq!(node.id, id);
        assert_eq!(node.ips.len(), 1);
        assert_eq!(node.ips[0].0, NetworkScope::Extranet);

        // 测试 from_addr 的 None 默认值分支
        let node_default = Node::from_addr(addr, None, None);
        assert_eq!(node_default.version, 1);
        assert_eq!(node_default.id.len(), 32);
    }

    // --- 2. 协议栈操作测试 ---
    #[test]
    fn test_node_protocols() {
        let node = Node::from_addr("127.0.0.1:9000".parse().unwrap(), None, None);

        // 覆盖默认协议
        let defaults = Node::default_protocols();
        assert!(defaults.contains(&Protocol::Tcp));
        assert!(node.protocols.contains(&Protocol::Http));

        // 覆盖 with_protocols (Builder 模式)
        let mut custom_set = HashSet::new();
        custom_set.insert(Protocol::Tcp);
        let node = node.with_protocols(custom_set);
        assert_eq!(node.protocols.len(), 1);
    }

    // --- 3. IP 获取与过滤逻辑 (覆盖 get_ips 的所有分支) ---
    #[test]
    fn test_node_ip_filtering() {
        let mut node = Node::from_addr("127.0.0.1:80".parse().unwrap(), None, None);
        node.ips.clear(); // 清空自动生成的，手动注入以精准控制测试路径

        // 注入多种组合
        node.add_observed_ip(
            NetworkScope::Intranet,
            IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)),
        );
        node.add_observed_ip(
            NetworkScope::Intranet,
            IpAddr::V6(Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1)),
        );
        node.add_observed_ip(
            NetworkScope::Extranet,
            IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)),
        );
        node.add_observed_ip(
            NetworkScope::Extranet,
            IpAddr::V6(Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 1)),
        );

        // 覆盖重复添加判断 (add_observed_ip 的 if !contains 分支)
        let len_before = node.ips.len();
        node.add_observed_ip(
            NetworkScope::Intranet,
            IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)),
        );
        assert_eq!(node.ips.len(), len_before);

        // 覆盖 get_all
        assert_eq!(node.get_all().len(), 4);

        // 覆盖 get_ips 的各个快捷函数和匹配逻辑
        assert_eq!(node.get_intranet_v4().len(), 1);
        assert_eq!(node.get_intranet_v6().len(), 1);
        assert_eq!(node.get_extranet_ips_v4().len(), 1);
        assert_eq!(node.get_extranet_ips_v6().len(), 1);

        // 覆盖 get_ips 的 None 版本分支 (不限 v4/v6)
        assert_eq!(node.get_intranet_ips().len(), 2);
        assert_eq!(node.get_extranet_ips().len(), 2);

        // 覆盖 get_ips 内部 filter 的 Scope 不匹配分支
        // (已经隐含在上面的测试中，因为只返回了符合条件的)
    }

    // --- 4. 系统探测测试 ---
    #[test]
    fn test_from_system() {
        // 这个测试取决于运行机器的网卡，但我们至少要保证它不崩溃
        let node = Node::from_system(1234, vec![0; 32], 1);
        assert_eq!(node.port, 1234);

        // 只要不是所有网卡都是 loopback，ips 就不应该为空
        // 注意：在 CI 环境（如 Github Actions）中，有时只有 loopback
        println!("Detected IPs: {:?}", node.ips);
    }
}
