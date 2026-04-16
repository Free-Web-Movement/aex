use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::{Ok, Result};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Mutex;

use crate::constants::tcp::MAX_HANDSHAKE_SIZE;
use crate::connection::commands::{
    HelloCommand, WelcomeCommand, AckCommand, RejectCommand, CommandId,
};
use crate::connection::context::Context;
use crate::connection::node::Node;
use crate::crypto::session_key_manager::PairedSessionKey;

pub struct HandshakeHandler {
    pub local_node: Node,
    pub session_keys: Option<Arc<Mutex<PairedSessionKey>>>,
    pub on_established: Option<Arc<dyn Fn(Node, SocketAddr) + Send + Sync>>,
    pub on_rejected: Option<Arc<dyn Fn(String, SocketAddr) + Send + Sync>>,
}

impl HandshakeHandler {
    pub fn new(local_node: Node) -> Self {
        Self {
            local_node,
            session_keys: None,
            on_established: None,
            on_rejected: None,
        }
    }

    #[cfg(test)]
    pub fn new_with_node(node: Node) -> Self {
        Self {
            local_node: node,
            session_keys: None,
            on_established: None,
            on_rejected: None,
        }
    }

    #[cfg(test)]
    pub fn local_node_ref(&self) -> &Node {
        &self.local_node
    }

    pub fn with_session_keys(mut self, keys: Arc<Mutex<PairedSessionKey>>) -> Self {
        self.session_keys = Some(keys);
        self
    }

    pub fn on_established<F>(mut self, callback: F) -> Self
    where
        F: Fn(Node, SocketAddr) + Send + Sync + 'static,
    {
        self.on_established = Some(Arc::new(callback));
        self
    }

    pub fn on_rejected<F>(mut self, callback: F) -> Self
    where
        F: Fn(String, SocketAddr) + Send + Sync + 'static,
    {
        self.on_rejected = Some(Arc::new(callback));
        self
    }

    pub fn create_hello(&self, request_encryption: bool) -> HelloCommand {
        let ephemeral_public = if request_encryption && self.session_keys.is_some() {
            Some(vec![0u8; 32])
        } else {
            None
        };
        
        HelloCommand::new(
            self.local_node.clone(),
            ephemeral_public,
            request_encryption,
        )
    }

    pub fn create_welcome(&self, accepted: bool, ephemeral_public: Option<Vec<u8>>) -> WelcomeCommand {
        WelcomeCommand::new(
            self.local_node.clone(),
            accepted,
            ephemeral_public,
        )
    }

    pub fn create_ack(&self, session_key_id: Option<Vec<u8>>) -> AckCommand {
        AckCommand::accepted(session_key_id)
    }

    pub fn create_reject(&self, reason: &str) -> RejectCommand {
        RejectCommand::new(reason)
    }

    pub async fn handle_server_side(
        &self,
        ctx: Arc<Mutex<Context>>,
        peer_addr: SocketAddr,
    ) -> Result<Option<Node>> {
        {
            let mut guard = ctx.lock().await;
            let reader = guard.reader.as_mut().ok_or_else(|| anyhow::anyhow!("no reader"))?;
            let mut length_buf = [0u8; 4];
            reader.read_exact(&mut length_buf).await?;
            let len = u32::from_le_bytes(length_buf) as usize;
            if len > MAX_HANDSHAKE_SIZE {
                return Err(anyhow::anyhow!("handshake message too large"));
            }
            let mut data = vec![0u8; len];
            reader.read_exact(&mut data).await?;
            
            let id = u32::from_le_bytes(data[0..4].try_into().unwrap());
            
            match CommandId::from_u32(id) {
                Some(CommandId::Hello) => {
                    let hello = HelloCommand::decode(&data).map_err(|e| anyhow::anyhow!(e))?;
                    if !hello.is_valid() {
                        let reject = self.create_reject("version mismatch");
                        self.send_frame(ctx.clone(), reject.encode()).await?;
                        return Err(anyhow::anyhow!("version mismatch"));
                    }
                    
                    if let Some(callback) = &self.on_established {
                        callback(hello.node.clone(), peer_addr);
                    }
                    
                    let ephemeral_public = if hello.request_encryption && self.session_keys.is_some() {
                        Some(vec![0u8; 32])
                    } else {
                        None
                    };
                    
                    let welcome = self.create_welcome(true, ephemeral_public);
                    self.send_frame(ctx.clone(), welcome.encode()).await?;
                    
                    return Ok(Some(hello.node));
                }
                Some(CommandId::Reject) => {
                    let reject = RejectCommand::decode(&data).map_err(|e| anyhow::anyhow!(e))?;
                    if let Some(callback) = &self.on_rejected {
                        callback(reject.reason.clone(), peer_addr);
                    }
                    return Err(anyhow::anyhow!("rejected: {}", reject.reason));
                }
                _ => {
                    return Err(anyhow::anyhow!("expected Hello"));
                }
            }
        }
    }

    async fn send_frame(&self, ctx: Arc<Mutex<Context>>, data: Vec<u8>) -> Result<()> {
        let mut guard = ctx.lock().await;
        let writer = guard.writer.as_mut().ok_or_else(|| anyhow::anyhow!("no writer"))?;
        writer.write_all(&(data.len() as u32).to_le_bytes()).await?;
        writer.write_all(&data).await?;
        Ok(())
    }

    #[cfg(test)]
    pub async fn send_frame_test(ctx: Arc<Mutex<Context>>, data: Vec<u8>) -> Result<()> {
        Self::send_frame_internal(&ctx, data).await
    }

    async fn send_frame_internal(ctx: &Arc<Mutex<Context>>, data: Vec<u8>) -> Result<()> {
        let mut guard = ctx.lock().await;
        let writer = guard.writer.as_mut().ok_or_else(|| anyhow::anyhow!("no writer"))?;
        writer.write_all(&(data.len() as u32).to_le_bytes()).await?;
        writer.write_all(&data).await?;
        Ok(())
    }

    pub async fn handshake_as_client(
        &self,
        peer_addr: SocketAddr,
        request_encryption: bool,
    ) -> Result<Node> {
        let socket = tokio::net::TcpStream::connect(peer_addr).await?;
        let mut socket = socket;

        let hello = self.create_hello(request_encryption);
        let data = hello.encode();

        socket.write_all(&(data.len() as u32).to_le_bytes()).await?;
        socket.write_all(&data).await?;
        socket.flush().await?;

        let mut length_buf = [0u8; 4];
        socket.read_exact(&mut length_buf).await?;

        let len = u32::from_le_bytes(length_buf) as usize;
        let mut data = vec![0u8; len];
        socket.read_exact(&mut data).await?;

        let id = u32::from_le_bytes(data[0..4].try_into().unwrap());

        match CommandId::from_u32(id) {
            Some(CommandId::Welcome) => {
                let welcome = WelcomeCommand::decode(&data).map_err(|e| anyhow::anyhow!(e))?;
                if !welcome.accepted {
                    return Err(anyhow::anyhow!("connection rejected"));
                }

                if let Some(callback) = &self.on_established {
                    callback(welcome.node.clone(), peer_addr);
                }

                Ok(welcome.node)
            }
            Some(CommandId::Reject) => {
                let reject = RejectCommand::decode(&data).map_err(|e| anyhow::anyhow!(e))?;
                Err(anyhow::anyhow!("rejected: {}", reject.reason))
            }
            _ => Err(anyhow::anyhow!("unexpected message")),
        }
    }
}