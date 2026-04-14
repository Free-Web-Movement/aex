use serde::{Deserialize, Serialize};

use super::command_id::CommandId;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AckCommand {
    pub accepted: bool,
    pub session_key_id: Option<Vec<u8>>,
}

impl AckCommand {
    pub fn accepted(session_key_id: Option<Vec<u8>>) -> Self {
        Self {
            accepted: true,
            session_key_id,
        }
    }

    pub fn rejected() -> Self {
        Self {
            accepted: false,
            session_key_id: None,
        }
    }

    pub fn id() -> CommandId {
        CommandId::Ack
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut bytes = vec![];
        bytes.extend_from_slice(&(CommandId::Ack.as_u32()).to_le_bytes());
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
        if id != CommandId::Ack.as_u32() {
            return Err("invalid command id".to_string());
        }
        serde_json::from_slice(&data[4..]).map_err(|e| e.to_string())
    }
}
