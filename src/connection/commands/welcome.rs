use serde::{Deserialize, Serialize};

use super::command_id::CommandId;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WelcomeCommand {
    pub version: u8,
    pub node: crate::connection::node::Node,
    pub accepted: bool,
    pub ephemeral_public: Option<Vec<u8>>,
}

impl WelcomeCommand {
    pub fn new(
        node: crate::connection::node::Node,
        accepted: bool,
        ephemeral_public: Option<Vec<u8>>,
    ) -> Self {
        Self {
            version: 1,
            node,
            accepted,
            ephemeral_public,
        }
    }

    pub fn rejected() -> Self {
        Self {
            version: 1,
            node: crate::connection::node::Node::from_addr(
                "0.0.0.0:0".parse().unwrap(),
                None,
                None,
            ),
            accepted: false,
            ephemeral_public: None,
        }
    }

    pub fn id() -> CommandId {
        CommandId::Welcome
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut bytes = vec![];
        bytes.extend_from_slice(&(CommandId::Welcome.as_u32()).to_le_bytes());
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
        if id != CommandId::Welcome.as_u32() {
            return Err("invalid command id".to_string());
        }
        serde_json::from_slice(&data[4..]).map_err(|e| e.to_string())
    }

    pub fn is_valid(&self) -> bool {
        self.version == 1
    }
}
