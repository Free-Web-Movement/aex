#[cfg(test)]
mod tests {
    use std::{
        collections::HashSet,
        net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    };

    use aex::connection::{node::Node, protocol::Protocol, scope::NetworkScope};

    #[test]
    fn test_node_new_sets_all_fields() {
        let node = Node::new(
            9999,
            vec![1, 2, 3, 4],
            3,
            vec![(NetworkScope::Extranet, IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)))],
        );
        assert_eq!(node.port, 9999);
        assert_eq!(node.id, vec![1, 2, 3, 4]);
        assert_eq!(node.version, 3);
        assert!(node.started_at > 0, "started_at 应取当前时间戳");
        assert_eq!(node.ips.len(), 1);
        assert_eq!(node.protocols.len(), 4);
        assert!(node.protocols.contains(&Protocol::Tcp));
        assert!(node.protocols.contains(&Protocol::Udp));
        assert!(node.protocols.contains(&Protocol::Http));
        assert!(node.protocols.contains(&Protocol::Ws));
    }

    #[test]
    fn test_node_from_addr_intranet_branch() {
        let node = Node::from_addr("127.0.0.1:8000".parse().unwrap(), None, None);
        assert_eq!(node.port, 8000);
        assert_eq!(node.version, 1);
        assert_eq!(node.id.len(), 32);
        let _ = node.ips;
    }

    #[test]
    fn test_node_with_protocols_empty_set() {
        let node = Node::from_addr("1.1.1.1:80".parse().unwrap(), None, None)
            .with_protocols(HashSet::new());
        assert!(node.protocols.is_empty());
    }

    #[test]
    fn test_node_get_ips_mismatch_returns_empty() {
        let mut node = Node::from_addr("1.1.1.1:80".parse().unwrap(), None, None);
        node.ips.clear();
        node.add_observed_ip(NetworkScope::Intranet, IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)));

        assert!(node
            .get_ips(
                NetworkScope::Intranet,
                Some(IpAddr::V6(Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1)))
            )
            .is_empty());
        assert!(node.get_ips(NetworkScope::Extranet, None).is_empty());
    }

    #[test]
    fn test_node_get_ips_mixed_family_filter() {
        let mut node = Node::from_addr("1.1.1.1:80".parse().unwrap(), None, None);
        node.ips.clear();
        node.add_observed_ip(NetworkScope::Intranet, IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)));
        node.add_observed_ip(
            NetworkScope::Intranet,
            IpAddr::V6(Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1)),
        );

        let v6 = node.get_ips(
            NetworkScope::Intranet,
            Some(IpAddr::V6(Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1))),
        );
        assert_eq!(v6.len(), 1);
        assert!(v6[0].is_ipv6());

        let v4 = node.get_ips(
            NetworkScope::Intranet,
            Some(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0))),
        );
        assert_eq!(v4.len(), 1);
        assert!(v4[0].is_ipv4());
    }

    #[test]
    fn test_node_get_ips_shortcuts_empty_when_no_match() {
        let node = Node::new(
            80,
            vec![1],
            1,
            vec![(NetworkScope::Intranet, IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10)))],
        );
        assert!(node.get_extranet_ips().is_empty());
        assert!(node.get_extranet_ips_v4().is_empty());
        assert!(node.get_extranet_ips_v6().is_empty());
        assert!(node.get_intranet_v6().is_empty());
        assert_eq!(node.get_intranet_ips().len(), 1);
    }

    #[test]
    fn test_node_add_observed_ip_new_and_get_all() {
        let mut node = Node::from_addr("1.1.1.1:80".parse().unwrap(), None, None);
        let before = node.ips.len();
        node.add_observed_ip(NetworkScope::Extranet, IpAddr::V4(Ipv4Addr::new(9, 9, 9, 9)));
        assert_eq!(node.ips.len(), before + 1);
        assert!(node
            .get_all()
            .contains(&IpAddr::V4(Ipv4Addr::new(9, 9, 9, 9))));
    }

    #[test]
    fn test_node_system_ips_no_loopback() {
        let ips = Node::system_ips();
        for (_, ip) in &ips {
            assert!(!ip.is_loopback(), "system_ips 应跳过 loopback: {ip}");
        }
    }

    #[test]
    fn test_node_serde_round_trip() {
        let node = Node::new(
            8080,
            vec![9, 9, 9],
            2,
            vec![(NetworkScope::Extranet, IpAddr::V4(Ipv4Addr::new(1, 2, 3, 4)))],
        );
        let json = serde_json::to_string(&node).unwrap();
        let decoded: Node = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, node);
    }

    #[test]
    fn test_node_bincode_round_trip() {
        let node = Node::from_addr("1.1.1.1:8080".parse().unwrap(), Some(5), Some(vec![7; 32]));
        let bytes = bincode::encode_to_vec(&node, aex::tcp::types::frame_config()).unwrap();
        let (decoded, len): (Node, usize) =
            bincode::decode_from_slice(&bytes, aex::tcp::types::frame_config()).unwrap();
        assert_eq!(decoded, node);
        assert_eq!(len, bytes.len());
    }
}
