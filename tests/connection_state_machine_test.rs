use aex::connection::state_machine::{ConnectionState, ConnectionStateMachine};

#[test]
fn test_connection_state_all_variants() {
    assert_eq!(ConnectionState::from_u8(0), ConnectionState::Initial);
    assert_eq!(ConnectionState::from_u8(1), ConnectionState::Connecting);
    assert_eq!(ConnectionState::from_u8(2), ConnectionState::Handshake);
    assert_eq!(ConnectionState::from_u8(3), ConnectionState::Established);
    assert_eq!(ConnectionState::from_u8(4), ConnectionState::Active);
    assert_eq!(ConnectionState::from_u8(5), ConnectionState::Reconnecting);
    assert_eq!(ConnectionState::from_u8(6), ConnectionState::Disconnecting);
    assert_eq!(ConnectionState::from_u8(7), ConnectionState::Disconnected);
}

#[test]
fn test_connection_state_unknown() {
    assert_eq!(ConnectionState::from_u8(100), ConnectionState::Initial);
}

#[test]
fn test_connection_state_as_u8() {
    assert_eq!(ConnectionState::Initial.as_u8(), 0);
    assert_eq!(ConnectionState::Connecting.as_u8(), 1);
    assert_eq!(ConnectionState::Handshake.as_u8(), 2);
    assert_eq!(ConnectionState::Established.as_u8(), 3);
    assert_eq!(ConnectionState::Active.as_u8(), 4);
    assert_eq!(ConnectionState::Reconnecting.as_u8(), 5);
    assert_eq!(ConnectionState::Disconnecting.as_u8(), 6);
    assert_eq!(ConnectionState::Disconnected.as_u8(), 7);
}

#[test]
fn test_connection_state_is_connected() {
    assert!(!ConnectionState::Initial.is_connected());
    assert!(!ConnectionState::Connecting.is_connected());
    assert!(!ConnectionState::Handshake.is_connected());
    assert!(ConnectionState::Established.is_connected());
    assert!(ConnectionState::Active.is_connected());
    assert!(!ConnectionState::Reconnecting.is_connected());
    assert!(!ConnectionState::Disconnecting.is_connected());
    assert!(!ConnectionState::Disconnected.is_connected());
}

#[test]
fn test_connection_state_can_send() {
    assert!(ConnectionState::Established.can_send());
    assert!(ConnectionState::Active.can_send());
    assert!(!ConnectionState::Initial.can_send());
}

#[test]
fn test_connection_state_can_receive() {
    assert!(ConnectionState::Established.can_receive());
    assert!(ConnectionState::Active.can_receive());
    assert!(!ConnectionState::Initial.can_receive());
}

#[test]
fn test_connection_state_should_heartbeat() {
    assert!(ConnectionState::Active.should_heartbeat());
    assert!(!ConnectionState::Initial.should_heartbeat());
}

#[test]
fn test_connection_state_display() {
    assert_eq!(format!("{}", ConnectionState::Initial), "Initial");
    assert_eq!(format!("{}", ConnectionState::Active), "Active");
}

#[test]
fn test_connection_state_machine_new() {
    let sm = ConnectionStateMachine::new();
    assert_eq!(sm.current(), ConnectionState::Initial);
}

#[test]
fn test_connection_state_machine_set_valid() {
    let sm = ConnectionStateMachine::new();
    assert!(sm.set(ConnectionState::Connecting));
    assert_eq!(sm.current(), ConnectionState::Connecting);
}

#[test]
fn test_connection_state_machine_set_invalid() {
    let sm = ConnectionStateMachine::new();
    sm.set(ConnectionState::Connecting);
    assert!(!sm.set(ConnectionState::Initial));
}

#[test]
fn test_connection_state_machine_transitions() {
    let sm = ConnectionStateMachine::new();

    sm.set(ConnectionState::Connecting);
    sm.set(ConnectionState::Handshake);
    sm.set(ConnectionState::Established);
    sm.set(ConnectionState::Active);

    assert_eq!(sm.current(), ConnectionState::Active);
}
