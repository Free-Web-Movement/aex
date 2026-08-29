use aex::connection::state_machine::{ConnectionState, ConnectionStateMachine};
use ConnectionState::*;

/// 从 Initial 沿合法迁移链走到 target 状态
fn enter(sm: &ConnectionStateMachine, target: ConnectionState) {
    let path: &[ConnectionState] = match target {
        Connecting => &[Connecting],
        Handshake => &[Connecting, Handshake],
        Established => &[Connecting, Handshake, Established],
        Active => &[Connecting, Handshake, Established, Active],
        Reconnecting => &[Connecting, Handshake, Established, Active, Reconnecting],
        Disconnecting => &[Connecting, Handshake, Disconnecting],
        Disconnected => &[Connecting, Handshake, Disconnecting, Disconnected],
        Initial => &[],
    };
    for &s in path {
        assert!(sm.set(s), "failed to enter {s:?}");
    }
}

#[test]
fn test_transition_alias_works() {
    let sm = ConnectionStateMachine::new();
    assert!(sm.transition(Connecting));
    assert_eq!(sm.current(), Connecting);
}

#[test]
fn test_all_valid_transitions() {
    let valid: &[(ConnectionState, ConnectionState)] = &[
        (Initial, Connecting),
        (Connecting, Handshake),
        (Handshake, Established),
        (Handshake, Disconnecting),
        (Established, Active),
        (Established, Disconnecting),
        (Active, Reconnecting),
        (Active, Disconnecting),
        (Reconnecting, Connecting),
        (Reconnecting, Established),
        (Reconnecting, Disconnected),
        (Disconnecting, Disconnected),
        (Disconnected, Connecting),
    ];
    for (from, to) in valid {
        let sm = ConnectionStateMachine::new();
        enter(&sm, *from);
        assert_eq!(sm.current(), *from);
        assert!(sm.set(*to), "transition {from:?} -> {to:?} should be valid");
        assert_eq!(sm.current(), *to);
    }
}

#[test]
fn test_invalid_transitions_rejected_and_state_kept() {
    let invalid: &[(ConnectionState, ConnectionState)] = &[
        (Initial, Established),
        (Initial, Active),
        (Initial, Disconnected),
        (Connecting, Connecting),
        (Connecting, Disconnected),
        (Handshake, Active),
        (Handshake, Reconnecting),
        (Established, Initial),
        (Established, Handshake),
        (Established, Reconnecting),
        (Active, Initial),
        (Active, Handshake),
        (Active, Established),
        (Reconnecting, Reconnecting),
        (Reconnecting, Handshake),
        (Disconnecting, Initial),
        (Disconnecting, Connecting),
        (Disconnecting, Active),
        (Disconnected, Initial),
        (Disconnected, Handshake),
        (Disconnected, Established),
        (Disconnected, Active),
    ];
    for (from, to) in invalid {
        let sm = ConnectionStateMachine::new();
        enter(&sm, *from);
        assert_eq!(sm.current(), *from);
        assert!(!sm.set(*to), "transition {from:?} -> {to:?} should be invalid");
        assert_eq!(sm.current(), *from, "state must be kept after invalid transition");
    }
}

#[test]
fn test_machine_helpers() {
    let sm = ConnectionStateMachine::new();
    assert!(!sm.is_connected());
    assert!(!sm.is_active());
    assert!(!sm.should_heartbeat());

    enter(&sm, Active);
    assert!(sm.is_connected());
    assert!(sm.is_active());
    assert!(sm.should_heartbeat());
}

#[test]
fn test_display_all_variants() {
    let cases = [
        (Initial, "Initial"),
        (Connecting, "Connecting"),
        (Handshake, "Handshake"),
        (Established, "Established"),
        (Active, "Active"),
        (Reconnecting, "Reconnecting"),
        (Disconnecting, "Disconnecting"),
        (Disconnected, "Disconnected"),
    ];
    for (state, expected) in cases {
        assert_eq!(format!("{}", state), expected);
    }
}

#[test]
fn test_can_send_receive_matrix() {
    assert!(Established.can_send());
    assert!(Active.can_send());
    assert!(!Initial.can_send());
    assert!(!Connecting.can_send());
    assert!(!Handshake.can_send());
    assert!(!Reconnecting.can_send());
    assert!(!Disconnecting.can_send());
    assert!(!Disconnected.can_send());

    assert!(Established.can_receive());
    assert!(Active.can_receive());
    assert!(!Initial.can_receive());
    assert!(!Connecting.can_receive());
    assert!(!Handshake.can_receive());
    assert!(!Reconnecting.can_receive());
    assert!(!Disconnecting.can_receive());
    assert!(!Disconnected.can_receive());

    assert!(Active.should_heartbeat());
    assert!(!Initial.should_heartbeat());
    assert!(!Connecting.should_heartbeat());
    assert!(!Handshake.should_heartbeat());
    assert!(!Established.should_heartbeat());
    assert!(!Reconnecting.should_heartbeat());
    assert!(!Disconnecting.should_heartbeat());
    assert!(!Disconnected.should_heartbeat());
}

#[test]
fn test_connection_state_flags() {
    assert!(Established.is_connected());
    assert!(Active.is_connected());
    assert!(!Initial.is_connected());
    assert!(!Connecting.is_connected());
    assert!(!Handshake.is_connected());
    assert!(!Reconnecting.is_connected());
    assert!(!Disconnecting.is_connected());
    assert!(!Disconnected.is_connected());
}

#[test]
fn test_default_impl() {
    let sm: ConnectionStateMachine = Default::default();
    assert_eq!(sm.current(), Initial);
}
