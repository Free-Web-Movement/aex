use std::fmt;


#[derive(Debug, Default)]
pub struct ConnectionStatus {
    pub total_ips: usize,          // 独立 IP 数量
    pub intranet_conns: usize,     // 内网总连接数
    pub extranet_conns: usize,     // 外网总连接数
    pub total_clients: usize,      // 总入站连接
    pub total_servers: usize,      // 总出站连接
    pub oldest_uptime: u64,        // 最长连接时长
    pub average_uptime: u64,       // 平均连接时长
}

impl fmt::Display for ConnectionStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let total_conns = self.total_clients + self.total_servers;
        
        writeln!(f, "┏━━━━━━━━━━━━━━━━ AEX Connection Profile ━━━━━━━━━━━━━━━┓")?;
        writeln!(f, "┃  Nodes (IPs):      {: <40} ┃", self.total_ips)?;
        writeln!(f, "┃  Total Conns:      {: <40} ┃", total_conns)?;
        writeln!(f, "┠──────────────────────────────────────────────────────┨")?;
        writeln!(f, "┃  Direction:        Inbound: {: <10} Outbound: {: <10} ┃", 
            self.total_clients, self.total_servers)?;
        writeln!(f, "┃  Network Scope:    Intra:   {: <10} Extra:    {: <10} ┃", 
            self.intranet_conns, self.extranet_conns)?;
        writeln!(f, "┠──────────────────────────────────────────────────────┨")?;
        writeln!(f, "┃  Uptime (secs):    Avg:     {: <10} Max:      {: <10} ┃", 
            self.average_uptime, self.oldest_uptime)?;
        write!(f,   "┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛")
    }
}