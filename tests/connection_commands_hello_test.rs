use aex::connection::commands::CommandId;
use aex::connection::commands::hello::HelloCommand;
use aex::connection::node::Node;

#[test]
fn test_hello_command_new() {
    let node = Node::from_addr("127.0.0.1:8080".parse().unwrap(), None, None);
    let cmd = HelloCommand::new(node.clone(), None, false);
    assert_eq!(cmd.version, 1);
    assert!(!cmd.request_encryption);
}

#[test]
fn test_hello_command_new_with_encryption() {
    let node = Node::from_addr("127.0.0.1:8080".parse().unwrap(), None, None);
    let cmd = HelloCommand::new(node.clone(), Some(vec![1, 2, 3]), true);
    assert_eq!(cmd.version, 1);
    assert!(cmd.request_encryption);
    assert_eq!(cmd.ephemeral_public, Some(vec![1, 2, 3]));
}

#[test]
fn test_hello_command_id() {
    assert_eq!(HelloCommand::id(), CommandId::Hello);
}

#[test]
fn test_hello_command_encode_decode() {
    let node = Node::from_addr("127.0.0.1:8080".parse().unwrap(), None, None);
    let cmd = HelloCommand::new(node, Some(vec![1, 2, 3]), true);
    let encoded = cmd.encode();
    assert!(encoded.len() >= 4);

    let decoded = HelloCommand::decode(&encoded).unwrap();
    assert_eq!(decoded.version, 1);
    assert!(decoded.request_encryption);
}

#[test]
fn test_hello_command_is_valid() {
    let node = Node::from_addr("127.0.0.1:8080".parse().unwrap(), None, None);
    let cmd = HelloCommand::new(node, None, false);
    assert!(cmd.is_valid());
}

#[test]
fn test_hello_command_decode_too_short() {
    let result = HelloCommand::decode(&[1, 2, 3]);
    assert!(result.is_err());
}

#[test]
fn test_hello_command_decode_invalid_id() {
    let mut data = vec![0u8; 4];
    data[0..4].copy_from_slice(&99999u32.to_le_bytes());
    let result = HelloCommand::decode(&data);
    assert!(result.is_err());
}
