use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
#[allow(unused_imports)]
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Mutex;
use tokio::task::AbortHandle;
use tokio_util::sync::CancellationToken;

use crate::connection::commands::{PingCommand, PongCommand};
use crate::connection::context::Context;
use crate::connection::node::Node;
use crate::crypto::session_key_manager::PairedSessionKey;

const DEFAULT_PING_INTERVAL: u64 = 30;
const DEFAULT_PING_TIMEOUT: u64 = 10;

#[derive(Clone)]
pub struct HeartbeatConfig {
    pub interval_secs: u64,
    pub timeout_secs: u64,
    pub on_timeout: Option<Arc<dyn Fn(SocketAddr) + Send + Sync>>,
    pub on_latency: Option<Arc<dyn Fn(SocketAddr, u64) + Send + Sync>>,
}

impl HeartbeatConfig {
    pub fn new() -> Self {
        Self {
            interval_secs: DEFAULT_PING_INTERVAL,
            timeout_secs: DEFAULT_PING_TIMEOUT,
            on_timeout: None,
            on_latency: None,
        }
    }

    pub fn with_interval(mut self, secs: u64) -> Self {
        self.interval_secs = secs;
        self
    }

    pub fn with_timeout(mut self, secs: u64) -> Self {
        self.timeout_secs = secs;
        self
    }

    pub fn on_timeout<F>(mut self, callback: F) -> Self
    where
        F: Fn(SocketAddr) + Send + Sync + 'static,
    {
        self.on_timeout = Some(Arc::new(callback));
        self
    }

    pub fn on_latency<F>(mut self, callback: F) -> Self
    where
        F: Fn(SocketAddr, u64) + Send + Sync + 'static,
    {
        self.on_latency = Some(Arc::new(callback));
        self
    }
}

impl Default for HeartbeatConfig {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone)]
pub struct HeartbeatManager {
    pub local_node: Node,
    pub config: HeartbeatConfig,
    pub session_keys: Option<Arc<Mutex<PairedSessionKey>>>,
    active_connections: Arc<tokio::sync::RwLock<std::collections::HashMap<SocketAddr, HeartbeatState>>>,
}

struct HeartbeatState {
    last_ping: u64,
    last_pong: u64,
    latency_ns: u64,
    latency_avg: u64,
    missed_pings: u32,
    abort_handle: Option<AbortHandle>,
}

impl HeartbeatManager {
    pub fn new(local_node: Node) -> Self {
        Self {
            local_node,
            config: HeartbeatConfig::new(),
            session_keys: None,
            active_connections: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
        }
    }

    pub fn new_with_arc(local_node: Node, active: Arc<tokio::sync::RwLock<std::collections::HashMap<SocketAddr, HeartbeatState>>>) -> Self {
        Self {
            local_node,
            config: HeartbeatConfig::new(),
            session_keys: None,
            active_connections: active,
        }
    }

    pub fn with_config(mut self, config: HeartbeatConfig) -> Self {
        self.config = config;
        self
    }

    pub fn with_session_keys(mut self, keys: Arc<Mutex<PairedSessionKey>>) -> Self {
        self.session_keys = Some(keys);
        self
    }

    pub fn create_ping(&self) -> PingCommand {
        if self.session_keys.is_some() {
            PingCommand::with_nonce(vec![0u8; 8])
        } else {
            PingCommand::new()
        }
    }

    pub fn create_pong(&self, ping: &PingCommand) -> PongCommand {
        PongCommand::new(ping.timestamp, ping.nonce.clone())
    }

    pub async fn start_server_heartbeat(
        &self,
        ctx: Arc<Mutex<Context>>,
        peer_addr: SocketAddr,
        cancel_token: CancellationToken,
    ) {
        let local_node = self.local_node.clone();
        let config = self.config.clone();
        let active = self.active_connections.clone();
        
        let mut interval = tokio::time::interval(Duration::from_secs(config.interval_secs));
        
        let ping = PingCommand::new();
        
        let state = HeartbeatState {
            last_ping: ping.timestamp,
            last_pong: 0,
            latency_ns: 0,
            latency_avg: 0,
            missed_pings: 0,
            abort_handle: None,
        };
        
        active.write().await.insert(peer_addr, state);
        
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = cancel_token.cancelled() => {
                        active.write().await.remove(&peer_addr);
                        break;
                    }
                    _ = interval.tick() => {
                        let result = Self::send_ping_internal(&local_node, &ctx, ping.clone()).await;
                        
                        if let Some(state) = active.write().await.get_mut(&peer_addr) {
                            if result.is_ok() {
                                state.last_ping = ping.timestamp;
                                state.missed_pings = 0;
                            } else {
                                state.missed_pings += 1;
                            }
                        }
                    }
                }
            }
            active.write().await.remove(&peer_addr);
        });
    }

    async fn send_ping_internal(local_node: &Node, ctx: &Arc<Mutex<Context>>, ping: PingCommand) -> Result<()> {
        let data = ping.encode();
        let mut guard = ctx.lock().await;
        let writer = guard.writer.as_mut().ok_or_else(|| anyhow::anyhow!("no writer"))?;
        writer.write_all(&(data.len() as u32).to_le_bytes()).await?;
        writer.write_all(&data).await?;
        Ok(())
    }

    #[allow(unused_variables)]
    pub async fn handle_ping(
        &self,
        ctx: Arc<Mutex<Context>>,
        data: &[u8],
        peer_addr: SocketAddr,
    ) -> Result<()> {
        let ping = PingCommand::decode(data).map_err(anyhow::Error::msg)?;
        
        let pong = self.create_pong(&ping);
        let pong_data = pong.encode();
        
        let mut guard = ctx.lock().await;
        let writer = guard.writer.as_mut().ok_or_else(|| anyhow::anyhow!("no writer"))?;
        writer.write_all(&(pong_data.len() as u32).to_le_bytes()).await?;
        writer.write_all(&pong_data).await?;
        
        Ok(())
    }

    pub async fn handle_pong(
        &self,
        data: &[u8],
        peer_addr: SocketAddr,
    ) -> Result<u64> {
        let pong = PongCommand::decode(data).map_err(anyhow::Error::msg)?;
        let latency = pong.latency();
        
        if let Some(state) = self.active_connections.write().await.get_mut(&peer_addr) {
            state.last_pong = pong.local_time;
            state.latency_ns = latency * 1000;
            
            let old_avg = state.latency_avg;
            state.latency_avg = (old_avg + state.latency_ns) / 2;
            
            if let Some(callback) = &self.config.on_latency {
                callback(peer_addr, state.latency_avg);
            }
        }
        
        Ok(latency)
    }

    pub async fn check_timeout(&self, peer_addr: SocketAddr) -> bool {
        if let Some(state) = self.active_connections.read().await.get(&peer_addr) {
            if state.missed_pings >= 2 {
                if let Some(callback) = &self.config.on_timeout {
                    callback(peer_addr);
                }
                return true;
            }
        }
        false
    }

    pub async fn remove_connection(&self, peer_addr: &SocketAddr) {
        self.active_connections.write().await.remove(peer_addr);
    }

    pub async fn get_latency(&self, peer_addr: SocketAddr) -> Option<u64> {
        self.active_connections
            .read()
            .await
            .get(&peer_addr)
            .map(|s| s.latency_avg)
    }

    pub async fn set_connection_state(&self, peer_addr: SocketAddr, missed: u32, latency: u64) {
        let mut active = self.active_connections.write().await;
        active.insert(peer_addr, HeartbeatState {
            last_ping: 0,
            last_pong: 0,
            latency_ns: latency,
            latency_avg: latency,
            missed_pings: missed,
            abort_handle: None,
        });
    }
}