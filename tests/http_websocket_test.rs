#[cfg(test)]
mod tests {
    use bytes::BytesMut;
    use aex::http::websocket::{WSFrame, WSCodec};
    use aex::tcp::types::{Codec, Command, Frame};
    use tokio_util::codec::{Decoder, Encoder};

    #[test]
    fn test_ws_frame_variants() {
        let cont = WSFrame::Continuation(vec![1, 2, 3]);
        assert!(matches!(cont, WSFrame::Continuation(_)));

        let text = WSFrame::Text("hello".to_string());
        assert!(matches!(text, WSFrame::Text(_)));

        let binary = WSFrame::Binary(vec![0xFF, 0x00]);
        assert!(matches!(binary, WSFrame::Binary(_)));

        let reserved_nc = WSFrame::ReservedNonControl(3, vec![]);
        assert!(matches!(reserved_nc, WSFrame::ReservedNonControl(_, _)));

        let close = WSFrame::Close(1000, Some("close reason".to_string()));
        assert!(matches!(close, WSFrame::Close(_, _)));

        let ping = WSFrame::Ping(vec![1, 2]);
        assert!(matches!(ping, WSFrame::Ping(_)));

        let pong = WSFrame::Pong(vec![3, 4]);
        assert!(matches!(pong, WSFrame::Pong(_)));

        let reserved_c = WSFrame::ReservedControl(11, vec![]);
        assert!(matches!(reserved_c, WSFrame::ReservedControl(_, _)));
    }

    #[test]
    fn test_ws_frame_opcode_via_command_trait() {
        // Use the Command trait's id() method
        assert_eq!(WSFrame::Continuation(vec![]).id(), 0x0);
        assert_eq!(WSFrame::Text("test".to_string()).id(), 0x1);
        assert_eq!(WSFrame::Binary(vec![]).id(), 0x2);
        assert_eq!(WSFrame::Close(0, None).id(), 0x8);
        assert_eq!(WSFrame::Ping(vec![]).id(), 0x9);
        assert_eq!(WSFrame::Pong(vec![]).id(), 0xa);
    }

    #[test]
    fn test_ws_frame_payload() {
        assert_eq!(WSFrame::Text("hello".to_string()).payload(), Some(b"hello".to_vec()));
        assert_eq!(WSFrame::Binary(vec![1, 2]).payload(), Some(vec![1, 2]));
        assert_eq!(WSFrame::Ping(vec![1]).payload(), Some(vec![1]));
        assert_eq!(WSFrame::Pong(vec![2]).payload(), Some(vec![2]));
        
        // Close frame has no payload
        assert!(WSFrame::Close(0, None).payload().is_none());
    }

    #[test]
    fn test_ws_frame_data() {
        assert_eq!(*WSFrame::Binary(vec![1, 2]).data(), vec![1, 2]);
        assert_eq!(*WSFrame::Ping(vec![1]).data(), vec![1]);
        assert!(WSFrame::Text("test".to_string()).data().is_empty());
    }

    #[test]
    fn test_ws_frame_command() {
        // WSFrame doesn't have command - returns None
        assert!(WSFrame::Text("hello".to_string()).command().is_none());
    }

    #[test]
    fn test_ws_frame_traits() {
        let f1 = WSFrame::Text("test".to_string());
        let f2 = f1.clone();
        assert_eq!(f1, f2);

        let _ = format!("{:?}", f1);
    }

    #[tokio::test]
    async fn test_ws_codec_decode_text_frame() {
        let mut codec = WSCodec {};
        let mut src = BytesMut::from(&[0x81, 0x05][..]); // FIN + text opcode, length 5
        src.extend_from_slice(b"hello");

        let result = codec.decode(&mut src);
        assert!(result.is_ok());
        let frame = result.unwrap().unwrap();
        assert_eq!(frame, WSFrame::Text("hello".to_string()));
    }

    #[tokio::test]
    async fn test_ws_codec_decode_binary_frame() {
        let mut codec = WSCodec {};
        let mut src = BytesMut::from(&[0x82, 0x02][..]); // FIN + binary opcode, length 2
        src.extend_from_slice(&[0xFF, 0x00]);

        let result = codec.decode(&mut src);
        assert!(result.is_ok());
        let frame = result.unwrap().unwrap();
        assert_eq!(frame, WSFrame::Binary(vec![0xFF, 0x00]));
    }

    #[tokio::test]
    async fn test_ws_codec_decode_ping_frame() {
        let mut codec = WSCodec {};
        let mut src = BytesMut::from(&[0x89, 0x02][..]); // FIN + ping opcode, length 2
        src.extend_from_slice(&[0x01, 0x02]);

        let result = codec.decode(&mut src);
        assert!(result.is_ok());
        let frame = result.unwrap().unwrap();
        assert_eq!(frame, WSFrame::Ping(vec![1, 2]));
    }

    #[tokio::test]
    async fn test_ws_codec_decode_pong_frame() {
        let mut codec = WSCodec {};
        let mut src = BytesMut::from(&[0x8A, 0x02][..]); // FIN + pong opcode, length 2
        src.extend_from_slice(&[0x03, 0x04]);

        let result = codec.decode(&mut src);
        assert!(result.is_ok());
        let frame = result.unwrap().unwrap();
        assert_eq!(frame, WSFrame::Pong(vec![3, 4]));
    }

    #[tokio::test]
    async fn test_ws_codec_decode_close_frame() {
        let mut codec = WSCodec {};
        let mut src = BytesMut::from(&[0x88, 0x02][..]); // FIN + close opcode, length 2
        src.extend_from_slice(&[0x03, 0xE8]); // 1000 in big endian

        let result = codec.decode(&mut src);
        assert!(result.is_ok());
        let frame = result.unwrap().unwrap();
        match frame {
            WSFrame::Close(code, reason) => {
                assert_eq!(code, 1000);
                assert!(reason.is_none());
            }
            _ => panic!("Expected Close frame"),
        }
    }

    #[tokio::test]
    async fn test_ws_codec_decode_continuation() {
        let mut codec = WSCodec {};
        let mut src = BytesMut::from(&[0x00, 0x03][..]); // Continuation opcode, length 3
        src.extend_from_slice(b"abc");

        let result = codec.decode(&mut src);
        assert!(result.is_ok());
        let frame = result.unwrap().unwrap();
        assert_eq!(frame, WSFrame::Continuation(b"abc".to_vec()));
    }

    #[tokio::test]
    async fn test_ws_codec_decode_reserved_non_control() {
        let mut codec = WSCodec {};
        let mut src = BytesMut::from(&[0x83, 0x02][..]); // Reserved opcode 3, length 2
        src.extend_from_slice(&[0x01, 0x02]);

        let result = codec.decode(&mut src);
        assert!(result.is_ok());
        let frame = result.unwrap().unwrap();
        match frame {
            WSFrame::ReservedNonControl(op, data) => {
                assert_eq!(op, 3);
                assert_eq!(data, vec![1, 2]);
            }
            _ => panic!("Expected ReservedNonControl frame"),
        }
    }

    #[tokio::test]
    async fn test_ws_codec_decode_reserved_control() {
        let mut codec = WSCodec {};
        let mut src = BytesMut::from(&[0x8B, 0x02][..]); // Reserved opcode 11, length 2
        src.extend_from_slice(&[0x01, 0x02]);

        let result = codec.decode(&mut src);
        assert!(result.is_ok());
        let frame = result.unwrap().unwrap();
        match frame {
            WSFrame::ReservedControl(op, data) => {
                assert_eq!(op, 11);
                assert_eq!(data, vec![1, 2]);
            }
            _ => panic!("Expected ReservedControl frame"),
        }
    }

    #[tokio::test]
    async fn test_ws_codec_decode_extended_length_16() {
        let mut codec = WSCodec {};
        let mut src = BytesMut::from(&[0x82, 0x7E, 0x00, 0x10][..]); // length = 16
        src.extend(vec![0u8; 16]);

        let result = codec.decode(&mut src);
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_ws_codec_decode_extended_length_64() {
        let mut codec = WSCodec {};
        let mut src = BytesMut::from(&[0x82, 0x7F, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01][..]); // length = 1
        src.extend(vec![0u8; 1]);

        let result = codec.decode(&mut src);
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_ws_codec_decode_insufficient_data() {
        let mut codec = WSCodec {};
        let mut src = BytesMut::from(&[0x81][..]); // Only first byte, no length

        let result = codec.decode(&mut src);
        assert!(result.unwrap().is_none());
    }

#[tokio::test]
    async fn test_ws_codec_decode_masked() {
        let mut codec = WSCodec {};
        // Mask bit set (0x80), length 5, mask key 0x01020304
        // Plaintext "hello": h=0x68, e=0x65, l=0x6C, l=0x6C, o=0x6F
        // Mask: 01 02 03 04 01
        // Encrypted: 68^01=69='i', 65^02=67='g', 6C^03=6F='o', 6C^04=68='h', 6F^01=6E='n'
        let mut src = BytesMut::from(&[0x81, 0x85, 0x01, 0x02, 0x03, 0x04][..]);
        src.extend_from_slice(&[0x69, 0x67, 0x6F, 0x68, 0x6E]);

        let result = codec.decode(&mut src);
        assert!(result.is_ok());
        let frame = result.unwrap().unwrap();
        assert_eq!(frame, WSFrame::Text("hello".to_string()));
    }

    #[tokio::test]
    async fn test_ws_codec_encode_text() {
        let mut codec = WSCodec {};
        let mut dst = BytesMut::new();

        codec.encode(WSFrame::Text("hello".to_string()), &mut dst).unwrap();

        assert_eq!(dst[0], 0x81); // FIN + text opcode
        assert_eq!(dst[1], 0x05); // length 5
        assert_eq!(&dst[2..], b"hello");
    }

    #[tokio::test]
    async fn test_ws_codec_encode_binary() {
        let mut codec = WSCodec {};
        let mut dst = BytesMut::new();

        codec.encode(WSFrame::Binary(vec![0xFF, 0x00]), &mut dst).unwrap();

        assert_eq!(dst[0], 0x82); // FIN + binary opcode
        assert_eq!(dst[1], 0x02); // length 2
        assert_eq!(&dst[2..], &[0xFF, 0x00]);
    }

    #[tokio::test]
    async fn test_ws_codec_encode_ping() {
        let mut codec = WSCodec {};
        let mut dst = BytesMut::new();

        codec.encode(WSFrame::Ping(vec![1, 2]), &mut dst).unwrap();

        assert_eq!(dst[0], 0x89); // FIN + ping opcode
        assert_eq!(dst[1], 0x02); // length 2
    }

    #[tokio::test]
    async fn test_ws_codec_encode_pong() {
        let mut codec = WSCodec {};
        let mut dst = BytesMut::new();

        codec.encode(WSFrame::Pong(vec![3, 4]), &mut dst).unwrap();

        assert_eq!(dst[0], 0x8A); // FIN + pong opcode
        assert_eq!(dst[1], 0x02); // length 2
    }

    #[tokio::test]
    async fn test_ws_codec_encode_close() {
        let mut codec = WSCodec {};
        let mut dst = BytesMut::new();

        codec.encode(WSFrame::Close(1000, Some("bye".to_string())), &mut dst).unwrap();

        assert_eq!(dst[0], 0x88); // FIN + close opcode
        assert_eq!(dst[1], 0x05); // length 5 (2 for code + 3 for "bye")
        assert_eq!(&dst[2..4], &[0x03, 0xE8]); // 1000 big endian
    }

    #[tokio::test]
    async fn test_ws_codec_encode_extended_length() {
        let mut codec = WSCodec {};
        let mut dst = BytesMut::new();

        let payload = vec![0u8; 200];
        codec.encode(WSFrame::Binary(payload), &mut dst).unwrap();

        assert_eq!(dst[0], 0x82);
        assert_eq!(dst[1], 0x7E); // 16-bit length indicator
        assert_eq!(&dst[2..4], &200u16.to_be_bytes());
    }
}