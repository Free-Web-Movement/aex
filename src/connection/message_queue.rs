use std::collections::VecDeque;
use std::sync::Arc;

use tokio::sync::{mpsc, Mutex};

use crate::connection::commands::CommandId;

#[derive(Debug, Clone)]
pub struct Message {
    pub id: u64,
    pub command_id: CommandId,
    pub payload: Vec<u8>,
    pub timestamp: u64,
    pub retries: u32,
    pub ack_required: bool,
}

impl Message {
    pub fn new(command_id: CommandId, payload: Vec<u8>) -> Self {
        Self {
            id: rand_id(),
            command_id,
            payload,
            timestamp: current_timestamp(),
            retries: 0,
            ack_required: false,
        }
    }

    pub fn with_ack(mut self, required: bool) -> Self {
        self.ack_required = required;
        self
    }
}

fn rand_id() -> u64 {
    use std::time::SystemTime;
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64
}

fn current_timestamp() -> u64 {
    use std::time::SystemTime;
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

pub struct MessageQueueConfig {
    pub max_size: usize,
    pub max_retries: u32,
    pub retry_delay_ms: u64,
    pub ttl_secs: u64,
}

impl MessageQueueConfig {
    pub fn new(max_size: usize) -> Self {
        Self {
            max_size,
            max_retries: 3,
            retry_delay_ms: 1000,
            ttl_secs: 300,
        }
    }
}

impl Default for MessageQueueConfig {
    fn default() -> Self {
        Self::new(1000)
    }
}

pub struct MessageQueue {
    config: MessageQueueConfig,
    pending: Arc<Mutex<VecDeque<Message>>>,
    sent: Arc<Mutex<VecDeque<Message>>>,
    confirmed: Arc<Mutex<std::collections::HashSet<u64>>>,
    tx: mpsc::Sender<Message>,
}

impl MessageQueue {
    pub fn new(config: MessageQueueConfig) -> Self {
        let (tx, _rx) = mpsc::channel(config.max_size);
        Self {
            config,
            pending: Arc::new(Mutex::new(VecDeque::new())),
            sent: Arc::new(Mutex::new(VecDeque::new())),
            confirmed: Arc::new(Mutex::new(std::collections::HashSet::new())),
            tx,
        }
    }

    pub async fn enqueue(&self, message: Message) -> Result<(), QueueError> {
        let mut queue = self.pending.lock().await;
        if queue.len() >= self.config.max_size {
            return Err(QueueError::Full);
        }
        queue.push_back(message);
        Ok(())
    }

    pub async fn dequeue(&self) -> Option<Message> {
        let mut queue = self.pending.lock().await;
        queue.pop_front()
    }

    pub async fn mark_sent(&self, message: Message) {
        let mut sent = self.sent.lock().await;
        sent.push_back(message);
    }

    pub async fn confirm(&self, message_id: u64) {
        let mut confirmed = self.confirmed.lock().await;
        confirmed.insert(message_id);
        
        let mut sent = self.sent.lock().await;
        sent.retain(|m| m.id != message_id);
    }

    pub async fn get_pending_count(&self) -> usize {
        self.pending.lock().await.len()
    }

    pub async fn get_sent_count(&self) -> usize {
        self.sent.lock().await.len()
    }

    pub async fn retry_failed(&self) -> Vec<Message> {
        let mut to_retry = VecDeque::new();
        let now = current_timestamp();
        
        let mut sent = self.sent.lock().await;
        while let Some(msg) = sent.pop_front() {
            if msg.retries >= self.config.max_retries {
                continue;
            }
            if now - msg.timestamp > self.config.ttl_secs {
                continue;
            }
            to_retry.push_back(msg);
        }
        
        let mut pending = self.pending.lock().await;
        while let Some(msg) = to_retry.pop_front() {
            pending.push_back(msg);
        }
        
        pending.iter().cloned().collect()
    }

    pub async fn clear_expired(&self) {
        let now = current_timestamp();
        
        let mut sent = self.sent.lock().await;
        sent.retain(|m| now - m.timestamp <= self.config.ttl_secs);
    }
}

#[derive(Debug)]
pub enum QueueError {
    Full,
    Empty,
    NotFound,
}