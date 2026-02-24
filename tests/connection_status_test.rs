#[cfg(test)]
mod tests {
    use aex::connection::status::ConnectionStatus;
    #[test]
    fn test_connection_status_default() {
        // 覆盖 derive(Default)
        let status = ConnectionStatus::default();
        assert_eq!(status.total_ips, 0);
        assert_eq!(status.total_clients, 0);
        assert_eq!(status.total_servers, 0);
        assert_eq!(status.oldest_uptime, 0);
    }

    #[test]
    fn test_connection_status_display_format() {
        let status = ConnectionStatus {
            total_ips: 5,
            intranet_conns: 2,
            extranet_conns: 8,
            total_clients: 4,
            total_servers: 6,
            oldest_uptime: 3600,
            average_uptime: 1200,
        };

        // 获取格式化后的字符串
        let output = format!("{}", status);

        // 1. 验证关键数据是否出现在字符串中
        assert!(output.contains("Nodes (IPs):      5"));
        assert!(output.contains("Total Conns:      10")); // 4 + 6
        assert!(output.contains("Inbound: 4"));
        assert!(output.contains("Outbound: 6"));
        assert!(output.contains("Intra:   2"));
        assert!(output.contains("Extra:    8"));
        assert!(output.contains("Avg:     1200"));
        assert!(output.contains("Max:      3600"));

        // 2. 验证视觉边框是否完整 (覆盖每一行 writeln!)
        assert!(output.contains("┏━━━━━━━━━━━━━━━━ AEX Connection Profile ━━━━━━━━━━━━━━━┓"));
        assert!(output.contains("┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛"));
    }

    #[test]
    fn test_connection_status_debug() {
        // 覆盖 derive(Debug)，防止覆盖率报告在结构体定义处变红
        let status = ConnectionStatus::default();
        let debug_str = format!("{:?}", status);
        assert!(debug_str.contains("ConnectionStatus"));
        assert!(debug_str.contains("total_ips"));
    }
}