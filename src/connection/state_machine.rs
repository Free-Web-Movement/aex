use std::fmt;
use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    Initial = 0,
    Connecting = 1,
    Handshake = 2,
    Established = 3,
    Active = 4,
    Reconnecting = 5,
    Disconnecting = 6,
    Disconnected = 7,
}

impl ConnectionState {
    pub fn from_u8(v: u8) -> Self {
        match v {
            0 => ConnectionState::Initial,
            1 => ConnectionState::Connecting,
            2 => ConnectionState::Handshake,
            3 => ConnectionState::Established,
            4 => ConnectionState::Active,
            5 => ConnectionState::Reconnecting,
            6 => ConnectionState::Disconnecting,
            7 => ConnectionState::Disconnected,
            _ => ConnectionState::Initial,
        }
    }

    pub fn as_u8(self) -> u8 {
        self as u8
    }

    pub fn is_connected(self) -> bool {
        matches!(self, ConnectionState::Established | ConnectionState::Active)
    }

    pub fn can_send(self) -> bool {
        matches!(self, ConnectionState::Established | ConnectionState::Active)
    }

    pub fn can_receive(self) -> bool {
        matches!(self, ConnectionState::Established | ConnectionState::Active)
    }

    pub fn should_heartbeat(self) -> bool {
        matches!(self, ConnectionState::Active)
    }
}

impl fmt::Display for ConnectionState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConnectionState::Initial => write!(f, "Initial"),
            ConnectionState::Connecting => write!(f, "Connecting"),
            ConnectionState::Handshake => write!(f, "Handshake"),
            ConnectionState::Established => write!(f, "Established"),
            ConnectionState::Active => write!(f, "Active"),
            ConnectionState::Reconnecting => write!(f, "Reconnecting"),
            ConnectionState::Disconnecting => write!(f, "Disconnecting"),
            ConnectionState::Disconnected => write!(f, "Disconnected"),
        }
    }
}

pub struct ConnectionStateMachine {
    state: AtomicU8,
}

impl ConnectionStateMachine {
    pub fn new() -> Self {
        Self {
            state: AtomicU8::new(ConnectionState::Initial.as_u8()),
        }
    }

    pub fn current(&self) -> ConnectionState {
        ConnectionState::from_u8(self.state.load(Ordering::SeqCst))
    }

    pub fn set(&self, state: ConnectionState) -> bool {
        let current = self.current();
        if Self::can_transition(current, state) {
            self.state.store(state.as_u8(), Ordering::SeqCst);
            true
        } else {
            false
        }
    }

    pub fn transition(&self, to: ConnectionState) -> bool {
        self.set(to)
    }

    fn can_transition(from: ConnectionState, to: ConnectionState) -> bool {
        match (from, to) {
            (ConnectionState::Initial, ConnectionState::Connecting) => true,
            (ConnectionState::Connecting, ConnectionState::Handshake) => true,
            (ConnectionState::Handshake, ConnectionState::Established) => true,
            (ConnectionState::Handshake, ConnectionState::Disconnecting) => true,
            (ConnectionState::Established, ConnectionState::Active) => true,
            (ConnectionState::Established, ConnectionState::Disconnecting) => true,
            (ConnectionState::Active, ConnectionState::Reconnecting) => true,
            (ConnectionState::Active, ConnectionState::Disconnecting) => true,
            (ConnectionState::Reconnecting, ConnectionState::Connecting) => true,
            (ConnectionState::Reconnecting, ConnectionState::Established) => true,
            (ConnectionState::Reconnecting, ConnectionState::Disconnected) => true,
            (ConnectionState::Disconnecting, ConnectionState::Disconnected) => true,
            (ConnectionState::Disconnected, ConnectionState::Connecting) => true,
            _ => false,
        }
    }

    pub fn is_connected(&self) -> bool {
        self.current().is_connected()
    }

    pub fn is_active(&self) -> bool {
        self.current() == ConnectionState::Active
    }

    pub fn should_heartbeat(&self) -> bool {
        self.current().should_heartbeat()
    }
}

impl Default for ConnectionStateMachine {
    fn default() -> Self {
        Self::new()
    }
}
