#[cfg(test)]
mod tests {
    use aex::tcp::types::{Codec, Command, Frame, RawCodec, StreamExecutor, frame_config};
    use bincode::{Decode, Encode, decode_from_slice, encode_to_vec};
    use serde::{Deserialize, Serialize};

    // 为测试创建一个简单的自定义 Command
    #[derive(Serialize, Deserialize, Encode, Decode, Debug, PartialEq, Eq)]
    struct TestCommand {
        pub id: u32,
        pub data: String,
    }
    impl Codec for TestCommand {}
    impl Command for TestCommand {
        fn id(&self) -> u32 {
            self.id
        }
    }

    #[test]
    fn test_codec_encode_decode() {
        let cmd = TestCommand {
            id: 101,
            data: "hello".to_string(),
        };

        // 测试序列化
        let encoded = Codec::encode(&cmd);
        assert!(!encoded.is_empty());

        // 测试反序列化
        let decoded = Codec::decode(&encoded).expect("Decode should work");
        assert_eq!(cmd, decoded);
    }

    #[test]
    fn test_codec_decode_failure() {
        // 提供伪造的损坏数据
        let junk_data = vec![0xFF, 0x00, 0xAA, 0xBB];
        let result: Result<TestCommand, anyhow::Error> = Codec::decode(&junk_data);
        assert!(result.is_err(), "Decoding junk data should fail");
    }

    #[test]
    fn test_raw_codec_implementation() {
        let raw_data = vec![1, 2, 3, 4, 5];
        let raw = RawCodec(raw_data.clone());

        // 测试 Command trait
        assert_eq!(raw.id(), 0);
        assert!(Command::validate(&raw)); // 测试默认实现

        // 测试 Frame trait
        assert_eq!(raw.payload(), Some(raw_data.clone()));
        assert_eq!(raw.command(), Some(raw_data.as_ref()));
        assert!(Frame::validate(&raw)); // 测试 Frame 的默认 validate
    }

    #[test]
    fn test_raw_codec_serialization() {
        let raw = RawCodec(vec![10, 20, 30]);
        let encoded = Codec::encode(&raw);
        let decoded: RawCodec = Codec::decode(&encoded).unwrap();

        assert_eq!(raw.0, decoded.0);
    }

    #[test]
    fn test_trait_default_methods() {
        // 测试 Command 的默认 validate
        struct DummyCmd;
        impl Serialize for DummyCmd {
            fn serialize<S>(&self, _: S) -> std::result::Result<S::Ok, S::Error>
            where
                S: serde::Serializer,
            {
                unreachable!()
            }
        }
        impl<'de> Deserialize<'de> for DummyCmd {
            fn deserialize<D>(_: D) -> std::result::Result<Self, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                unreachable!()
            }
        }
        impl Encode for DummyCmd {
            fn encode<E: bincode::enc::Encoder>(
                &self,
                _: &mut E,
            ) -> std::result::Result<(), bincode::error::EncodeError> {
                Ok(())
            }
        }
        impl Decode<()> for DummyCmd {
            fn decode<D: bincode::de::Decoder<Context = ()>>(
                _: &mut D,
            ) -> std::result::Result<Self, bincode::error::DecodeError> {
                Ok(DummyCmd)
            }
        }
        impl Codec for DummyCmd {}
        impl Command for DummyCmd {
            fn id(&self) -> u32 {
                99
            }
        }

        let dummy = DummyCmd;
        assert!(dummy.validate()); // 覆盖 Command::validate 默认路径
    }

    // 验证 StreamExecutor 类型定义（编译期验证）
    #[test]
    fn test_stream_executor_signature() {
        let _: StreamExecutor = Box::new(|_r, _w| Box::pin(async { Ok(true) }));
    }

    #[test]
    fn test_raw_codec_roundtrip() {
        let original_data = vec![1, 2, 3, 4, 0xFF];
        let raw = RawCodec(original_data.clone());

        // 1. 调用实例方法 encode
        // 这里的 self 是 &RawCodec，会匹配到 impl Codec for RawCodec 的默认实现
        // let encoded = raw.encode();
        let encoded = <RawCodec as Codec>::encode(&raw);
        assert!(!encoded.is_empty());

        // 2. 调用关联函数 decode
        // 使用完全限定语法确保调用的是 Codec trait 里的实现
        let decoded = <RawCodec as Codec>::decode(&encoded).expect("RawCodec should be decodable");

        // 3. 验证数据和接口
        assert_eq!(decoded.0, original_data);
        assert_eq!(decoded.id(), 0);
        assert_eq!(decoded.payload(), Some(original_data));
    }

    #[test]
    fn test_raw_codec_frame_handle() {
        let data = vec![10, 20];
        let raw = RawCodec(data.clone());

        // 覆盖 Frame trait 的 handle 方法
        let handled_data = raw.command();
        assert_eq!(handled_data, Some(data.as_ref()));

        // 覆盖默认的 validate 实现
        assert!(Frame::validate(&raw));
        assert!(Command::validate(&raw));
    }

    #[test]
    fn test_codec_decode_error_handling() {
        // 构造一个非法的数据片段（例如对于 Vec 来说长度前缀不完整的数据）
        let malformed = vec![0x81];

        // 尝试解码，这会触发 Codec::decode 中的 map_err
        let result = <RawCodec as Codec>::decode(&malformed);

        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        // 验证是否包含你定义的错误前缀
        assert!(err_msg.contains("decode failed"));
    }

    #[test]
    fn test_raw_codec_derive_encode_directly() {
        let data = vec![1, 2, 3, 4];
        let raw = RawCodec(data.clone());

        // 1. 直接调用 bincode 宏生成的 Encode 逻辑
        // 注意：这里我们直接使用 bincode 的方法，而不是你 trait 里的包装方法
        let config = frame_config();
        let encoded =
            encode_to_vec(&raw, config).expect("Derived Encode should work with RawCodec");

        assert!(!encoded.is_empty());

        // 2. 直接调用 bincode 宏生成的 Decode 逻辑
        // 验证宏生成的代码能否正确识别并还原 Vec<u8>
        let (decoded, len): (RawCodec, usize) =
            decode_from_slice(&encoded, config).expect("Derived Decode should work with RawCodec");

        assert_eq!(len, encoded.len());
        assert_eq!(decoded.0, data);
    }

    #[test]
    fn test_raw_codec_derive_consistency() {
        let raw = RawCodec(vec![10, 20, 30]);

        // 验证你手写的 Codec::encode 结果是否与直接调用 bincode 结果一致
        // 这确保了你的 trait 逻辑没有意外地修改数据流
        // let trait_encoded = raw.encode();
        let trait_encoded = Codec::encode(&raw);
        let direct_encoded = encode_to_vec(&raw, frame_config()).unwrap();

        assert_eq!(
            trait_encoded, direct_encoded,
            "Trait encoding must match derived encoding"
        );

        let signature = raw.sign(|bytes| bytes.to_vec());

        assert!(raw.verify(&signature, |_| true));
    }
}
