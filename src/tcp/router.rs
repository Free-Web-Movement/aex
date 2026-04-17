use std::collections::HashMap;
use std::sync::Arc;

use crate::connection::context::Context;
use crate::connection::global::GlobalContext;
use tokio::io::AsyncReadExt;
use tokio::sync::Mutex;

pub trait Command: Send + Sync + 'static {
    fn id(&self) -> u32;
    fn data(&self) -> &[u8];
}

pub struct TcpRouter {
    pub handlers: HashMap<u32, Box<dyn Fn(Arc<Mutex<Context>>, &[u8]) -> bool + Send + Sync>>,
}

impl TcpRouter {
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
        }
    }

    pub fn on<C: Command>(&mut self, key: u32, handler: impl Fn(Arc<Mutex<Context>>, &C) -> bool + Send + Sync + 'static) {
        self.handlers.insert(key, Box::new(move |ctx, data| {
            let cmd = parse_command::<C>(data);
            if let Some(cmd) = cmd {
                return handler(ctx, &cmd);
            }
            true
        }));
    }

    pub async fn handle(&self, ctx: Arc<Mutex<Context>>) -> std::io::Result<()> {
        let mut buf = vec![0u8; 4096];
        let mut session = Vec::new();
        
        loop {
            let n = {
                let mut guard = ctx.lock().await;
                if let Some(ref mut r) = guard.reader {
                    r.read(&mut buf).await?
                } else {
                    break;
                }
            };
            
            if n == 0 { break; }
            session.extend_from_slice(&buf[..n]);
            
            while session.len() >= 4 {
                let len = u32::from_le_bytes(session[..4].try_into().unwrap()) as usize;
                if session.len() >= 4 + len {
                    let frame = &session[4..4 + len];
                    for handler in self.handlers.values() {
                        if !handler(ctx.clone(), frame) {
                            return Ok(());
                        }
                    }
                    session.drain(..4 + len);
                } else {
                    break;
                }
            }
        }
        Ok(())
    }
}

fn parse_command<C: Command>(data: &[u8]) -> Option<C> {
    None
}

impl Default for TcpRouter {
    fn default() -> Self {
        Self::new()
    }
}

pub struct UdpRouter {
    pub handlers: HashMap<u32, Box<dyn Fn(Arc<GlobalContext>, &[u8], std::net::SocketAddr) -> bool + Send + Sync>>,
}

impl UdpRouter {
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
        }
    }

    pub fn on<C: Command>(&mut self, key: u32, handler: impl Fn(Arc<GlobalContext>, &C, std::net::SocketAddr) -> bool + Send + Sync + 'static) {
        self.handlers.insert(key, Box::new(move |global, data, addr| {
            let cmd = parse_command::<C>(data);
            if let Some(cmd) = cmd {
                return handler(global, &cmd, addr);
            }
            true
        }));
    }

    pub async fn handle(self: Arc<Self>, global: Arc<GlobalContext>, socket: std::sync::Arc<tokio::net::UdpSocket>) {
        let mut buf = [0u8; 65535];
        loop {
            if let Ok((n, addr)) = socket.recv_from(&mut buf).await {
                let data = &buf[..n];
                for handler in self.handlers.values() {
                    if !handler(global.clone(), data, addr) {
                        break;
                    }
                }
            }
        }
    }
}

impl Default for UdpRouter {
    fn default() -> Self {
        Self::new()
    }
}
