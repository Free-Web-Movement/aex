use serde::{Deserialize, Serialize};

use super::command_id::CommandId;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PingCommand {
    pub timestamp: u64,
    pub nonce: Option<Vec<u8>>,
}

impl PingCommand {
    pub fn new() -> Self {
        Self {
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            nonce: None,
        }
    }

    pub fn with_nonce(nonce: Vec<u8>) -> Self {
        Self {
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            nonce: Some(nonce),
        }
    }

    pub fn id() -> CommandId {
        CommandId::Ping
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut bytes = vec![];
        bytes.extend_from_slice(&(CommandId::Ping.as_u32()).to_le_bytes());
        if let Ok(v) = serde_json::to_vec(self) {
            bytes.extend_from_slice(&v);
        }
        bytes
    }

    pub fn decode(data: &[u8]) -> Result<Self, String> {
        if data.len() < 4 {
            return Err("data too short".to_string());
        }
        let id = u32::from_le_bytes(data[0..4].try_into().unwrap());
        if id != CommandId::Ping.as_u32() {
            return Err("invalid command id".to_string());
        }
        serde_json::from_slice(&data[4..]).map_err(|e| e.to_string())
    }
}

impl Default for PingCommand {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PongCommand {
    pub timestamp: u64,
    pub nonce: Option<Vec<u8>>,
    pub local_time: u64,
}

impl PongCommand {
    pub fn new(timestamp: u64, nonce: Option<Vec<u8>>) -> Self {
        Self {
            timestamp,
            nonce,
            local_time: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        }
    }

    pub fn id() -> CommandId {
        CommandId::Pong
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut bytes = vec![];
        bytes.extend_from_slice(&(CommandId::Pong.as_u32()).to_le_bytes());
        if let Ok(v) = serde_json::to_vec(self) {
            bytes.extend_from_slice(&v);
        }
        bytes
    }

    pub fn decode(data: &[u8]) -> Result<Self, String> {
        if data.len() < 4 {
            return Err("data too short".to_string());
        }
        let id = u32::from_le_bytes(data[0..4].try_into().unwrap());
        if id != CommandId::Pong.as_u32() {
            return Err("invalid command id".to_string());
        }
        serde_json::from_slice(&data[4..]).map_err(|e| e.to_string())
    }

    pub fn latency(&self) -> u64 {
        self.local_time.saturating_sub(self.timestamp)
    }
}

pub fn detect_command_type(data: &[u8]) -> Option<CommandId> {
    if data.len() < 4 {
        return None;
    }
    let id = u32::from_le_bytes(data[0..4].try_into().unwrap());
    CommandId::from_u32(id)
}
