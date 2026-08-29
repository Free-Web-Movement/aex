#[cfg(test)]
mod tests {
    use aex::http::websocket::{WSCodec, WSFrame};
    use aex::tcp::types::{Codec, Command, Frame};
    use bytes::BytesMut;
    use tokio_util::codec::{Decoder, Encoder};

    #[test]
    fn test_ws_frame_command_id_reserved() {
        assert_eq!(WSFrame::ReservedNonControl(0x3, vec![]).id(), 0x3);
        assert_eq!(WSFrame::ReservedNonControl(0x7, vec![]).id(), 0x7);
        assert_eq!(WSFrame::ReservedControl(0xB, vec![]).id(), 0xB);
        assert_eq!(WSFrame::ReservedControl(0xF, vec![]).id(), 0xF);
    }

    #[test]
    fn test_ws_frame_data_reserved_and_close() {
        assert_eq!(*WSFrame::Continuation(vec![3]).data(), vec![3]);
        assert_eq!(*WSFrame::ReservedNonControl(3, vec![1]).data(), vec![1]);
        assert_eq!(*WSFrame::ReservedControl(0xB, vec![2]).data(), vec![2]);
        assert!(WSFrame::Close(1000, Some("x".into())).data().is_empty());
    }

    #[test]
    fn test_ws_frame_payload_reserved_and_continuation() {
        assert_eq!(WSFrame::Continuation(vec![1]).payload(), Some(vec![1]));
        assert_eq!(WSFrame::ReservedNonControl(3, vec![2]).payload(), Some(vec![2]));
        assert_eq!(WSFrame::ReservedControl(0xB, vec![3]).payload(), Some(vec![3]));
        assert_eq!(WSFrame::Close(1000, None).payload(), None);
    }

    #[test]
    fn test_ws_frame_frame_trait_defaults() {
        let f = WSFrame::Binary(vec![1, 2]);
        assert!(Frame::validate(&f));
        assert!(Frame::is_flat(&f));
        let encoded = f.encode().unwrap();
        let sig = f.sign(|bytes| bytes.to_vec());
        assert_eq!(sig, encoded);
        assert!(f.verify(&sig, |_| true));
        assert!(!Command::is_trusted(&f));
    }

    #[test]
    fn test_ws_frame_codec_round_trip() {
        let f = WSFrame::Text("round trip".to_string());
        let encoded = f.encode().unwrap();
        let decoded = WSFrame::decode(&encoded).unwrap();
        assert_eq!(decoded, f);

        let close = WSFrame::Close(1000, Some("bye".to_string()));
        let encoded = close.encode().unwrap();
        let decoded = WSFrame::decode(&encoded).unwrap();
        assert_eq!(decoded, close);
    }

    #[test]
    fn test_ws_frame_serde_round_trip() {
        let f = WSFrame::Close(1000, Some("bye".to_string()));
        let json = serde_json::to_string(&f).unwrap();
        let decoded: WSFrame = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, f);
    }

    #[test]
    fn test_ws_codec_decode_partial_extended_length_16_header() {
        let mut codec = WSCodec {};
        let mut src = BytesMut::from(&[0x82, 0x7E, 0x00][..]);
        assert!(codec.decode(&mut src).unwrap().is_none());
    }

    #[test]
    fn test_ws_codec_decode_partial_extended_length_64_header() {
        let mut codec = WSCodec {};
        let mut src = BytesMut::from(&[0x82, 0x7F, 0, 0, 0, 0, 0, 0, 0][..]);
        assert!(codec.decode(&mut src).unwrap().is_none());
    }

    #[test]
    fn test_ws_codec_decode_partial_payload() {
        let mut codec = WSCodec {};
        let mut src = BytesMut::from(&[0x82, 0x7E, 0x01, 0x2C][..]);
        src.extend_from_slice(&[1, 2, 3, 4, 5]);
        assert!(codec.decode(&mut src).unwrap().is_none());
    }

    #[test]
    fn test_ws_codec_decode_text_invalid_utf8() {
        let mut codec = WSCodec {};
        let mut src = BytesMut::from(&[0x81, 0x02][..]);
        src.extend_from_slice(&[0xFF, 0xFE]);
        assert!(codec.decode(&mut src).is_err());
    }

    #[test]
    fn test_ws_codec_decode_close_empty_payload() {
        let mut codec = WSCodec {};
        let mut src = BytesMut::from(&[0x88, 0x00][..]);
        let frame = codec.decode(&mut src).unwrap().unwrap();
        assert_eq!(frame, WSFrame::Close(1005, None));
    }

    #[test]
    fn test_ws_codec_decode_close_incomplete_status() {
        let mut codec = WSCodec {};
        let mut src = BytesMut::from(&[0x88, 0x01, 0x03][..]);
        assert!(codec.decode(&mut src).is_err());
    }

    #[test]
    fn test_ws_codec_decode_close_with_reason() {
        let mut codec = WSCodec {};
        let mut src = BytesMut::from(&[0x88, 0x05][..]);
        src.extend_from_slice(&[0x03, 0xE8]);
        src.extend_from_slice(b"bye");
        let frame = codec.decode(&mut src).unwrap().unwrap();
        assert_eq!(frame, WSFrame::Close(1000, Some("bye".to_string())));
    }

    #[test]
    fn test_ws_codec_decode_close_invalid_utf8_reason() {
        let mut codec = WSCodec {};
        let mut src = BytesMut::from(&[0x88, 0x03][..]);
        src.extend_from_slice(&[0x03, 0xE8]);
        src.extend_from_slice(&[0xFF]);
        assert!(codec.decode(&mut src).is_err());
    }

    #[test]
    fn test_ws_codec_decode_masked_extended_length_16() {
        let mut codec = WSCodec {};
        let payload: Vec<u8> = (0..300u16).map(|i| (i % 256) as u8).collect();
        let mask = [0x11u8, 0x22, 0x33, 0x44];
        let mut src = BytesMut::with_capacity(8 + payload.len());
        src.extend_from_slice(&[0x82, 0xFE]);
        src.extend_from_slice(&(300u16).to_be_bytes());
        src.extend_from_slice(&mask);
        let masked_payload: Vec<u8> = payload
            .iter()
            .enumerate()
            .map(|(i, b)| b ^ mask[i % 4])
            .collect();
        src.extend_from_slice(&masked_payload);

        let frame = codec.decode(&mut src).unwrap().unwrap();
        assert_eq!(frame, WSFrame::Binary(payload));
    }

    #[test]
    fn test_ws_codec_decode_masked_extended_length_64() {
        let mut codec = WSCodec {};
        let payload: Vec<u8> = (0..1000u16).map(|i| (i % 256) as u8).collect();
        let mask = [0xAAu8, 0xBB, 0xCC, 0xDD];
        let mut src = BytesMut::with_capacity(14 + payload.len());
        src.extend_from_slice(&[0x82, 0xFF]);
        src.extend_from_slice(&(1000u64).to_be_bytes());
        src.extend_from_slice(&mask);
        let masked_payload: Vec<u8> = payload
            .iter()
            .enumerate()
            .map(|(i, b)| b ^ mask[i % 4])
            .collect();
        src.extend_from_slice(&masked_payload);

        let frame = codec.decode(&mut src).unwrap().unwrap();
        assert_eq!(frame, WSFrame::Binary(payload));
    }

    #[test]
    fn test_ws_codec_encode_extended_length_64() {
        let mut codec = WSCodec {};
        let mut dst = BytesMut::new();
        let payload = vec![0xABu8; 70_000];
        codec.encode(WSFrame::Binary(payload), &mut dst).unwrap();

        assert_eq!(dst[0], 0x82);
        assert_eq!(dst[1], 0x7F);
        let len = u64::from_be_bytes(dst[2..10].try_into().unwrap());
        assert_eq!(len, 70_000);
        assert_eq!(dst.len(), 10 + 70_000);
    }

    #[test]
    fn test_ws_codec_encode_continuation() {
        let mut codec = WSCodec {};
        let mut dst = BytesMut::new();
        codec.encode(WSFrame::Continuation(vec![1, 2]), &mut dst).unwrap();
        assert_eq!(dst[0], 0x80);
        assert_eq!(dst[1], 0x02);
        assert_eq!(&dst[2..], &[1, 2]);
    }

    #[test]
    fn test_ws_codec_encode_reserved_non_control() {
        let mut codec = WSCodec {};
        let mut dst = BytesMut::new();
        codec.encode(WSFrame::ReservedNonControl(0x7, vec![0xDE, 0xAD]), &mut dst).unwrap();
        assert_eq!(dst[0], 0x80 | 0x07);
        assert_eq!(dst[1], 0x02);
        assert_eq!(&dst[2..], &[0xDE, 0xAD]);
    }

    #[test]
    fn test_ws_codec_encode_reserved_control() {
        let mut codec = WSCodec {};
        let mut dst = BytesMut::new();
        codec.encode(WSFrame::ReservedControl(0xB, vec![0xCC]), &mut dst).unwrap();
        assert_eq!(dst[0], 0x80 | 0x0B);
        assert_eq!(dst[1], 0x01);
        assert_eq!(&dst[2..], &[0xCC]);
    }

    #[test]
    fn test_ws_codec_encode_close_no_reason() {
        let mut codec = WSCodec {};
        let mut dst = BytesMut::new();
        codec.encode(WSFrame::Close(1001, None), &mut dst).unwrap();
        assert_eq!(dst[0], 0x88);
        assert_eq!(dst[1], 0x02);
        assert_eq!(&dst[2..], &1001u16.to_be_bytes());
    }

    #[test]
    fn test_ws_codec_encode_empty_text() {
        let mut codec = WSCodec {};
        let mut dst = BytesMut::new();
        codec.encode(WSFrame::Text(String::new()), &mut dst).unwrap();
        assert_eq!(dst[0], 0x81);
        assert_eq!(dst[1], 0x00);
        assert_eq!(dst.len(), 2);
    }

    #[test]
    fn test_ws_codec_round_trip_all_variants() {
        let frames = vec![
            WSFrame::Continuation(vec![1, 2, 3]),
            WSFrame::Text("hello ws".to_string()),
            WSFrame::Binary(vec![0, 1, 2, 3]),
            WSFrame::ReservedNonControl(5, vec![9, 9]),
            WSFrame::Close(1000, None),
            WSFrame::Close(1000, Some("reason".to_string())),
            WSFrame::Ping(vec![1]),
            WSFrame::Pong(vec![2]),
            WSFrame::ReservedControl(0xD, vec![3, 3]),
        ];
        for frame in frames {
            let mut codec = WSCodec {};
            let mut dst = BytesMut::new();
            codec.encode(frame.clone(), &mut dst).unwrap();
            let decoded = codec.decode(&mut dst).unwrap().unwrap();
            assert_eq!(decoded, frame);
        }
    }
}
