use serde::{Deserialize, Serialize};

pub const CMD_REJECT: u32 = 0xFF;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RejectCommand {
    pub reason: String,
}

impl RejectCommand {
    pub fn new(reason: &str) -> Self {
        Self {
            reason: reason.to_string(),
        }
    }

    pub fn id() -> u32 {
        CMD_REJECT
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut bytes = vec![];
        bytes.extend_from_slice(&(CMD_REJECT as u32).to_le_bytes());
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
        if id != CMD_REJECT {
            return Err("invalid command id".to_string());
        }
        serde_json::from_slice(&data[4..]).map_err(|e| e.to_string())
    }
}
