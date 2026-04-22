use crate::connection::context::{BoxReader, BoxWriter, Context};
use crate::connection::global::GlobalContext;
use crate::connection::heartbeat::{HeartbeatConfig, HeartbeatManager};
use crate::connection::node::Node;
use crate::tcp::types::{TCPFrame, TCPCommand};
use std::fmt;
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::io::{BufReader, BufWriter};
use tokio::net::TcpStream;
use tokio::sync::{Mutex, RwLock};
use tokio::task::AbortHandle;

#[derive(Clone)]
pub struct ConnectionEntry {
    pub node: Arc<RwLock<Option<Node>>>,
    pub addr: SocketAddr,
    pub abort_handle: AbortHandle,
    pub context: Option<Arc<Mutex<Context>>>,
    pub cancel_token: tokio_util::sync::CancellationToken,
    pub connected_at: u64,
    pub last_seen: Arc<AtomicU64>,
}

impl fmt::Debug for ConnectionEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConnectionEntry")
            .field("addr", &self.addr)
            .field("connected_at", &self.connected_at)
            .field("last_seen", &self.last_seen)
            .finish()
    }
}

impl ConnectionEntry {
    pub fn new_empty_node(
        addr: SocketAddr,
        context: Option<Arc<Mutex<Context>>>,
        handle: tokio::task::AbortHandle,
        cancel_token: tokio_util::sync::CancellationToken,
    ) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        Self {
            node: Arc::new(RwLock::new(None)),
            addr,
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

    pub fn is_deactivated(&self, current: u64, timeout_secs: u64, max_lifetime_secs: u64) -> bool {
        self.uptime_secs() >= max_lifetime_secs || self.last_seen.load(Ordering::Relaxed) + timeout_secs < current
    }

    pub async fn update_node(&self, new_node: Node) {
        let mut lock = self.node.write().await;
        *lock = Some(new_node);
    }

    pub async fn get_peer_id(&self) -> Option<Vec<u8>> {
        let lock = self.node.read().await;
        lock.as_ref().map(|n| n.id.clone())
    }

    pub fn start_heartbeat(&self, local_node: Node, config: HeartbeatConfig) {
        let ctx = match &self.context {
            Some(c) => c.clone(),
            None => return,
        };
        
        let heartbeat = HeartbeatManager::new(local_node).with_config(config);
        heartbeat.start_server_heartbeat(ctx, self.addr, self.cancel_token.clone());
    }

    pub async fn start_heartbeat_with_global(&self, global: &Arc<GlobalContext>) {
        let config = global.heartbeat_config.clone();
        let local_node = global.local_node.clone();
        let guard = local_node.read().await;
        let node = (*guard).clone();
        drop(guard);
        self.start_heartbeat(node, config);
    }

    pub fn start<FF, Fut>(
        parent_token: tokio_util::sync::CancellationToken,
        socket: TcpStream,
        addr: std::net::SocketAddr,
        global: Arc<GlobalContext>,
        f: FF,
    ) -> (tokio_util::sync::CancellationToken, tokio::task::AbortHandle, Arc<Mutex<Context>>)
    where
        FF: FnOnce(Arc<Mutex<Context>>) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = anyhow::Result<()>> + Send + 'static,
    {
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
                _ = task_token.cancelled() => {
                    Ok::<(), anyhow::Error>(())
                }

                res = async {
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

        (child_token, join_handle.abort_handle(), ctx)
    }

    pub fn default_pipeline<F, C>(
        peer_addr: SocketAddr,
        is_server: bool,
    ) -> impl FnOnce(Arc<Mutex<Context>>) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send>> + Send
    where
        F: TCPFrame + Send + 'static,
        C: TCPCommand + Send + 'static,
    {
        move |_ctx: Arc<Mutex<Context>>| {
            Box::pin(async move {
                Ok(())
            }) as Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send>>
        }
    }
}

impl Drop for ConnectionEntry {
    fn drop(&mut self) {
        self.abort_handle.abort();
    }
}
