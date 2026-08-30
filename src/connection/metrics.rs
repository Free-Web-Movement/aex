use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::Instant;

/// 全局网络流量计数：跨所有连接累计的收发字节，供 Proof of Resource
/// 的"实际带宽使用量"测量使用。由 tcp router / http 层在收发时累加。
static GLOBAL_SENT_BYTES: OnceLock<AtomicU64> = OnceLock::new();
static GLOBAL_RECEIVED_BYTES: OnceLock<AtomicU64> = OnceLock::new();
/// 全局 HTTP API 请求计数：本节点作为服务方实际处理过的请求次数。
static GLOBAL_API_REQUESTS: OnceLock<AtomicU64> = OnceLock::new();

pub(crate) fn global_sent() -> &'static AtomicU64 {
    GLOBAL_SENT_BYTES.get_or_init(|| AtomicU64::new(0))
}

pub(crate) fn global_received() -> &'static AtomicU64 {
    GLOBAL_RECEIVED_BYTES.get_or_init(|| AtomicU64::new(0))
}

/// 记录本进程累计发送的字节数（所有连接 / 协议汇总）。
pub fn record_global_sent(bytes: u64) {
    global_sent().fetch_add(bytes, Ordering::SeqCst);
}

/// 记录本进程累计接收的字节数（所有连接 / 协议汇总）。
pub fn record_global_received(bytes: u64) {
    global_received().fetch_add(bytes, Ordering::SeqCst);
}

/// 记录一个 HTTP API 请求。
pub fn record_global_api_request() {
    let _ = GLOBAL_API_REQUESTS
        .get_or_init(|| AtomicU64::new(0))
        .fetch_add(1, Ordering::SeqCst);
}

/// 本进程累计发送字节数。
pub fn total_sent_bytes() -> u64 {
    global_sent().load(Ordering::SeqCst)
}

/// 本进程累计接收字节数。
pub fn total_received_bytes() -> u64 {
    global_received().load(Ordering::SeqCst)
}

/// 本进程累计发送+接收字节数（实际使用的带宽总量）。
pub fn total_bandwidth_bytes() -> u64 {
    total_sent_bytes().saturating_add(total_received_bytes())
}

/// 本进程累计处理的 HTTP API 请求次数。
pub fn total_api_requests() -> u64 {
    GLOBAL_API_REQUESTS
        .get()
        .map(|a| a.load(Ordering::SeqCst))
        .unwrap_or(0)
}

pub struct ConnectionMetrics {
    pub bytes_sent: AtomicU64,
    pub bytes_received: AtomicU64,
    pub packets_sent: AtomicU64,
    pub packets_received: AtomicU64,
    pub errors: AtomicU64,
    pub last_sent_at: AtomicU64,
    pub last_received_at: AtomicU64,
    pub latency_avg_ns: AtomicU64,
    pub latency_min_ns: AtomicU64,
    pub latency_max_ns: AtomicU64,
    pub start_time: Instant,
}

impl ConnectionMetrics {
    pub fn new() -> Self {
        Self {
            bytes_sent: AtomicU64::new(0),
            bytes_received: AtomicU64::new(0),
            packets_sent: AtomicU64::new(0),
            packets_received: AtomicU64::new(0),
            errors: AtomicU64::new(0),
            last_sent_at: AtomicU64::new(0),
            last_received_at: AtomicU64::new(0),
            latency_avg_ns: AtomicU64::new(0),
            latency_min_ns: AtomicU64::new(u64::MAX),
            latency_max_ns: AtomicU64::new(0),
            start_time: Instant::now(),
        }
    }

    pub fn record_sent(&self, bytes: usize) {
        self.bytes_sent.fetch_add(bytes as u64, Ordering::SeqCst);
        self.packets_sent.fetch_add(1, Ordering::SeqCst);
        self.last_sent_at
            .store(current_timestamp(), Ordering::SeqCst);
    }

    pub fn record_received(&self, bytes: usize) {
        self.bytes_received
            .fetch_add(bytes as u64, Ordering::SeqCst);
        self.packets_received.fetch_add(1, Ordering::SeqCst);
        self.last_received_at
            .store(current_timestamp(), Ordering::SeqCst);
    }

    pub fn record_error(&self) {
        self.errors.fetch_add(1, Ordering::SeqCst);
    }

    pub fn record_latency(&self, ns: u64) {
        let old_avg = self.latency_avg_ns.load(Ordering::SeqCst);
        let new_avg = (old_avg + ns) / 2;
        self.latency_avg_ns.store(new_avg, Ordering::SeqCst);

        let current_min = self.latency_min_ns.load(Ordering::SeqCst);
        if ns < current_min {
            self.latency_min_ns.store(ns, Ordering::SeqCst);
        }

        let current_max = self.latency_max_ns.load(Ordering::SeqCst);
        if ns > current_max {
            self.latency_max_ns.store(ns, Ordering::SeqCst);
        }
    }

    pub fn bytes_sent(&self) -> u64 {
        self.bytes_sent.load(Ordering::SeqCst)
    }

    pub fn bytes_received(&self) -> u64 {
        self.bytes_received.load(Ordering::SeqCst)
    }

    pub fn packets_sent(&self) -> u64 {
        self.packets_sent.load(Ordering::SeqCst)
    }

    pub fn packets_received(&self) -> u64 {
        self.packets_received.load(Ordering::SeqCst)
    }

    pub fn errors(&self) -> u64 {
        self.errors.load(Ordering::SeqCst)
    }

    pub fn latency_avg_ns(&self) -> u64 {
        self.latency_avg_ns.load(Ordering::SeqCst)
    }

    pub fn latency_min_ns(&self) -> u64 {
        let val = self.latency_min_ns.load(Ordering::SeqCst);
        if val == u64::MAX { 0 } else { val }
    }

    pub fn latency_max_ns(&self) -> u64 {
        self.latency_max_ns.load(Ordering::SeqCst)
    }

    pub fn uptime_secs(&self) -> u64 {
        self.start_time.elapsed().as_secs()
    }

    pub fn throughput_mbps(&self) -> f64 {
        let secs = self.uptime_secs() as f64;
        if secs == 0.0 {
            0.0
        } else {
            (self.bytes_sent.load(Ordering::SeqCst) as f64) / secs / 1_000_000.0
        }
    }

    pub fn packet_loss_rate(&self) -> f64 {
        let sent = self.packets_sent.load(Ordering::SeqCst);
        let errors = self.errors.load(Ordering::SeqCst);
        if sent == 0 {
            0.0
        } else {
            (errors as f64) / (sent + errors) as f64
        }
    }
}

impl Default for ConnectionMetrics {
    fn default() -> Self {
        Self::new()
    }
}

fn current_timestamp() -> u64 {
    use std::time::SystemTime;
    match SystemTime::now().duration_since(SystemTime::UNIX_EPOCH) {
        Ok(d) => d.as_nanos() as u64,
        Err(e) => {
            tracing::error!("SystemTime before UNIX_EPOCH: {}", e);
            0
        }
    }
}
