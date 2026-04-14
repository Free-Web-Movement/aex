use crate::connection::context::{BoxReader, BoxWriter, Context, TypeMapExt};
use crate::connection::global::GlobalContext;
use crate::connection::heartbeat::{HeartbeatConfig, HeartbeatManager};
use crate::connection::node::Node;
use crate::connection::types::IDExtractor;
use crate::tcp::types::{TCPCommand, TCPFrame};

use crate::http::router::Router as HttpRouter;
use crate::tcp::router::Router as TcpRouter;
#[allow(unused_imports)]
use crate::http2::H2Codec;
use std::fmt;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::io::{BufReader, BufWriter};
use tokio::net::TcpStream;
use tokio::sync::{Mutex, RwLock};
use tokio::task::AbortHandle;
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub struct ConnectionEntry {
    /// 💡 新增：节点的静态信息（ID, Version, 声明的 IPs 等）
    /// 这个数据在握手成功后填入，并在连接生命周期内保持不变
    pub node: Arc<RwLock<Option<Node>>>,
    pub addr: SocketAddr,
    pub abort_handle: AbortHandle,
    pub context: Option<Arc<Mutex<Context>>>,
    pub cancel_token: CancellationToken,
    pub connected_at: u64,
    /// 最后活跃时间戳（秒）
    pub last_seen: Arc<AtomicU64>,
}

// 手动实现 Debug 以跳过 writer
impl fmt::Debug for ConnectionEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConnectionEntry")
            .field("addr", &self.addr)
            .field("connected_at", &self.connected_at)
            // 如果有 last_seen 等字段，照常添加
            .field("last_seen", &self.last_seen)
            .finish()
    }
}

impl ConnectionEntry {
    pub fn new_empty_node(
        addr: SocketAddr,
        context: Option<Arc<Mutex<Context>>>,
        handle: tokio::task::AbortHandle,
        cancel_token: CancellationToken,
    ) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        Self {
            node: Arc::new(RwLock::new(None)),
            addr,
            // writer,
            abort_handle: handle,
            cancel_token,
            context,
            connected_at: now,
            last_seen: Arc::new(AtomicU64::new(now)),
        }
    }

    pub fn uptime_secs(&self) -> u64 {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        now.saturating_sub(self.connected_at)
    }

    /// 判定该连接是否应当被停用（根据心跳限制和寿命限制）
    /// @param timeout_secs: 最大静默允许时间
    /// @param max_lifetime_secs: 最大允许存活时长
    pub fn is_deactivated(&self, current: u64, timeout_secs: u64, max_lifetime_secs: u64) -> bool {
        // 1. 检查寿命
        let uptime = current.saturating_sub(self.connected_at);
        if uptime >= max_lifetime_secs {
            return true;
        }

        // 2. 检查活跃度
        let last_active = self.last_seen.load(Ordering::Relaxed);
        let idle_time = current.saturating_sub(last_active);
        if idle_time >= timeout_secs {
            return true;
        }

        false
    }

    /// 动态更新节点信息（例如收到对方的地址交换报文或心跳包时）
    pub async fn update_node(&self, new_node: Node) {
        let mut lock = self.node.write().await;
        *lock = Some(new_node);
    }

    /// 尝试获取当前的节点 ID
    pub async fn get_peer_id(&self) -> Option<Vec<u8>> {
        let lock = self.node.read().await;
        lock.as_ref().map(|n| n.id.clone())
    }

    /// 启动心跳
    pub fn start_heartbeat(&self, local_node: Node, config: HeartbeatConfig) {
        let ctx = match &self.context {
            Some(c) => c.clone(),
            None => return,
        };
        
        let heartbeat = HeartbeatManager::new(local_node).with_config(config);
        heartbeat.start_server_heartbeat(ctx, self.addr, self.cancel_token.clone());
    }

    /// 启动心跳（使用全局配置）
    pub async fn start_heartbeat_with_global(&self, global: &Arc<GlobalContext>) {
        let config = global.heartbeat_config.clone();
        let local_node = global.local_node.clone();
        let guard = local_node.read().await;
        let node = (*guard).clone();
        drop(guard);
        self.start_heartbeat(node, config);
    }

    pub fn default_pipeline<F, C>(
        peer_addr: std::net::SocketAddr,
        is_server: bool,
        extractor: IDExtractor<C>,
    ) -> impl FnOnce(Arc<Mutex<Context>>) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send>>
    where
        F: TCPFrame,
        C: TCPCommand,
    {
        move |ctx: Arc<Mutex<Context>>| {
            Box::pin(async move {
                // 1. 获取全局上下文并注册连接
                let gtx = {
                    let guard = ctx.lock().await;
                    let gtx = guard.global.clone();
                    // 统一在此处更新 Manager 索引
                    gtx.manager.update(peer_addr, is_server, Some(ctx.clone()));
                    gtx
                };

                // 2. 协议嗅探：HTTP/1.1
                if let Some(hr) = gtx.routers.get_value::<Arc<HttpRouter>>() {
                    if hr.clone().is_http(ctx.clone()).await? {
                        return Ok(());
                    }
                }

                // 3. 自定义 TCP 路由处理
                if let Some(tr) = crate::connection::context::get_tcp_router(&gtx.routers) {
                    return tr.handle::<F, C>(ctx, extractor).await;
                }

                Ok(())
            })
        }
    }

    pub fn start<F, C, FF, Fut>(
        parent_token: CancellationToken, // 传入全局或上层令牌
        socket: TcpStream,
        addr: std::net::SocketAddr,
        global: Arc<GlobalContext>,
        f: FF,
    ) -> (CancellationToken, tokio::task::AbortHandle, Arc<Mutex<Context>>)
    where
        F: TCPFrame,
        C: TCPCommand,
        FF: FnOnce(Arc<Mutex<Context>>) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        // 1. ⚡ 关键：派生子令牌，这样外部可以单独 cancel 这个连接而不影响全局
        let child_token = parent_token.child_token();
        let task_token = child_token.clone();

        let (reader, writer) = socket.into_split();

        let r_opt: Option<BoxReader> = Some(Box::new(BufReader::new(reader)));
        let w_opt: Option<BoxWriter> = Some(Box::new(BufWriter::new(writer)));

        let mut raw_ctx = Context::new(
            r_opt,
            w_opt,
            global,
            addr
        );
        raw_ctx.set(task_token.clone()); 
        let ctx = Arc::new(Mutex::new(raw_ctx));

        let ctx_cloned = ctx.clone();
        let join_handle = tokio::spawn(async move {
            tokio::select! {
                // 监听子令牌或父令牌的取消信号
                _ = task_token.cancelled() => {
                    Ok::<(), anyhow::Error>(())
                }

                res = async {
                    // 执行业务逻辑
                    f(ctx_cloned).await?;

                    Ok(())
                } => {
                    if let Err(e) = &res {
                         tracing::warn!("Pipeline error for {}: {}", addr, e);
                    }
                    res
                }
            }
        });

        // 2. 返回子令牌和句柄，外部 Entry 可以直接存入这两个值
        (child_token, join_handle.abort_handle(), ctx)
    }
}

impl Drop for ConnectionEntry {
    fn drop(&mut self) {
        // 确保当 Entry 彻底离开内存时，协程任务一定停止
        self.abort_handle.abort();
    }
}
