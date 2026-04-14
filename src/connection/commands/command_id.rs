#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CommandId {
    Hello = 1,
    Welcome = 2,
    Ack = 3,
    Reject = 4,
    Ping = 5,
    Pong = 6,
}

impl CommandId {
    pub fn from_u32(v: u32) -> Option<Self> {
        match v {
            1 => Some(Self::Hello),
            2 => Some(Self::Welcome),
            3 => Some(Self::Ack),
            4 => Some(Self::Reject),
            5 => Some(Self::Ping),
            6 => Some(Self::Pong),
            _ => None,
        }
    }

    pub fn as_u32(self) -> u32 {
        self as u32
    }
}

impl From<CommandId> for u32 {
    fn from(id: CommandId) -> u32 {
        id as u32
    }
}
