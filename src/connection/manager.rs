use std::{
    net::{IpAddr, SocketAddr},
    sync::{Arc, atomic::AtomicU64},
    time::{SystemTime, UNIX_EPOCH},
};

use dashmap::DashMap;
use tokio::{
    sync::{Mutex, RwLock},
    task::AbortHandle,
};
use tokio_util::sync::CancellationToken;

use crate::{
    connection::{
        context::Context,
        entry::ConnectionEntry,
        global::GlobalContext,
        scope::NetworkScope,
        status::ConnectionStatus,
        types::BiDirectionalConnections,
    },
    tcp::types::{TCPCommand, TCPFrame},
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

    /// 发起外联连接并自动拆分读写流
    ///
    /// # 参数
    /// * `f`: 业务闭包。接收 Reader (OwnedReadHalf) 和已封装好的 Writer。
    // pub async fn connect<F, Fut>(
    //     &self,
    //     addr: SocketAddr,
    //     global: Arc<GlobalContext>,
    //     f: F,
    // ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
    // where
    //     F: FnOnce(Arc<Mutex<Context>>, CancellationToken) -> Fut + Send + 'static,
    //     Fut: std::future::Future<Output = ()> + Send + 'static,
    // {
    //     // 1. 检查重复连接 (防止对同一地址多次拨号)
    //     let ip = addr.ip();
    //     let scope = NetworkScope::from_ip(&ip);
    //     if let Some(bi_conn) = self.connections.get(&(ip, scope)) {
    //         if bi_conn.servers.contains_key(&addr) {
    //             return Ok(());
    //         }
    //     }

    //     // 2. 拨号：物理连接失败则直接退出
    //     let stream = tokio::net::TcpStream::connect(addr).await?;

    //     // 3. ⚡ 核心步骤：拆分 TcpStream
    //     // into_split() 返回 (OwnedReadHalf, OwnedWriteHalf)
    //     let (raw_reader, raw_writer) = stream.into_split();
    //     // let buf_witer = Box::new(BufWriter::new(raw_writer));

    //     let reader_opt: Option<BoxReader> = Some(Box::new(BufReader::new(raw_reader)));
    //     // let writer_opt: Option<BoxWriter> = Some(buf_witer.clone());

    //     let shared_writer = Arc::new(Mutex::new(Some(
    //         Box::new(BufWriter::new(raw_writer)) as BoxWriter
    //     )));
    //     let writer_for_context = shared_writer.clone();

    //     let writer_opt = {
    //         let mut guard = writer_for_context.lock().await;
    //         guard.take() // 此时 Context 拿到了所有权，shared_writer 内部变为了 None
    //     };

    //     // let writer = shared_writer.clone();

    //     // 5. 准备生命周期工具
    //     let child_token = self.cancel_token.child_token();
    //     let move_token = child_token.clone();

    //     // 初始化Context
    //     let ctx = Arc::new(Mutex::new(Context::new(
    //         reader_opt, writer_opt, global, addr,
    //     )));
    //     let ctx_cloned = ctx.clone();

    //     // 6. 启动异步任务
    //     // 将 Reader 和 Writer 的克隆 移交给闭包
    //     let handle = tokio::spawn(async move {
    //         f(ctx_cloned.clone(), move_token).await;
    //     });

    //     // 7. 登记到管理池
    //     // 💡 优化：这里我们直接把构造好的 writer 存入 Entry，省去了后续再 update 的麻

    //     self.add(
    //         addr,
    //         handle.abort_handle(),
    //         child_token,
    //         false,
    //         Some(ctx),
    //         // Some(writer),
    //     );

    //     Ok(())
    // }
    pub async fn connect<F, C, FF, Fut>(
        &self,
        addr: SocketAddr,
        global: Arc<GlobalContext>,
        f: FF,
        timeout_secs: Option<u64>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
    where
        F: TCPFrame,
        C: TCPCommand,
        FF: FnOnce(Arc<Mutex<Context>>) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        let ip = addr.ip();
        let scope = NetworkScope::from_ip(&ip);
        if let Some(bi_conn) = self.connections.get(&(ip, scope)) {
            if bi_conn.servers.contains_key(&addr) {
                return Ok(());
            }
        }

        let timeout = timeout_secs.unwrap_or(10);
        let socket = match tokio::time::timeout(std::time::Duration::from_secs(timeout), tokio::net::TcpStream::connect(addr)).await {
            Ok(Ok(socket)) => socket,
            Ok(Err(e)) => return Err(Box::new(e)),
            Err(_) => {
                let err = std::io::Error::new(std::io::ErrorKind::TimedOut, "connection timeout");
                return Err(Box::new(err));
            }
        };

        let pipeline = ConnectionEntry::default_pipeline::<F, C>(addr, false);

        // 4. 使用统一的启动器
        // 传入 manager 的 token 作为父级，获取该连接独有的 token 和 handle
        let (conn_token, abort_handle, ctx) = ConnectionEntry::start::<_, _>(
            self.cancel_token.clone(),
            socket,
            addr,
            global.clone(),
            move |ctx: Arc<Mutex<Context>>| {
                Box::pin(async move {
                    let _ = pipeline(ctx.clone()).await;
                    f(ctx).await;
                    Ok(())
                })
            },
        );

        // 5. 登记到管理池 (is_server = false)
        self.add(addr, abort_handle, conn_token, false, Some(ctx));

        Ok(())
    }

    pub fn add(
        &self,
        addr: SocketAddr,
        handle: AbortHandle,
        cancel_token: CancellationToken,
        is_client: bool,
        context: Option<Arc<Mutex<Context>>>, // writer: Option<Arc<Mutex<Option<BoxWriter>>>>,
    ) {
        let ip = addr.ip();
        
        // 跳过 loopback 地址
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

        let entry = Arc::new(ConnectionEntry {
            addr,
            node: Arc::new(RwLock::new(None)), // 初始时没有节点信息，握手完成后会填充
            // writer,
            abort_handle: handle,
            connected_at: now, // 💡 记录建立时间
            context,
            cancel_token,
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

    pub fn update(&self, addr: SocketAddr, is_client: bool, context: Option<Arc<Mutex<Context>>>) {
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
                    context,
                    // writer: Some(writer), // ⚡ 填充传入的 writer
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
        self.cancel_token.cancel();

        let mut handles = Vec::new();

        // 1. 快速收集所有 handle 并清空 Map
        // 使用 drain 可以获取所有权并自动释放锁
        for mut bucket in self.connections.iter_mut() {
            let bi_conn = bucket.value_mut();
            bi_conn
                .clients
                .iter()
                .for_each(|r| handles.push(r.value().abort_handle.clone()));
            bi_conn
                .servers
                .iter()
                .for_each(|r| handles.push(r.value().abort_handle.clone()));
            bi_conn.clients.clear();
            bi_conn.servers.clear();
        }
        self.connections.clear();

        // 2. 在锁外物理掐断
        for h in handles {
            h.abort();
        }
        println!("ConnectionManager: All connections physically aborted.");
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

    /// 根据 Node ID 获取所有相关的连接句柄
    /// f 是一个闭包，允许你在获取到列表后立即执行操作
    pub async fn notify<F, Fut>(&self, node_id: &[u8], f: F)
    where
        F: FnOnce(Vec<Arc<ConnectionEntry>>) -> Fut,
        Fut: std::future::Future<Output = ()>,
    {
        let mut all_entries = Vec::new();

        // 1. 【同步阶段】快速收集所有 Arc 引用
        // 这一步很快，且不涉及 .await，能迅速释放 DashMap 的分片锁
        for bucket_ref in self.connections.iter() {
            let bi_conn = bucket_ref.value();

            for entry_ref in bi_conn.clients.iter() {
                all_entries.push(Arc::clone(entry_ref.value()));
            }
            for entry_ref in bi_conn.servers.iter() {
                all_entries.push(Arc::clone(entry_ref.value()));
            }
        }

        // 2. 【异步阶段】过滤匹配的 Node ID
        let mut matched = Vec::new();
        for entry in all_entries {
            let is_match = {
                // 在这个小作用域内获取锁
                let node_lock = entry.node.read().await;
                if let Some(node) = node_lock.as_ref() {
                    node.id == node_id
                } else {
                    false
                }
            }; // 👈 锁 (node_lock) 在这里被自动 Drop

            // 此时 node_lock 已经不存在了，可以安全地移动 entry (它是 Arc，克隆它也行)
            if is_match {
                matched.push(entry);
            }
        }

        // 3. 执行回调
        f(matched).await
    }

    // 获取所有连接
    pub async fn forward<F, Fut>(&self, f: F)
    where
        F: FnOnce(Vec<Arc<ConnectionEntry>>) -> Fut,
        Fut: std::future::Future<Output = ()>,
    {
        let mut targets = Vec::new();

        // 遍历所有 IP 桶
        for bucket_ref in self.connections.iter() {
            let bi_conn = bucket_ref.value();

            // 辅助函数：检查 Node ID 是否匹配
            let mut collect_matching = |entry: &Arc<ConnectionEntry>| {
                targets.push(Arc::clone(entry));
            };

            // 检查该 IP 下的所有客户端和服务器连接
            bi_conn
                .clients
                .iter()
                .for_each(|r| collect_matching(r.value()));
            bi_conn
                .servers
                .iter()
                .for_each(|r| collect_matching(r.value()));
        }

        // 执行回调
        f(targets).await;
    }
}
