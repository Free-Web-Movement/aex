use aex::connection::metrics::ConnectionMetrics;

#[test]
fn test_connection_metrics_new() {
    let metrics = ConnectionMetrics::new();
    assert_eq!(metrics.bytes_sent(), 0);
    assert_eq!(metrics.bytes_received(), 0);
    assert_eq!(metrics.packets_sent(), 0);
    assert_eq!(metrics.packets_received(), 0);
    assert_eq!(metrics.errors(), 0);
}

#[test]
fn test_connection_metrics_record_sent() {
    let metrics = ConnectionMetrics::new();
    metrics.record_sent(100);
    assert_eq!(metrics.bytes_sent(), 100);
    assert_eq!(metrics.packets_sent(), 1);
}

#[test]
fn test_connection_metrics_record_received() {
    let metrics = ConnectionMetrics::new();
    metrics.record_received(200);
    assert_eq!(metrics.bytes_received(), 200);
    assert_eq!(metrics.packets_received(), 1);
}

#[test]
fn test_connection_metrics_record_error() {
    let metrics = ConnectionMetrics::new();
    metrics.record_error();
    assert_eq!(metrics.errors(), 1);
}

#[test]
fn test_connection_metrics_record_latency() {
    let metrics = ConnectionMetrics::new();
    metrics.record_latency(1000);
    assert_eq!(metrics.latency_avg_ns(), 500);
    assert_eq!(metrics.latency_min_ns(), 1000);
    assert_eq!(metrics.latency_max_ns(), 1000);
}

#[test]
fn test_connection_metrics_latency_min_max() {
    let metrics = ConnectionMetrics::new();
    metrics.record_latency(2000);
    metrics.record_latency(5000);
    assert_eq!(metrics.latency_min_ns(), 2000);
    assert_eq!(metrics.latency_max_ns(), 5000);
}

#[test]
fn test_connection_metrics_latency_avg() {
    let metrics = ConnectionMetrics::new();
    metrics.record_latency(1000);
    metrics.record_latency(2000);
    assert_eq!(metrics.latency_avg_ns(), 1250);
}

#[test]
fn test_connection_metrics_throughput() {
    let metrics = ConnectionMetrics::new();
    metrics.record_sent(10_000_000);
    let _ = metrics.throughput_mbps();
}

#[test]
fn test_connection_metrics_packet_loss_rate() {
    let metrics = ConnectionMetrics::new();
    metrics.record_sent(100);
    metrics.record_error();
    let loss = metrics.packet_loss_rate();
    assert!(loss > 0.0);
}

#[test]
fn test_connection_metrics_uptime() {
    let metrics = ConnectionMetrics::new();
    std::thread::sleep(std::time::Duration::from_millis(10));
    assert!(metrics.uptime_secs() >= 0);
}

#[test]
fn test_connection_metrics_default() {
    let metrics = ConnectionMetrics::default();
    assert_eq!(metrics.bytes_sent(), 0);
}
