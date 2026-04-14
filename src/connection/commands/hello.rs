use serde::{Deserialize, Serialize};

pub const CMD_HELLO: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HelloCommand {
    pub version: u8,
    pub node: crate::connection::node::Node,
    pub ephemeral_public: Option<Vec<u8>>,
    pub request_encryption: bool,
}

impl HelloCommand {
    pub fn new(
        node: crate::connection::node::Node,
        ephemeral_public: Option<Vec<u8>>,
        request_encryption: bool,
    ) -> Self {
        Self {
            version: 1,
            node,
            ephemeral_public,
            request_encryption,
        }
    }

    pub fn id() -> u32 {
        CMD_HELLO
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut bytes = vec![];
        bytes.extend_from_slice(&(CMD_HELLO as u32).to_le_bytes());
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
        if id != CMD_HELLO {
            return Err("invalid command id".to_string());
        }
        serde_json::from_slice(&data[4..]).map_err(|e| e.to_string())
    }

    pub fn is_valid(&self) -> bool {
        self.version == 1
    }
}
