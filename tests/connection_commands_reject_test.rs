use aex::connection::commands::CommandId;
use aex::connection::commands::reject::RejectCommand;

#[test]
fn test_reject_command_new() {
    let cmd = RejectCommand::new("connection refused");
    assert_eq!(cmd.reason, "connection refused");
}

#[test]
fn test_reject_command_id() {
    assert_eq!(RejectCommand::id(), CommandId::Reject);
}

#[test]
fn test_reject_command_encode_decode() {
    let cmd = RejectCommand::new("test reason");
    let encoded = cmd.encode();
    assert!(encoded.len() >= 4);

    let decoded = RejectCommand::decode(&encoded).unwrap();
    assert_eq!(decoded.reason, "test reason");
}

#[test]
fn test_reject_command_decode_too_short() {
    let result = RejectCommand::decode(&[1, 2, 3]);
    assert!(result.is_err());
}

#[test]
fn test_reject_command_decode_invalid_id() {
    let mut data = vec![0u8; 4];
    data[0..4].copy_from_slice(&99999u32.to_le_bytes());
    let result = RejectCommand::decode(&data);
    assert!(result.is_err());
}
