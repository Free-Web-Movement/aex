use aex::connection::commands::CommandId;
use aex::connection::protocol_codec::{FrameHeader, ProtocolCodec, ProtocolFlags, ProtocolFrame};

#[test]
fn test_protocol_flags() {
    let flags = ProtocolFlags::NONE;
    assert!(!flags.has_compressed());
    assert!(!flags.has_encrypted());
    assert!(!flags.has_priority());
    assert!(!flags.has_fragment());

    let flags = ProtocolFlags::COMPRESSED;
    assert!(flags.has_compressed());
}

#[test]
fn test_protocol_flags_encrypted() {
    let flags = ProtocolFlags::ENCRYPTED;
    assert!(flags.has_encrypted());
}

#[test]
fn test_protocol_flags_priority() {
    let flags = ProtocolFlags::PRIORITY;
    assert!(flags.has_priority());
}

#[test]
fn test_protocol_flags_fragment() {
    let flags = ProtocolFlags::FRAGMENT;
    assert!(flags.has_fragment());
}

#[test]
fn test_frame_header_new() {
    let header = FrameHeader::new(CommandId::Ping, 100);
    assert_eq!(header.command_id, CommandId::Ping.as_u32());
    assert_eq!(header.payload_length, 100);
}

#[test]
fn test_frame_header_with_flags() {
    let header = FrameHeader::new(CommandId::Ping, 100).with_flags(ProtocolFlags::COMPRESSED);
    assert!(header.flags().has_compressed());
}

#[test]
fn test_frame_header_with_sequence() {
    let header = FrameHeader::new(CommandId::Ping, 100).with_sequence(42);
    assert_eq!(header.sequence, 42);
}

#[test]
fn test_frame_header_command() {
    let header = FrameHeader::new(CommandId::Ping, 100);
    assert_eq!(header.command(), Some(CommandId::Ping));
}

#[test]
fn test_frame_header_encode_decode() {
    let header = FrameHeader::new(CommandId::Ping, 100)
        .with_flags(ProtocolFlags::COMPRESSED)
        .with_sequence(42);

    let encoded = header.encode();
    let decoded = FrameHeader::decode(&encoded).unwrap();

    assert_eq!(decoded.command_id, header.command_id);
    assert_eq!(decoded.flags, header.flags);
    assert_eq!(decoded.sequence, header.sequence);
}

#[test]
fn test_frame_header_decode_too_short() {
    let result = FrameHeader::decode(&[0, 1, 2]);
    assert!(result.is_err());
}

#[test]
fn test_protocol_frame_new() {
    let frame = ProtocolFrame::new(CommandId::Ping, vec![1, 2, 3]);
    assert_eq!(frame.command_id(), Some(CommandId::Ping));
    assert_eq!(frame.payload, vec![1, 2, 3]);
}

#[test]
fn test_protocol_frame_encode() {
    let frame = ProtocolFrame::new(CommandId::Ping, vec![1, 2, 3]);
    let encoded = frame.encode();
    assert!(encoded.len() > 3);
}

#[test]
fn test_protocol_frame_encode_with_length() {
    let frame = ProtocolFrame::new(CommandId::Ping, vec![1, 2, 3]);
    let encoded = frame.encode_with_length();
    assert!(encoded.len() > 4);
}

#[test]
fn test_protocol_codec_new() {
    let mut codec = ProtocolCodec::new();
    assert_eq!(codec.next_sequence(), 1);
}

#[test]
fn test_protocol_codec_next_sequence() {
    let mut codec = ProtocolCodec::new();
    let seq1 = codec.next_sequence();
    let seq2 = codec.next_sequence();
    assert!(seq2 > seq1);
}

#[test]
fn test_protocol_flags_combined() {
    // 组合标志通过 encode→decode 往返保留验证（用全部单个标志相加的语义）
    let h = FrameHeader::new(CommandId::Ping, 0)
        .with_flags(ProtocolFlags::FRAGMENT)
        .with_flags(ProtocolFlags::COMPRESSED);
    // with_flags 会覆盖，最后是 COMPRESSED
    assert!(h.flags().has_compressed());
    assert!(!h.flags().has_fragment());

    // 编码解码往返保留 flags
    let encoded = h.encode();
    let decoded = FrameHeader::decode(&encoded).unwrap();
    assert!(decoded.flags().has_compressed());
}

#[test]
fn test_frame_header_command_none_for_unknown_id() {
    // 构造一个无效 command_id 的 header，command() 应返回 None
    let header = FrameHeader {
        command_id: 999,
        flags: 0,
        sequence: 0,
        payload_length: 0,
    };
    assert_eq!(header.command(), None);
}

#[test]
fn test_protocol_frame_decode_roundtrip() {
    let frame = ProtocolFrame::new(CommandId::Pong, vec![1, 2, 3, 4]);
    let encoded = frame.encode();
    let decoded = ProtocolFrame::decode(&encoded).unwrap();
    assert_eq!(decoded.payload, vec![1, 2, 3, 4]);
    assert_eq!(decoded.command_id(), Some(CommandId::Pong));
}

#[test]
fn test_protocol_frame_decode_too_short() {
    let result = ProtocolFrame::decode(&[1, 2, 3]);
    assert!(result.is_err());
}

#[test]
fn test_protocol_frame_decode_incomplete_payload() {
    // header 声称 payload 很长，但数据不足
    let header = FrameHeader::new(CommandId::Ping, 100);
    let mut bytes = header.encode();
    bytes.extend_from_slice(&[1, 2, 3]);
    let result = ProtocolFrame::decode(&bytes);
    assert!(result.is_err());
}

#[test]
fn test_protocol_frame_decode_with_length_roundtrip() {
    let frame = ProtocolFrame::new(CommandId::Ack, vec![9, 9, 9]);
    let encoded = frame.encode_with_length();
    let decoded = ProtocolFrame::decode_with_length(&encoded).unwrap();
    assert_eq!(decoded.payload, vec![9, 9, 9]);
    assert_eq!(decoded.command_id(), Some(CommandId::Ack));
}

#[test]
fn test_protocol_frame_decode_with_length_short() {
    assert!(ProtocolFrame::decode_with_length(&[1, 2, 3]).is_err());
}

#[test]
fn test_protocol_frame_decode_with_length_incomplete() {
    // 长度声明 100，但数据不足
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&100u32.to_le_bytes());
    bytes.extend_from_slice(&[1, 2, 3]);
    assert!(ProtocolFrame::decode_with_length(&bytes).is_err());
}

#[test]
fn test_protocol_codec_encode_and_decode() {
    let codec = ProtocolCodec::new();
    let encoded = codec.encode(CommandId::Ping, b"payload");
    let decoded = codec.decode(&encoded).unwrap();
    assert_eq!(decoded.payload, b"payload");
    assert_eq!(decoded.command_id(), Some(CommandId::Ping));
}

#[test]
fn test_protocol_codec_default() {
    let codec = ProtocolCodec::default();
    let encoded = codec.encode(CommandId::Pong, b"x");
    assert!(codec.decode(&encoded).is_ok());
}

