use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

use crate::connection::commands::CommandId;
use crate::constants::tcp::{MAX_FRAME_SIZE, PROTOCOL_HEADER_SIZE};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProtocolFlags(u8);

impl ProtocolFlags {
    pub const NONE: ProtocolFlags = ProtocolFlags(0b0000_0000);
    pub const COMPRESSED: ProtocolFlags = ProtocolFlags(0b0000_0001);
    pub const ENCRYPTED: ProtocolFlags = ProtocolFlags(0b0000_0010);
    pub const PRIORITY: ProtocolFlags = ProtocolFlags(0b0000_0100);
    pub const FRAGMENT: ProtocolFlags = ProtocolFlags(0b0000_1000);

    pub fn has_compressed(self) -> bool {
        self.0 & Self::COMPRESSED.0 != 0
    }

    pub fn has_encrypted(self) -> bool {
        self.0 & Self::ENCRYPTED.0 != 0
    }

    pub fn has_priority(self) -> bool {
        self.0 & Self::PRIORITY.0 != 0
    }

    pub fn has_fragment(self) -> bool {
        self.0 & Self::FRAGMENT.0 != 0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrameHeader {
    pub command_id: u32,
    pub flags: u8,
    pub sequence: u32,
    pub payload_length: u32,
}

impl FrameHeader {
    pub fn new(command_id: CommandId, payload_length: u32) -> Self {
        Self {
            command_id: command_id.as_u32(),
            flags: 0,
            sequence: 0,
            payload_length,
        }
    }

    pub fn with_flags(mut self, flags: ProtocolFlags) -> Self {
        self.flags = flags.0;
        self
    }

    pub fn with_sequence(mut self, sequence: u32) -> Self {
        self.sequence = sequence;
        self
    }

    pub fn command(&self) -> Option<CommandId> {
        CommandId::from_u32(self.command_id)
    }

    pub fn flags(&self) -> ProtocolFlags {
        ProtocolFlags(self.flags)
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut bytes = vec![0u8; PROTOCOL_HEADER_SIZE];
        bytes[0..4].copy_from_slice(&self.command_id.to_le_bytes());
        bytes[4] = self.flags;
        bytes[5..8].copy_from_slice(&self.sequence.to_le_bytes()[..3]);
        bytes
    }

    pub fn decode(data: &[u8]) -> Result<Self> {
        if data.len() < PROTOCOL_HEADER_SIZE {
            return Err(anyhow!("frame header too short"));
        }
        let command_id = u32::from_le_bytes(data[0..4].try_into().unwrap());
        let flags = data[4];
        let sequence = u32::from_le_bytes([data[5], data[6], data[7], 0]);
        let payload_length = 0;
        Ok(Self {
            command_id,
            flags,
            sequence,
            payload_length,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolFrame {
    pub header: FrameHeader,
    pub payload: Vec<u8>,
}

impl ProtocolFrame {
    pub fn new(command_id: CommandId, payload: Vec<u8>) -> Self {
        let header = FrameHeader::new(command_id, payload.len() as u32);
        Self { header, payload }
    }

    pub fn command_id(&self) -> Option<CommandId> {
        self.header.command()
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut bytes = self.header.encode();
        bytes.extend_from_slice(&self.payload);
        bytes
    }

    pub fn decode(data: &[u8]) -> Result<Self> {
        if data.len() < PROTOCOL_HEADER_SIZE {
            return Err(anyhow!("frame too short"));
        }
        let header = FrameHeader::decode(data)?;
        let payload_length = header.payload_length as usize;
        if data.len() < PROTOCOL_HEADER_SIZE + payload_length {
            return Err(anyhow!("incomplete payload"));
        }
        let payload = data[PROTOCOL_HEADER_SIZE..PROTOCOL_HEADER_SIZE + payload_length].to_vec();
        Ok(Self { header, payload })
    }

    pub fn encode_with_length(&self) -> Vec<u8> {
        let mut frame = self.encode();
        let mut result = vec![0u8; 4];
        result.extend_from_slice(&(frame.len() as u32).to_le_bytes());
        result.extend_from_slice(&frame);
        result
    }

    pub fn decode_with_length(data: &[u8]) -> Result<Self> {
        if data.len() < 4 {
            return Err(anyhow!("data too short for length"));
        }
        let length = u32::from_le_bytes(data[0..4].try_into().unwrap()) as usize;
        if data.len() < 4 + length {
            return Err(anyhow!("incomplete frame"));
        }
        Self::decode(&data[4..4 + length])
    }
}

pub struct ProtocolCodec {
    sequence: u32,
}

impl ProtocolCodec {
    pub fn new() -> Self {
        Self { sequence: 0 }
    }

    pub fn next_sequence(&mut self) -> u32 {
        self.sequence = self.sequence.wrapping_add(1);
        self.sequence
    }

    pub fn encode(&self, command_id: CommandId, payload: &[u8]) -> Vec<u8> {
        let frame = ProtocolFrame::new(command_id, payload.to_vec());
        frame.encode_with_length()
    }

    pub fn decode(&self, data: &[u8]) -> Result<ProtocolFrame> {
        ProtocolFrame::decode_with_length(data)
    }
}

impl Default for ProtocolCodec {
    fn default() -> Self {
        Self::new()
    }
}
