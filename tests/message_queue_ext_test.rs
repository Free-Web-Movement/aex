use aex::connection::commands::CommandId;
use aex::connection::message_queue::{Message, MessageQueue, MessageQueueConfig, QueueError};

#[tokio::test]
async fn test_message_new_defaults() {
    let msg = Message::new(CommandId::Ping, vec![1, 2, 3]);
    assert_eq!(msg.command_id, CommandId::Ping);
    assert_eq!(msg.payload, vec![1, 2, 3]);
    assert_eq!(msg.retries, 0);
    assert!(!msg.ack_required);
    assert_ne!(msg.id, 0, "id 取纳秒时间戳");
    assert_ne!(msg.timestamp, 0);
}

#[tokio::test]
async fn test_message_with_ack_false() {
    let msg = Message::new(CommandId::Ping, vec![1]).with_ack(false);
    assert!(!msg.ack_required);
}

#[test]
fn test_message_queue_config_new_defaults() {
    let config = MessageQueueConfig::new(50);
    assert_eq!(config.max_size, 50);
    assert_eq!(config.max_retries, 3);
    assert_eq!(config.retry_delay_ms, 1000);
    assert_eq!(config.ttl_secs, 300);
}

#[test]
fn test_message_queue_config_default() {
    let config = MessageQueueConfig::default();
    assert_eq!(config.max_size, 1000);
    assert_eq!(config.max_retries, 3);
    assert_eq!(config.retry_delay_ms, 1000);
    assert_eq!(config.ttl_secs, 300);
}

#[tokio::test]
async fn test_dequeue_empty_returns_none() {
    let queue = MessageQueue::new(MessageQueueConfig::new(10));
    assert!(queue.dequeue().await.is_none());
}

#[tokio::test]
async fn test_enqueue_dequeue_fifo_order() {
    let queue = MessageQueue::new(MessageQueueConfig::new(10));
    queue.enqueue(Message::new(CommandId::Ping, vec![1])).await.unwrap();
    queue.enqueue(Message::new(CommandId::Pong, vec![2])).await.unwrap();

    let first = queue.dequeue().await.unwrap();
    let second = queue.dequeue().await.unwrap();
    assert_eq!(first.payload, vec![1]);
    assert_eq!(second.payload, vec![2]);
}

#[tokio::test]
async fn test_confirm_unknown_id_noop() {
    let queue = MessageQueue::new(MessageQueueConfig::new(10));
    let msg = Message::new(CommandId::Ping, vec![1]);
    queue.mark_sent(msg).await;
    queue.confirm(12345).await;
    assert_eq!(queue.get_sent_count().await, 1);
    assert_eq!(queue.get_pending_count().await, 0);
}

#[tokio::test]
async fn test_confirm_only_affects_sent() {
    let queue = MessageQueue::new(MessageQueueConfig::new(10));
    let msg = Message::new(CommandId::Ping, vec![1]);
    let id = msg.id;
    queue.enqueue(msg).await.unwrap();
    queue.confirm(id).await;
    assert_eq!(queue.get_pending_count().await, 1);
}

#[tokio::test]
async fn test_retry_failed_empty_sent() {
    let queue = MessageQueue::new(MessageQueueConfig::new(10));
    assert!(queue.retry_failed().await.is_empty());
}

#[tokio::test]
async fn test_retry_failed_drops_over_retries() {
    let config = MessageQueueConfig {
        max_size: 10,
        max_retries: 2,
        retry_delay_ms: 1000,
        ttl_secs: 300,
    };
    let queue = MessageQueue::new(config);

    let mut msg = Message::new(CommandId::Ping, vec![1]);
    msg.retries = 2;
    queue.mark_sent(msg).await;

    let result = queue.retry_failed().await;
    assert!(result.is_empty());
    assert_eq!(queue.get_sent_count().await, 0);
    assert_eq!(queue.get_pending_count().await, 0);
}

#[tokio::test]
async fn test_retry_failed_drops_ttl_expired() {
    let config = MessageQueueConfig {
        max_size: 10,
        max_retries: 10,
        retry_delay_ms: 1000,
        ttl_secs: 1,
    };
    let queue = MessageQueue::new(config);

    let mut msg = Message::new(CommandId::Ping, vec![1]);
    msg.timestamp = 0;
    queue.mark_sent(msg).await;

    let result = queue.retry_failed().await;
    assert!(result.is_empty());
    assert_eq!(queue.get_sent_count().await, 0);
}

#[tokio::test]
async fn test_retry_failed_moves_valid_message_back() {
    let queue = MessageQueue::new(MessageQueueConfig::new(10));

    let msg = Message::new(CommandId::Ping, vec![42]);
    let id = msg.id;
    queue.mark_sent(msg).await;

    let result = queue.retry_failed().await;
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].id, id);
    assert_eq!(queue.get_sent_count().await, 0);
    assert_eq!(queue.get_pending_count().await, 1);
}

#[tokio::test]
async fn test_retry_failed_mixed_branches() {
    let config = MessageQueueConfig {
        max_size: 10,
        max_retries: 2,
        retry_delay_ms: 1000,
        ttl_secs: 300,
    };
    let queue = MessageQueue::new(config);

    let mut over = Message::new(CommandId::Ping, vec![1]);
    over.retries = 2;
    queue.mark_sent(over).await;

    let mut expired = Message::new(CommandId::Ping, vec![2]);
    expired.timestamp = 0;
    queue.mark_sent(expired).await;

    let good = Message::new(CommandId::Ping, vec![3]);
    queue.mark_sent(good).await;

    let result = queue.retry_failed().await;
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].payload, vec![3]);
    assert_eq!(queue.get_pending_count().await, 1);
    assert_eq!(queue.get_sent_count().await, 0);
}

#[tokio::test]
async fn test_clear_expired_keeps_fresh_removes_old() {
    let queue = MessageQueue::new(MessageQueueConfig::new(10));

    let fresh = Message::new(CommandId::Ping, vec![1]);
    queue.mark_sent(fresh).await;

    let mut old = Message::new(CommandId::Ping, vec![2]);
    old.timestamp = 0;
    queue.mark_sent(old).await;

    queue.clear_expired().await;
    assert_eq!(queue.get_sent_count().await, 1);
}

#[test]
fn test_message_clone_and_queue_error_debug() {
    let msg = Message::new(CommandId::Pong, vec![1, 2]).with_ack(true);
    let cloned = msg.clone();
    assert_eq!(cloned.id, msg.id);
    assert_eq!(cloned.payload, msg.payload);
    assert!(cloned.ack_required);

    let _ = format!("{:?}", QueueError::Full);
    let _ = format!("{:?}", QueueError::Empty);
    let _ = format!("{:?}", QueueError::NotFound);
}
