use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

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
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64
}
