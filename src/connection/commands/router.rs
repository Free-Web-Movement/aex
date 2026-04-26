use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::{Result, anyhow};
use async_lock::Mutex;

use crate::connection::commands::CommandId;
use crate::connection::context::Context;

pub type CommandHandler =
    Box<dyn Fn(Arc<Mutex<Context>>, &[u8], SocketAddr) -> Result<()> + Send + Sync + 'static>;

pub struct CommandRouter {
    handlers: std::collections::HashMap<CommandId, CommandHandler>,
}

impl CommandRouter {
    pub fn new() -> Self {
        Self {
            handlers: std::collections::HashMap::new(),
        }
    }

    pub fn register<F>(&mut self, id: CommandId, handler: F) -> &mut Self
    where
        F: Fn(Arc<Mutex<Context>>, &[u8], SocketAddr) -> Result<()> + Send + Sync + 'static,
    {
        self.handlers.insert(id, Box::new(handler));
        self
    }

    pub fn dispatch(&self, ctx: Arc<Mutex<Context>>, data: &[u8], addr: SocketAddr) -> Result<()> {
        if data.len() < 4 {
            return Err(anyhow!("data too short"));
        }
        let cmd_id = CommandId::from_u32(u32::from_le_bytes(data[0..4].try_into().unwrap()))
            .ok_or_else(|| anyhow!("unknown command id"))?;

        if let Some(handler) = self.handlers.get(&cmd_id) {
            handler(ctx, &data[4..], addr)?;
        }
        Ok(())
    }
}

impl Default for CommandRouter {
    fn default() -> Self {
        Self::new()
    }
}
