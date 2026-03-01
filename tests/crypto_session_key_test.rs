#[cfg(test)]
mod tests {
    use aex::crypto::zero_trust_session_key::SessionKey;
    use chrono::Utc;
    use x25519_dalek::PublicKey;


    #[test]
    fn test_full_handshake_and_communication() -> anyhow::Result<()> {
        // 1. 初始化两个对端
        let mut alice = SessionKey::new();
        let mut bob = SessionKey::new();

        // 记录初始时间用于后续验证
        let _ = alice.created_at;

        // 2. 交换公钥并建立会话 (ECDH)
        alice.establish(&bob.ephemeral_public)?;
        bob.establish(&alice.ephemeral_public)?;

        // 3. 验证双方生成的对称密钥是否一致
        assert!(alice.key.is_some());
        assert!(bob.key.is_some());
        assert_eq!(alice.key, bob.key, "Shared secrets must match");

        // 4. 验证私钥是否已被销毁 (take() 逻辑)
        assert!(alice.ephemeral_secret.is_none());
        assert!(bob.ephemeral_secret.is_none());

        // 5. 测试加密与解密
        let message = b"Hello, Zero Trust P2P!";
        
        // Alice 加密
        let ciphertext = alice.encrypt(message)?;
        // 密文长度应为：Nonce(24 bytes) + Tag(16 bytes) + Message(22 bytes) = 62
        assert_eq!(ciphertext.len(), 24 + 16 + message.len());

        // Bob 解密
        let decrypted = bob.decrypt(&ciphertext)?;
        assert_eq!(decrypted, message);

        // 6. 验证 touch 更新时间
        // 稍微等待或手动模拟时间流逝（如果 SystemTime 支持模拟）
        let old_updated_at = alice.updated_at;
        alice.touch();
        assert!(alice.updated_at >= old_updated_at);

        Ok(())
    }

    #[test]
    fn test_decrypt_with_wrong_key() -> anyhow::Result<()> {
        let mut alice = SessionKey::new();
        let mut bob = SessionKey::new();
        let eve = SessionKey::new(); // 攻击者

        alice.establish(&bob.ephemeral_public)?;
        bob.establish(&alice.ephemeral_public)?;
        // Eve 与任何人都没有建立合法的 session

        let message = b"Secret Message";
        let ciphertext = alice.encrypt(message)?;

        // Eve 尝试解密 Alice 的消息，由于没有 key，直接报错
        let result = eve.decrypt(&ciphertext);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().to_string(), "session not established");

        Ok(())
    }

    #[test]
    fn test_tampered_data() -> anyhow::Result<()> {
        let mut alice = SessionKey::new();
        let mut bob = SessionKey::new();
        alice.establish(&bob.ephemeral_public)?;
        bob.establish(&alice.ephemeral_public)?;

        let mut ciphertext = alice.encrypt(b"Original Data")?;
        
        // 篡改密文最后一个字节（通常属于 Poly1305 Tag）
        if let Some(last) = ciphertext.last_mut() {
            *last ^= 0xFF; 
        }

        // 解密必须失败（AEAD 完整性校验）
        let result = bob.decrypt(&ciphertext);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("decrypt failed"));

        Ok(())
    }

    #[test]
    fn test_ciphertext_too_short() -> anyhow::Result<()> {
        let alice = SessionKey {
            key: Some([0u8; 32]),
            ephemeral_secret: None,
            ephemeral_public: PublicKey::from([0u8; 32]),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        let invalid_data = vec![0u8; 23]; // 低于 24 字节 Nonce 长度
        let result = alice.decrypt(&invalid_data);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().to_string(), "ciphertext too short");

        Ok(())
    }
}