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

#[test]
fn test_global_counters_record_and_total() {
    use aex::connection::metrics::{
        record_global_api_request, record_global_received, record_global_sent, total_api_requests,
        total_bandwidth_bytes, total_received_bytes, total_sent_bytes,
    };
    // 全局计数器跨测试累计，这里验证「记录后至少增加对应量」
    let before_sent = total_sent_bytes();
    let before_recv = total_received_bytes();
    let before_api = total_api_requests();

    record_global_sent(100);
    record_global_received(50);
    record_global_api_request();

    assert!(total_sent_bytes() >= before_sent + 100);
    assert!(total_received_bytes() >= before_recv + 50);
    assert!(total_api_requests() >= before_api + 1);
    assert!(total_bandwidth_bytes() >= before_sent + before_recv + 150);
}

#[test]
fn test_latency_min_initial_zero_when_no_record() {
    let metrics = ConnectionMetrics::new();
    // 未记录任何延迟时，min 应为 0（初始 u64::MAX 被归一化）
    assert_eq!(metrics.latency_min_ns(), 0);
    assert_eq!(metrics.latency_max_ns(), 0);
    assert_eq!(metrics.latency_avg_ns(), 0);
}

#[test]
fn test_packet_loss_rate_zero_when_no_sent() {
    let metrics = ConnectionMetrics::new();
    // 未发送任何包时 loss rate 为 0.0
    assert_eq!(metrics.packet_loss_rate(), 0.0);
}
