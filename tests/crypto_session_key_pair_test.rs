#[cfg(test)]
mod tests {
    use aex::crypto::session_key_manager::PairedSessionKey;
    use anyhow::Result;
    use chacha20poly1305::aead::OsRng;
    use std::time::Duration;
    use tokio::time::sleep;
    use x25519_dalek::{EphemeralSecret, PublicKey, StaticSecret};

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
        let (temp_id, _) = manager.create(false).await;
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
            assert!(
                main_lock.contains_key(&main_key),
                "Main should have the key"
            );
        }

        // 3. 测试 Establish: 握手确认
        manager
            .establish_begins(main_key.clone(), &peer_public.as_bytes().to_vec())
            .await?;

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
        let (temp_id, _) = manager.create(true).await;

        // 等待一小会儿确保时间戳有差异
        sleep(Duration::from_millis(10)).await;

        // 清理 TTL 为 5ms 的 session (这应该会清理掉刚才创建的)
        manager.cleanup(5).await;

        let main_lock = manager.main.read().await;
        assert!(
            !main_lock.contains_key(&temp_id),
            "Expired session should be removed"
        );
    }

    #[tokio::test]
    async fn test_with_session_callback() -> Result<()> {
        let manager = PairedSessionKey::new(16);
        let (key, _) = manager.create(true).await;

        // 测试自定义回调修改 SessionKey
        manager
            .with_session(&key, |_sk| {
                // 假设 SessionKey 有内部状态可以验证
                println!("Inside callback for session!");
                Ok(())
            })
            .await?;

        Ok(())
    }

    #[tokio::test]
    async fn test_session_not_found_errors() {
        let manager = PairedSessionKey::new(16);
        let fake_key = vec![0u8; 16];

        let result = manager.decrypt(&fake_key, b"data").await;
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().to_string(),
            "session not found for address"
        );
    }

    #[tokio::test]
    async fn test_full_handshake_and_communication() -> Result<()> {
        // 1. 初始化管理器（假设 session_id 长度为 16）
        let manager = PairedSessionKey::new(16);

        // 模拟客户端生成的临时公钥
        let client_secret = EphemeralSecret::random_from_rng(OsRng);
        let client_pub = PublicKey::from(&client_secret);
        let client_pub_bytes = client_pub.as_bytes().to_vec();

        // 2. 服务端：收到客户端请求，开始建立 session (establish_begins)
        let session_id = vec![1u8; 16]; // 模拟固定的 session_id
        let server_pub_opt = manager
            .establish_begins(session_id.clone(), &client_pub_bytes)
            .await?;

        assert!(server_pub_opt.is_some(), "握手应该成功生成服务端公钥");
        let _ = server_pub_opt.unwrap();

        // 3. 验证加密：此时 SessionKey 内部应该已经生成了对称密钥
        let plaintext = b"Hello Zero Trust";

        // 加密
        let ciphertext = manager.encrypt(&session_id, plaintext).await?;
        assert_ne!(plaintext.to_vec(), ciphertext, "加密后的数据不应与原文相同");

        // 解密
        let decrypted = manager.decrypt(&session_id, &ciphertext).await?;
        assert_eq!(plaintext.to_vec(), decrypted, "解密后的数据应与原文一致");

        Ok(())
    }

    #[tokio::test]
    async fn test_session_migration() -> Result<()> {
        let manager = PairedSessionKey::new(16);

        // 1. 创建一个临时 session (is_main = false)
        let (temp_id, _) = manager.create(false).await;

        {
            let temp_map = manager.temp.lock().await;
            assert!(temp_map.contains_key(&temp_id), "临时表中应包含该 ID");
        }

        // 2. 迁移到正式表并更换 ID (比如从连接 ID 变为 用户 ID)
        let formal_id = vec![2u8; 16];
        manager.save(temp_id.clone(), formal_id.clone()).await?;

        // 3. 验证状态
        let temp_map = manager.temp.lock().await;
        let main_map = manager.main.read().await;

        assert!(!temp_map.contains_key(&temp_id), "迁移后临时表应为空");
        assert!(main_map.contains_key(&formal_id), "正式表中应包含新 ID");

        Ok(())
    }

    #[tokio::test]
    async fn test_session_cleanup() -> Result<()> {
        let manager = PairedSessionKey::new(16);

        // 创建一个 session
        let (id, _) = manager.create(true).await;

        // 立即清理（TTL 为 0），应该会被移除
        manager.cleanup(0).await;

        let main_map = manager.main.read().await;
        assert!(!main_map.contains_key(&id), "超时的 Session 应该被清除");

        Ok(())
    }

    #[tokio::test]
    async fn test_establish_ends_flow() -> Result<()> {
        let manager = PairedSessionKey::new(16);
        
        // 1. 模拟初始化阶段：服务端生成临时 Session 并发给客户端自己的公钥
        // 此时 is_main = false，SessionKey 存在 temp 中
        let (session_id, server_pub) = manager.create(false).await;
        
        {
            let temp_map = manager.temp.lock().await;
            assert!(temp_map.contains_key(&session_id), "应该在临时表中");
        }

        // 2. 模拟客户端响应：客户端收到 server_pub，生成自己的公钥 client_pub 并发回
        let client_secret = EphemeralSecret::random_from_rng(OsRng);
        let client_pub = PublicKey::from(&client_secret);
        let client_pub_bytes = client_pub.as_bytes();

        // 3. 服务端调用 establish_ends：处理客户端公钥，并将 session 移至 main
        let success = manager.establish_ends(session_id.clone(), client_pub_bytes).await?;
        assert!(success, "握手结束阶段应返回 true");

        // 4. 验证状态迁移
        {
            let temp_map = manager.temp.lock().await;
            let main_map = manager.main.read().await;
            
            assert!(!temp_map.contains_key(&session_id), "临时表应该已经移除该 Session");
            assert!(main_map.contains_key(&session_id), "正式表应该已经存入该 Session");
            
            // 验证 key 字段是否已经从 None 变成了 Some（即握手完成，对称密钥已生成）
            let sk = main_map.get(&session_id).unwrap();
            assert!(sk.key.is_some(), "对称密钥 session_key 应该已经生成");
        }

        // 5. 验证是否可以正常加密
        let plaintext = b"End-to-end encrypted message";
        let ciphertext = manager.encrypt(&session_id, plaintext).await?;
        let decrypted = manager.decrypt(&session_id, &ciphertext).await?;
        assert_eq!(plaintext.to_vec(), decrypted);

        Ok(())
    }
}
