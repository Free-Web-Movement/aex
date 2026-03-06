use std::{
    net::{IpAddr, SocketAddr},
    sync::{Arc, atomic::AtomicU64},
    time::{SystemTime, UNIX_EPOCH},
};

use dashmap::DashMap;
use tokio::
    sync::{Mutex, RwLock}
;
use tokio_util::sync::CancellationToken;

use crate::connection::{
    context::BoxWriter, status::ConnectionStatus, types::{BiDirectionalConnections, ConnectionEntry, NetworkScope}
};

pub struct ConnectionManager {
    // 用于通知所有连接优雅退出的信号
    pub cancel_token: CancellationToken,
    /// 入站连接池：其他节点连入 (Inbound)
    pub connections: DashMap<(IpAddr, NetworkScope), BiDirectionalConnections>,
    // pub(crate) index_by_id: DashMap<Vec<u8>, SocketAddr>,
}

impl Default for ConnectionManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ConnectionManager {
    pub fn new() -> Self {
        Self {
            cancel_token: CancellationToken::new(),
            connections: DashMap::new(),
        }
    }

    pub fn add(&self, addr: SocketAddr, handle: tokio::task::AbortHandle, is_client: bool) {
        let ip = addr.ip();
        if ip.is_loopback() {
            return;
        }

        let scope = NetworkScope::from_ip(&ip);
        let key = (ip, scope);

        // 获取当前时间戳
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let child_token = self.cancel_token.child_token();

        let entry = Arc::new(ConnectionEntry {
            addr,
            node: Arc::new(RwLock::new(None)), // 初始时没有节点信息，握手完成后会填充
            writer: None,
            abort_handle: handle,
            connected_at: now, // 💡 记录建立时间
            cancel_token: child_token,
            last_seen: Arc::new(AtomicU64::new(now)), // 初始活跃时间等于建立时间
        });

        // DashMap 写入逻辑
        let bi_conn = self
            .connections
            .entry(key)
            .or_insert_with(BiDirectionalConnections::new);
        if is_client {
            bi_conn.clients.insert(addr, entry);
        } else {
            bi_conn.servers.insert(addr, entry);
        }
    }

    pub fn update(&self, addr: SocketAddr, is_client: bool, writer: Arc<Mutex<BoxWriter>>) {
        let ip = addr.ip();
        let scope = NetworkScope::from_ip(&ip);
        let key = (ip, scope);

        // 1. 获取 IP 桶的可变引用
        if let Some(mut bi_conn) = self.connections.get_mut(&key) {
            // 2. 根据方向定位到具体的 DashMap (clients 或 servers)
            let target_map = if is_client {
                &mut bi_conn.clients
            } else {
                &mut bi_conn.servers
            };

            // 3. ⚡ 使用 DashMap 的 entry API 定位并更新
            // 如果存在该地址的 Entry，我们创建一个包含了 writer 的新 Arc 实例
            if let Some(mut entry_ref) = target_map.get_mut(&addr) {
                let old_entry = entry_ref.value();

                // 重新构造一个新的 Arc，保留原有的其他信息，只更新 writer
                let new_entry = Arc::new(ConnectionEntry {
                    addr: old_entry.addr,
                    node: old_entry.node.clone(),
                    writer: Some(writer), // ⚡ 填充传入的 writer
                    abort_handle: old_entry.abort_handle.clone(),
                    connected_at: old_entry.connected_at,
                    cancel_token: old_entry.cancel_token.clone(),
                    last_seen: old_entry.last_seen.clone(),
                });

                // 替换旧的 Arc
                *entry_ref.value_mut() = new_entry;
            }
        }
    }

    pub fn remove(&self, addr: SocketAddr, is_client: bool) {
        let ip = addr.ip();
        let scope = NetworkScope::from_ip(&ip);

        if let Some(bi_conn) = self.connections.get(&(ip, scope)) {
            if is_client {
                bi_conn.clients.remove(&addr);
            } else {
                bi_conn.servers.remove(&addr);
            }

            // 💡 进阶逻辑：如果该 IP 下已经没有任何连接了，清理掉这个桶
            if bi_conn.clients.is_empty() && bi_conn.servers.is_empty() {
                // 释放引用后，在外层删除
                drop(bi_conn);
                self.connections.remove(&(ip, scope));
            }
        }
    }

    /// 根据 SocketAddr 强制断开并取消特定的连接任务
    /// 返回值表示是否成功找到了该连接并执行了取消操作
    pub fn cancel_by_addr(&self, addr: SocketAddr) -> bool {
        let ip = addr.ip();
        let scope = NetworkScope::from_ip(&ip);
        let key = (ip, scope);

        let mut found = false;

        // 1. 使用作用域或显式 drop 确保 bi_conn 的引用在 check_and_cleanup 之前释放
        {
            if let Some(bi_conn) = self.connections.get(&key) {
                if let Some((_, entry)) = bi_conn.clients.remove(&addr) {
                    entry.abort_handle.abort();
                    found = true;
                } else if let Some((_, entry)) = bi_conn.servers.remove(&addr) {
                    entry.abort_handle.abort();
                    found = true;
                }
            }
        } // <--- 这里 bi_conn (Ref) 被 drop，释放了分片锁

        if found {
            self.check_and_cleanup_bucket(key); // 现在可以安全地重新获取锁或执行 remove
        }

        found
    }

    /// 内部辅助：当某个 IP 桶完全为空时，从全局 Map 中移除以节省内存
    pub fn check_and_cleanup_bucket(&self, key: (IpAddr, NetworkScope)) {
        // 使用 get_mut 或 entry 以确保逻辑连贯
        if let Some(bi_conn) = self.connections.get(&key)
            && bi_conn.clients.is_empty()
            && bi_conn.servers.is_empty()
        {
            // 必须手动显式 drop 掉 bi_conn 引用，否则 remove 会造成死锁（Ref 锁住了分片）
            drop(bi_conn);
            self.connections.remove(&key);
        }
    }

    /// 取消该 IP 下的所有连接（无论是哪个端口，无论是入站还是出站）
    pub fn cancel_all_by_ip(&self, ip: IpAddr) {
        let scope = NetworkScope::from_ip(&ip);
        if let Some((_, bi_conn)) = self.connections.remove(&(ip, scope)) {
            // 遍历清理所有入站
            for r in bi_conn.clients {
                r.1.abort_handle.abort();
            }
            // 遍历清理所有出站
            for r in bi_conn.servers {
                r.1.abort_handle.abort();
            }
        }
    }

    /// 遍历所有连接，根据传入的参数策略执行停用
    pub fn deactivate(&self, timeout_secs: u64, max_lifetime_secs: u64) {
        let current = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let mut empty_buckets = Vec::new();

        // DashMap 的 iter_mut 锁定分片进行原地修改
        for mut bucket_ref in self.connections.iter_mut() {
            let (key, bi_conn) = bucket_ref.pair_mut();

            // 封装后的代码极其简洁
            let mut cleaner = |_: &SocketAddr, entry: &mut Arc<ConnectionEntry>| {
                if entry.is_deactivated(current, timeout_secs, max_lifetime_secs) {
                    entry.abort_handle.abort();
                    return false; // 从 Map 中移除
                }
                true
            };

            bi_conn.clients.retain(&mut cleaner);
            bi_conn.servers.retain(&mut cleaner);

            if bi_conn.clients.is_empty() && bi_conn.servers.is_empty() {
                empty_buckets.push(*key);
            }
        }

        // 清理由于断开连接而产生的空 IP 桶
        for key in empty_buckets {
            self.connections.remove(&key);
        }
    }

    pub fn status(&self) -> ConnectionStatus {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let mut status = ConnectionStatus {
            total_ips: self.connections.len(),
            ..Default::default()
        };

        let mut total_uptime = 0u64;
        let mut conn_count = 0usize;

        // 遍历所有 IP 桶
        for bucket_ref in self.connections.iter() {
            let (key, bi_conn) = bucket_ref.pair();
            let scope = key.1;

            // 获取内外网属性
            let is_intranet = matches!(scope, NetworkScope::Intranet);

            // 统计该 IP 下的所有连接
            let client_count = bi_conn.clients.len();
            let server_count = bi_conn.servers.len();

            status.total_clients += client_count;
            status.total_servers += server_count;

            if is_intranet {
                status.intranet_conns += client_count + server_count;
            } else {
                status.extranet_conns += client_count + server_count;
            }

            // 统计 Uptime (由于 DashMap 嵌套，我们需要进一步遍历)
            let mut process_uptime = |entry: &Arc<ConnectionEntry>| {
                let uptime = now.saturating_sub(entry.connected_at);
                if uptime > status.oldest_uptime {
                    status.oldest_uptime = uptime;
                }
                total_uptime += uptime;
                conn_count += 1;
            };

            bi_conn
                .clients
                .iter()
                .for_each(|r| process_uptime(r.value()));
            bi_conn
                .servers
                .iter()
                .for_each(|r| process_uptime(r.value()));
        }

        if conn_count > 0 {
            status.average_uptime = total_uptime / (conn_count as u64);
        }

        status
    }

    /// 全局关闭：停止所有连接任务并清理所有内存
    pub fn shutdown(&self) {
        // 1. 发送全局取消信号
        // 配合每个连接任务中对此 token 的 select! 监听
        self.cancel_token.cancel();

        // 2. 物理切断：强制 abort 每一个现有的任务
        // 我们利用现有的迭代逻辑，确保不漏掉任何一个
        for mut bucket_ref in self.connections.iter_mut() {
            let (_, bi_conn) = bucket_ref.pair_mut();

            bi_conn
                .clients
                .iter()
                .for_each(|r| r.value().abort_handle.abort());
            bi_conn
                .servers
                .iter()
                .for_each(|r| r.value().abort_handle.abort());

            bi_conn.clients.clear();
            bi_conn.servers.clear();
        }

        // 3. 清空整个路由表
        self.connections.clear();

        println!("ConnectionManager: All connections shut down and cleared.");
    }

    /// 优雅地取消单个连接：先发信号，让任务自己处理后事
    pub fn cancel_gracefully(&self, addr: SocketAddr) -> bool {
        let ip = addr.ip();
        let scope = NetworkScope::from_ip(&ip);

        if let Some(bi_conn) = self.connections.get(&(ip, scope)) {
            // 尝试在 clients 或 servers 中找到 entry
            let entry = bi_conn
                .clients
                .get(&addr)
                .or_else(|| bi_conn.servers.get(&addr));

            if let Some(e) = entry {
                e.cancel_token.cancel(); // 仅取消这一个
                return true;
            }
        }
        false
    }
}
