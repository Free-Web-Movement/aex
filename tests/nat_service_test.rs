//! NAT 服务与检测层测试。
//!
//! 验证：检测层识别 NAT 魔数（与 HTTP/SOCKS 在同一端口共存）、NatFrame
//! 编解码（带魔数）、NatRelayService 连接处理。

use aex::nat::{
    frame_len_from_header, NatDetector, NatFrame, NatFrameType, NatRelayService, TunnelPeer,
    NAT_MAGIC, NAT_MAGIC_LEN, NAT_MAX_FRAME_BODY,
};
use aex::unified::detect::{DetectionState, ProtocolDetector, Verdict};

#[test]
fn nat_detector_matches_magic() {
    let d = NatDetector;
    let mut state = DetectionState::new();
    assert_eq!(d.protocol(), "nat");
    // 部分魔数 → NeedMore
    match d.detect(&NAT_MAGIC[..3], &mut state) {
        Verdict::NeedMore(_) => {}
        v => panic!("expected NeedMore, got {:?}", v),
    }
    // 完整魔数 → Match
    assert!(matches!(d.detect(NAT_MAGIC, &mut state), Verdict::Match));
}

#[test]
fn nat_detector_passes_non_magic() {
    let d = NatDetector;
    let mut state = DetectionState::new();
    // HTTP 请求行不是 NAT（首字节 G）
    assert!(matches!(
        d.detect(b"GET / HTTP/1.1", &mut state),
        Verdict::Pass
    ));
    // SOCKS5 greeting 不是 NAT（首字节 0x05），且长度已 ≥ 魔数长度
    assert!(matches!(
        d.detect(&[0x05, 0x01, 0x00, 0x02, 0x00, 0x01], &mut state),
        Verdict::Pass
    ));
}

#[test]
fn nat_detector_empty_needs_more() {
    let d = NatDetector;
    let mut state = DetectionState::new();
    assert!(matches!(d.detect(&[], &mut state), Verdict::NeedMore(_)));
}

#[test]
fn nat_frame_roundtrip_with_magic() {
    let frame = NatFrame::data("node_a", "node_b", b"hello".to_vec());
    let bytes = frame.encode_wire().unwrap();
    // 线上帧以魔数开头，随后是长度(4) + body。
    assert_eq!(&bytes[..NAT_MAGIC.len()], NAT_MAGIC);
    let decoded = NatFrame::decode(&bytes).unwrap();
    assert_eq!(decoded.frame_type, NatFrameType::Data);
    assert_eq!(decoded.src, "node_a");
    assert_eq!(decoded.dst, "node_b");
    assert_eq!(decoded.payload, b"hello".to_vec());
}

#[test]
fn nat_frame_register_ack_roundtrip() {
    let frame = NatFrame::register_ack("69.171.73.252:20260");
    let bytes = frame.encode().unwrap();
    let decoded = NatFrame::decode(&bytes).unwrap();
    assert_eq!(decoded.frame_type, NatFrameType::RegisterAck);
    assert_eq!(decoded.public_addr, "69.171.73.252:20260");
}

#[test]
fn nat_frame_punch_hint_carries_peer_addr() {
    let frame = NatFrame::punch_hint("node_b", "101.66.6.227:37226");
    let bytes = frame.encode().unwrap();
    let decoded = NatFrame::decode(&bytes).unwrap();
    assert_eq!(decoded.frame_type, NatFrameType::PunchHint);
    assert_eq!(decoded.src, "node_b");
    assert_eq!(decoded.extra, "101.66.6.227:37226");
}

#[test]
fn nat_frame_decode_without_magic_still_works() {
    // 兼容无魔数的裸 bincode 帧（旧格式）。
    let frame = NatFrame::keep_alive("node_a");
    let body = bincode::encode_to_vec(&frame, bincode::config::standard()).unwrap();
    let decoded = NatFrame::decode(&body).unwrap();
    assert_eq!(decoded.frame_type, NatFrameType::KeepAlive);
    assert_eq!(decoded.src, "node_a");
}

#[test]
fn tunnel_peer_tracks_seen() {
    let peer = TunnelPeer::new("node_a".into(), "1.2.3.4:5000".into());
    assert_eq!(peer.node_id, "node_a");
    assert_eq!(peer.public_addr, "1.2.3.4:5000");
    assert!(peer.last_seen > 0);
}

#[tokio::test]
async fn nat_relay_service_starts_empty() {
    let service = NatRelayService::new();
    assert_eq!(service.peer_count(), 0);
    assert!(service.peers_snapshot().is_empty());
}

#[test]
fn frame_len_from_header_rejects_oversized_length() {
    // 帧头 = 魔数 + 4 字节长度前缀。超限长度必须被拒绝（防巨大内存分配崩溃）。
    let mut header = vec![0u8; NAT_MAGIC_LEN + 4];
    header[..NAT_MAGIC_LEN].copy_from_slice(NAT_MAGIC);
    let oversize = (NAT_MAX_FRAME_BODY as u32) + 1;
    header[NAT_MAGIC_LEN..].copy_from_slice(&oversize.to_le_bytes());
    let err = frame_len_from_header(&header).unwrap_err();
    assert!(err.to_string().contains("exceeds max"), "got: {err}");
}

#[test]
fn frame_len_from_header_accepts_normal_length() {
    let mut header = vec![0u8; NAT_MAGIC_LEN + 4];
    header[..NAT_MAGIC_LEN].copy_from_slice(NAT_MAGIC);
    header[NAT_MAGIC_LEN..].copy_from_slice(&123u32.to_le_bytes());
    assert_eq!(frame_len_from_header(&header).unwrap(), 123);
}

#[test]
fn frame_len_from_header_rejects_short_header() {
    // 帧头不足魔数+长度 → 协议错误。
    assert!(frame_len_from_header(&[0u8; NAT_MAGIC_LEN]).is_err());
}

