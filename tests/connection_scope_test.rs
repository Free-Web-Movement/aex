#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    use aex::connection::scope::NetworkScope;

    #[test]
    fn test_network_scope_from_ipv4_loopback() {
        let ip = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));
        assert_eq!(NetworkScope::from_ip(&ip), NetworkScope::Intranet);
    }

    #[test]
    fn test_network_scope_from_ipv4_private_10() {
        let ip = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1));
        assert_eq!(NetworkScope::from_ip(&ip), NetworkScope::Intranet);
    }

    #[test]
    fn test_network_scope_from_ipv4_private_172() {
        let ip = IpAddr::V4(Ipv4Addr::new(172, 16, 0, 1));
        assert_eq!(NetworkScope::from_ip(&ip), NetworkScope::Intranet);
    }

    #[test]
    fn test_network_scope_from_ipv4_private_192() {
        let ip = IpAddr::V4(Ipv4Addr::new(192, 168, 0, 1));
        assert_eq!(NetworkScope::from_ip(&ip), NetworkScope::Intranet);
    }

    #[test]
    fn test_network_scope_from_ipv4_link_local() {
        let ip = IpAddr::V4(Ipv4Addr::new(169, 254, 0, 1));
        assert_eq!(NetworkScope::from_ip(&ip), NetworkScope::Intranet);
    }

    #[test]
    fn test_network_scope_from_ipv4_extranet() {
        let ip = IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8));
        assert_eq!(NetworkScope::from_ip(&ip), NetworkScope::Extranet);
    }

    #[test]
    fn test_network_scope_from_ipv6_loopback() {
        let ip = IpAddr::V6(Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1));
        assert_eq!(NetworkScope::from_ip(&ip), NetworkScope::Intranet);
    }

    #[test]
    fn test_network_scope_from_ipv6_link_local() {
        let ip = IpAddr::V6(Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 1));
        assert_eq!(NetworkScope::from_ip(&ip), NetworkScope::Intranet);
    }

    #[test]
    fn test_network_scope_from_ipv6_ula() {
        let ip = IpAddr::V6(Ipv6Addr::new(0xfc00, 0, 0, 0, 0, 0, 0, 1));
        assert_eq!(NetworkScope::from_ip(&ip), NetworkScope::Intranet);
    }

    #[test]
    fn test_network_scope_from_ipv6_extranet() {
        let ip = IpAddr::V6(Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 1));
        assert_eq!(NetworkScope::from_ip(&ip), NetworkScope::Extranet);
    }

    #[test]
    fn test_network_scope_is_intranet() {
        let intranet = NetworkScope::from_ip(&IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)));
        let extranet = NetworkScope::from_ip(&IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)));

        match intranet {
            NetworkScope::Intranet => assert!(true),
            _ => assert!(false, "Expected Intranet"),
        }

        match extranet {
            NetworkScope::Extranet => assert!(true),
            _ => assert!(false, "Expected Extranet"),
        }
    }

    #[test]
    fn test_network_scope_is_extranet() {
        let extranet = NetworkScope::from_ip(&IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)));
        assert!(matches!(extranet, NetworkScope::Extranet));
    }

    #[test]
    fn test_network_scope_traits() {
        let scope1 = NetworkScope::from_ip(&IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)));
        let scope2 = NetworkScope::from_ip(&IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)));
        let scope3 = NetworkScope::from_ip(&IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)));

        assert_eq!(scope1, scope2);
        assert_ne!(scope1, scope3);

        let _ = format!("{:?}", scope1);
        let _ = scope1.clone();
    }
}
