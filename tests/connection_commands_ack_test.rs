use aex::connection::commands::CommandId;
use aex::connection::commands::ack::AckCommand;

#[test]
fn test_ack_command_accepted() {
    let cmd = AckCommand::accepted(None);
    assert!(cmd.accepted);
    assert!(cmd.session_key_id.is_none());
}

#[test]
fn test_ack_command_accepted_with_key() {
    let cmd = AckCommand::accepted(Some(vec![1, 2, 3]));
    assert!(cmd.accepted);
    assert_eq!(cmd.session_key_id, Some(vec![1, 2, 3]));
}

#[test]
fn test_ack_command_rejected() {
    let cmd = AckCommand::rejected();
    assert!(!cmd.accepted);
    assert!(cmd.session_key_id.is_none());
}

#[test]
fn test_ack_command_id() {
    assert_eq!(AckCommand::id(), CommandId::Ack);
}

#[test]
fn test_ack_command_encode_decode() {
    let cmd = AckCommand::accepted(Some(vec![1, 2, 3]));
    let encoded = cmd.encode();
    assert!(encoded.len() >= 4);

    let decoded = AckCommand::decode(&encoded).unwrap();
    assert!(decoded.accepted);
    assert_eq!(decoded.session_key_id, Some(vec![1, 2, 3]));
}

#[test]
fn test_ack_command_decode_too_short() {
    let result = AckCommand::decode(&[1, 2, 3]);
    assert!(result.is_err());
}

#[test]
fn test_ack_command_decode_invalid_id() {
    let mut data = vec![0u8; 4];
    data[0..4].copy_from_slice(&99999u32.to_le_bytes());
    let result = AckCommand::decode(&data);
    assert!(result.is_err());
}
