use aex::connection::commands::CommandId;
use aex::connection::message_queue::{Message, MessageQueue, MessageQueueConfig, QueueError};

#[tokio::test]
async fn test_message_queue_new() {
    let config = MessageQueueConfig::new(100);
    let queue = MessageQueue::new(config);
    assert_eq!(queue.get_pending_count().await, 0);
    assert_eq!(queue.get_sent_count().await, 0);
}

#[tokio::test]
async fn test_message_queue_enqueue_dequeue() {
    let config = MessageQueueConfig::new(10);
    let queue = MessageQueue::new(config);

    let msg = Message::new(CommandId::Ping, vec![1, 2, 3]);
    queue.enqueue(msg).await.unwrap();

    let dequeued = queue.dequeue().await;
    assert!(dequeued.is_some());
    assert_eq!(dequeued.unwrap().payload, vec![1, 2, 3]);
}

#[tokio::test]
async fn test_message_queue_full() {
    let config = MessageQueueConfig::new(2);
    let queue = MessageQueue::new(config);

    queue
        .enqueue(Message::new(CommandId::Ping, vec![1]))
        .await
        .unwrap();
    queue
        .enqueue(Message::new(CommandId::Ping, vec![2]))
        .await
        .unwrap();

    let result = queue.enqueue(Message::new(CommandId::Ping, vec![3])).await;
    assert!(matches!(result, Err(QueueError::Full)));
}

#[tokio::test]
async fn test_message_queue_mark_sent_and_confirm() {
    let config = MessageQueueConfig::new(10);
    let queue = MessageQueue::new(config);

    let msg = Message::new(CommandId::Ping, vec![1, 2]);
    let msg_id = msg.id;
    queue.enqueue(msg).await.unwrap();

    let sent_msg = queue.dequeue().await.unwrap();
    queue.mark_sent(sent_msg).await;
    assert_eq!(queue.get_sent_count().await, 1);

    queue.confirm(msg_id).await;
    assert_eq!(queue.get_sent_count().await, 0);
}

#[tokio::test]
async fn test_message_queue_retry_failed() {
    let config = MessageQueueConfig {
        max_size: 10,
        max_retries: 2,
        retry_delay_ms: 1000,
        ttl_secs: 300,
    };
    let queue = MessageQueue::new(config);

    let msg = Message::new(CommandId::Ping, vec![1]);
    queue.enqueue(msg).await.unwrap();
    let sent = queue.dequeue().await.unwrap();
    queue.mark_sent(sent).await;

    let pending = queue.retry_failed().await;
    assert!(!pending.is_empty());
}

#[tokio::test]
async fn test_message_queue_clear_expired() {
    let config = MessageQueueConfig {
        max_size: 10,
        max_retries: 1,
        retry_delay_ms: 1000,
        ttl_secs: 0,
    };
    let queue = MessageQueue::new(config);

    let msg = Message::new(CommandId::Ping, vec![1]);
    queue.enqueue(msg).await.unwrap();
    let sent = queue.dequeue().await.unwrap();
    queue.mark_sent(sent).await;

    queue.clear_expired().await;
}

#[tokio::test]
async fn test_message_with_ack() {
    let msg = Message::new(CommandId::Ping, vec![1, 2]).with_ack(true);
    assert!(msg.ack_required);
}
