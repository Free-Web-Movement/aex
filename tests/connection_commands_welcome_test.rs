use aex::connection::commands::welcome::WelcomeCommand;
use aex::connection::commands::CommandId;
use aex::connection::node::Node;

#[test]
fn test_welcome_command_new() {
    let node = Node::from_addr("127.0.0.1:8080".parse().unwrap(), None, None);
    let cmd = WelcomeCommand::new(node, true, None);
    assert_eq!(cmd.version, 1);
    assert!(cmd.accepted);
}

#[test]
fn test_welcome_command_new_with_key() {
    let node = Node::from_addr("127.0.0.1:8080".parse().unwrap(), None, None);
    let cmd = WelcomeCommand::new(node, true, Some(vec![1, 2, 3]));
    assert!(cmd.accepted);
    assert_eq!(cmd.ephemeral_public, Some(vec![1, 2, 3]));
}

#[test]
fn test_welcome_command_rejected() {
    let cmd = WelcomeCommand::rejected();
    assert!(!cmd.accepted);
}

#[test]
fn test_welcome_command_id() {
    assert_eq!(WelcomeCommand::id(), CommandId::Welcome);
}

#[test]
fn test_welcome_command_encode_decode() {
    let node = Node::from_addr("127.0.0.1:8080".parse().unwrap(), None, None);
    let cmd = WelcomeCommand::new(node, true, Some(vec![1, 2, 3]));
    let encoded = cmd.encode();
    assert!(encoded.len() >= 4);

    let decoded = WelcomeCommand::decode(&encoded).unwrap();
    assert!(decoded.accepted);
}

#[test]
fn test_welcome_command_is_valid() {
    let node = Node::from_addr("127.0.0.1:8080".parse().unwrap(), None, None);
    let cmd = WelcomeCommand::new(node, true, None);
    assert!(cmd.is_valid());
}

#[test]
fn test_welcome_command_decode_too_short() {
    let result = WelcomeCommand::decode(&[1, 2, 3]);
    assert!(result.is_err());
}

#[test]
fn test_welcome_command_decode_invalid_id() {
    let mut data = vec![0u8; 4];
    data[0..4].copy_from_slice(&99999u32.to_le_bytes());
    let result = WelcomeCommand::decode(&data);
    assert!(result.is_err());
}
