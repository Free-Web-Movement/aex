use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use tokio::sync::Mutex;

use aex::connection::commands::{
    PingCommand, PongCommand, CommandId,
};
use aex::connection::node::Node;
use aex::connection::heartbeat::{HeartbeatConfig, HeartbeatManager};
use aex::crypto::session_key_manager::PairedSessionKey;

#[test]
fn test_ping_command_constants() {
    assert_eq!(CommandId::Ping.as_u32(), 5);
    assert_eq!(CommandId::Pong.as_u32(), 6);
}

#[test]
fn test_ping_command_creation() {
    let ping = PingCommand::new();
    
    assert!(ping.timestamp > 0);
    assert!(ping.nonce.is_none());
}

#[test]
fn test_ping_command_with_nonce() {
    let nonce = vec![1, 2, 3, 4, 5, 6, 7, 8];
    let ping = PingCommand::with_nonce(nonce.clone());
    
    assert_eq!(ping.nonce, Some(nonce));
}

#[test]
fn test_ping_command_encode_decode() {
    let ping = PingCommand::new();
    
    let encoded = ping.encode();
    assert!(encoded.len() > 4);
    
    let id = u32::from_le_bytes(encoded[0..4].try_into().unwrap());
    assert_eq!(id, CommandId::Ping.as_u32());
    
    let decoded = PingCommand::decode(&encoded).unwrap();
    assert_eq!(decoded.timestamp, ping.timestamp);
}

#[test]
fn test_ping_command_decode_invalid() {
    let data = vec![0xFFu8; 10];
    let result = PingCommand::decode(&data);
    assert!(result.is_err());
}

#[test]
fn test_ping_command_decode_wrong_id() {
    let mut data = vec![0u8; 8];
    data[0..4].copy_from_slice(&11u32.to_le_bytes());
    let result = PingCommand::decode(&data);
    assert!(result.is_err());
}

#[test]
fn test_pong_command_creation() {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    
    let pong = PongCommand::new(timestamp, None);
    
    assert_eq!(pong.timestamp, timestamp);
    assert!(pong.nonce.is_none());
    assert!(pong.local_time > 0);
}

#[test]
fn test_pong_command_with_nonce() {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let nonce = vec![9, 8, 7, 6, 5, 4, 3, 2, 1];
    
    let pong = PongCommand::new(timestamp, Some(nonce.clone()));
    
    assert_eq!(pong.nonce, Some(nonce));
}

#[test]
fn test_pong_command_encode_decode() {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let pong = PongCommand::new(timestamp, None);
    
    let encoded = pong.encode();
    let id = u32::from_le_bytes(encoded[0..4].try_into().unwrap());
    assert_eq!(id, CommandId::Pong.as_u32());
    
    let decoded = PongCommand::decode(&encoded).unwrap();
    assert_eq!(decoded.timestamp, timestamp);
}

#[test]
fn test_pong_latency_calculation() {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    
    let pong = PongCommand::new(now - 5, None);
    let latency = pong.latency();
    
    assert_eq!(latency, 5);
}

#[test]
fn test_heartbeat_config_creation() {
    let config = HeartbeatConfig::new();
    
    assert_eq!(config.interval_secs, 30);
    assert_eq!(config.timeout_secs, 10);
}

#[test]
fn test_heartbeat_config_builder() {
    let config = HeartbeatConfig::new()
        .with_interval(60)
        .with_timeout(20);
    
    assert_eq!(config.interval_secs, 60);
    assert_eq!(config.timeout_secs, 20);
}

#[test]
fn test_heartbeat_config_callbacks() {
    let config = HeartbeatConfig::new()
        .on_timeout(|_addr| {})
        .on_latency(|_addr, _latency| {});
    
    assert!(config.on_timeout.is_some());
    assert!(config.on_latency.is_some());
}

#[tokio::test]
async fn test_heartbeat_manager_creation() {
    let node = Node::from_system(8080, vec![0x11u8; 32], 1);
    let manager = HeartbeatManager::new(node.clone());
    
    assert_eq!(manager.local_node.id, node.id);
}

#[tokio::test]
async fn test_heartbeat_manager_with_config() {
    let node = Node::from_system(8080, vec![0x22u8; 32], 1);
    let config = HeartbeatConfig::new().with_interval(45);
    let manager = HeartbeatManager::new(node).with_config(config);
    
    assert_eq!(manager.config.interval_secs, 45);
}

#[tokio::test]
async fn test_heartbeat_manager_create_ping() {
    let node = Node::from_system(8080, vec![0x33u8; 32], 1);
    let manager = HeartbeatManager::new(node);
    
    let ping = manager.create_ping();
    
    assert!(ping.timestamp > 0);
}

#[tokio::test]
async fn test_heartbeat_manager_create_ping_with_keys() {
    let node = Node::from_system(8080, vec![0x44u8; 32], 1);
    let keys = Arc::new(Mutex::new(PairedSessionKey::new(32)));
    let manager = HeartbeatManager::new(node).with_session_keys(keys);
    
    let ping = manager.create_ping();
    assert!(ping.nonce.is_some());
}

#[tokio::test]
async fn test_heartbeat_manager_create_pong() {
    let node = Node::from_system(8080, vec![0x55u8; 32], 1);
    let manager = HeartbeatManager::new(node);
    
    let ping = PingCommand::new();
    let pong = manager.create_pong(&ping);
    
    assert_eq!(pong.timestamp, ping.timestamp);
}

#[tokio::test]
async fn test_heartbeat_manager_check_timeout() {
    use std::net::SocketAddr;
    
    let node = Node::from_system(8080, vec![0x77u8; 32], 1);
    let config = HeartbeatConfig::new().on_timeout(|_addr| {});
    let manager = HeartbeatManager::new(node).with_config(config);
    
    let peer_addr: SocketAddr = "127.0.0.1:8080".parse().unwrap();
    
    manager.set_connection_state(peer_addr, 3, 0).await;
    
    let timed_out = manager.check_timeout(peer_addr).await;
    assert!(timed_out);
}

#[tokio::test]
async fn test_heartbeat_manager_remove_connection() {
    use std::net::SocketAddr;
    
    let node = Node::from_system(8080, vec![0x88u8; 32], 1);
    let manager = HeartbeatManager::new(node);
    
    let peer_addr: SocketAddr = "127.0.0.1:8080".parse().unwrap();
    
    manager.set_connection_state(peer_addr, 0, 1000).await;
    
    manager.remove_connection(&peer_addr).await;
    
    let latency = manager.get_latency(peer_addr).await;
    assert!(latency.is_none());
}

#[tokio::test]
async fn test_heartbeat_manager_get_latency() {
    use std::net::SocketAddr;
    
    let node = Node::from_system(8080, vec![0x99u8; 32], 1);
    let manager = HeartbeatManager::new(node);
    
    let peer_addr: SocketAddr = "127.0.0.1:8080".parse().unwrap();
    
    manager.set_connection_state(peer_addr, 0, 5000).await;
    
    let latency = manager.get_latency(peer_addr).await;
    assert_eq!(latency, Some(5000));
}