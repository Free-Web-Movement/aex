use aex::connection::commands::{CommandId, detect_command};

#[test]
fn test_detect_command_valid() {
    let mut data = vec![0u8; 4];
    data[0..4].copy_from_slice(&CommandId::Ping.as_u32().to_le_bytes());
    let result = detect_command(&data);
    assert_eq!(result, Some(CommandId::Ping));
}

#[test]
fn test_detect_command_too_short() {
    let result = detect_command(&[1, 2, 3]);
    assert_eq!(result, None);
}

#[test]
fn test_detect_command_unknown() {
    let mut data = vec![0u8; 4];
    data[0..4].copy_from_slice(&99999u32.to_le_bytes());
    let result = detect_command(&data);
    assert_eq!(result, None);
}
