//! # P2P Handshake Commands
//!
//!握手消息命令，每个命令独立一个文件

pub mod ack;
pub mod command_id;
pub mod hello;
pub mod ping;
pub mod reject;
pub mod router;
pub mod welcome;

pub use ack::AckCommand;
pub use command_id::CommandId;
pub use hello::HelloCommand;
pub use ping::{PingCommand, PongCommand};
pub use reject::RejectCommand;
pub use router::CommandRouter;
pub use welcome::WelcomeCommand;

pub fn detect_command(data: &[u8]) -> Option<CommandId> {
    if data.len() < 4 {
        return None;
    }
    let id = u32::from_le_bytes(data[0..4].try_into().unwrap());
    CommandId::from_u32(id)
}
