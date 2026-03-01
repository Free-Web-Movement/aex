#[cfg(test)]
mod tests {
    use aex::crypto::session_key_manager::PairedSessionKey;
    use chacha20poly1305::aead::OsRng;
    use x25519_dalek::{PublicKey, StaticSecret};
    use std::time::Duration;
    use tokio::time::sleep;
    use anyhow::Result;
    

    // 辅助函数：生成一个随机的 PublicKey
    fn generate_peer_public() -> PublicKey {
        let secret = StaticSecret::random_from_rng(OsRng);
        PublicKey::from(&secret)
    }

    #[tokio::test]
    async fn test_session_lifecycle() -> Result<()> {
        let manager = PairedSessionKey::new(16);
        let peer_public = generate_peer_public();
        
        // 1. 测试 Create: 在 temp 中创建 session
        let temp_id = manager.create(&manager.temp).await;
        assert_eq!(temp_id.len(), 16);
        {
            let temp_lock = manager.temp.lock().await;
            assert!(temp_lock.contains_key(&temp_id));
        }

        // 2. 测试 Save: 从 temp 迁移到 main (例如以公钥地址为 key)
        let main_key = vec![1, 2, 3, 4];
        manager.save(temp_id.clone(), main_key.clone()).await?;

        {
            let temp_lock = manager.temp.lock().await;
            let main_lock = manager.main.read().await;
            assert!(!temp_lock.contains_key(&temp_id), "Temp should be cleared");
            assert!(main_lock.contains_key(&main_key), "Main should have the key");
        }

        // 3. 测试 Establish: 握手确认
        manager.session_establish(&main_key, &peer_public).await?;

        // 4. 测试加解密
        let message = b"Hello Zero Trust";
        let ciphertext = manager.encrypt(&main_key, message).await?;
        let decrypted = manager.decrypt(&main_key, &ciphertext).await?;
        
        assert_eq!(message.to_vec(), decrypted);

        Ok(())
    }

    #[tokio::test]
    async fn test_cleanup_expiry() {
        let manager = PairedSessionKey::new(16);

        // 模拟创建一个 session
        let _ = manager.create(&manager.temp).await; // 直接存入 main 测试清理
        let temp_id = manager.add(&manager.main).await; // 直接存入 main 测试清理
        
        // 等待一小会儿确保时间戳有差异
        sleep(Duration::from_millis(10)).await;

        // 清理 TTL 为 5ms 的 session (这应该会清理掉刚才创建的)
        manager.cleanup(5).await;

        let main_lock = manager.main.read().await;
        assert!(!main_lock.contains_key(&temp_id), "Expired session should be removed");
    }

    #[tokio::test]
    async fn test_with_session_callback() -> Result<()> {
        let manager = PairedSessionKey::new(16);
        let key = manager.add(&manager.main).await;

        // 测试自定义回调修改 SessionKey
        manager.with_session(&key, |_sk| {
            // 假设 SessionKey 有内部状态可以验证
            println!("Inside callback for session!");
            Ok(())
        }).await?;

        Ok(())
    }

    #[tokio::test]
    async fn test_session_not_found_errors() {
        let manager = PairedSessionKey::new(16);
        let fake_key = vec![0u8; 16];

        let result = manager.decrypt(&fake_key, b"data").await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().to_string(), "session not found for address");
    }
}