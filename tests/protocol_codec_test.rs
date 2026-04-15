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
