use std::{collections::HashMap, sync::Arc};

use anyhow::Ok;
use anyhow::Result;
use anyhow::anyhow;
use chacha20poly1305::aead::{OsRng, rand_core::RngCore};
use tokio::sync::{Mutex, RwLock};
use x25519_dalek::PublicKey;

use crate::{crypto::zero_trust_session_key::SessionKey, time::SystemTime};

pub struct PairedSessionKey {
    pub length: usize,
    pub main: Arc<RwLock<HashMap<Vec<u8>, SessionKey>>>,
    pub temp: Arc<Mutex<HashMap<Vec<u8>, SessionKey>>>, // 临时 session_id → SessionKey
}

impl PairedSessionKey {
    pub fn new(length: usize) -> Self {
        Self {
            length,
            main: Arc::new(RwLock::new(HashMap::new())),
            temp: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn create(&self, is_main: bool) -> (Vec<u8>, PublicKey) {
        let mut session_id = vec![0u8; self.length];
        OsRng.fill_bytes(&mut session_id);

        let session_key = SessionKey::new();
        let ephemeral_public = session_key.ephemeral_public.clone();
        if is_main {
            self.main
                .write()
                .await
                .insert(session_id.clone(), session_key);
        } else {
            self.temp
                .lock()
                .await
                .insert(session_id.clone(), session_key);
        }

        (session_id, ephemeral_public)
    }

    pub async fn save(&self, from: Vec<u8>, to: Vec<u8>) -> Result<()> {
        let mut temp_sessions = self.temp.lock().await;

        let mut session_key = temp_sessions
            .remove(&from)
            .ok_or_else(|| anyhow!("temp session not found"))?;

        // 迁移本身视为一次有效使用
        session_key.touch();

        let mut sk = self.main.write().await;
        sk.insert(to, session_key);

        Ok(())
    }

    pub async fn cleanup(&self, ttl_ms: u128) {
        self.temp
            .lock()
            .await
            .retain(|_, sk| !SystemTime::is_expired(sk.updated_at, ttl_ms));
        self.main
            .write()
            .await
            .retain(|_, sk| !SystemTime::is_expired(sk.updated_at, ttl_ms));
    }

    pub async fn with_session<R>(
        &self,
        key: &Vec<u8>,
        f: impl FnOnce(&mut SessionKey) -> Result<R>,
    ) -> Result<R> {
        let mut sessions = self.main.write().await;

        let sk = sessions
            .get_mut(key)
            .ok_or_else(|| anyhow!("session not found for address"))?;

        // 每次合法使用都 touch
        sk.touch();

        f(sk)
    }
    // 辅助方法：安全地从 Vec 转换到 [u8; 32]
    fn parse_public_key(bytes: &[u8]) -> Result<x25519_dalek::PublicKey> {
        let array: [u8; 32] = bytes
            .get(..32)
            .and_then(|slice| slice.try_into().ok())
            .ok_or_else(|| {
                anyhow!(
                    "Invalid public key length: expected 32, got {}",
                    bytes.len()
                )
            })?;
        Ok(x25519_dalek::PublicKey::from(array))
    }

    /// 完成 session 握手（ACK 阶段）
    pub async fn establish_begins(
        &self,
        id: Vec<u8>,
        remote: &[u8], // 改为切片更通用
    ) -> Result<Option<PublicKey>> {
        let mut session_key = SessionKey::new();
        let ephemeral_public = session_key.ephemeral_public.clone();

        // 安全转换，不再使用 expect
        let client_pub = Self::parse_public_key(remote)?;

        // 执行 Diffie-Hellman
        if let Err(_) = session_key.establish(&client_pub) {
            return Ok(None);
        }

        session_key.touch();
        self.main.write().await.insert(id, session_key);

        Ok(Some(ephemeral_public))
    }

    pub async fn establish_ends(&self, id: Vec<u8>, remote: &[u8]) -> Result<bool> {
        let mut temp_sessions = self.temp.lock().await;

        let mut session = match temp_sessions.remove(&id) {
            Some(s) => s,
            None => return Ok(false),
        };

        // 安全转换
        let peer_pub = Self::parse_public_key(remote)?;

        if let Err(_) = session.establish(&peer_pub) {
            return Ok(false);
        }

        session.touch();

        // 跨锁操作建议：先释放 temp 锁再拿 main 锁，避免潜在死锁
        drop(temp_sessions);

        self.main.write().await.insert(id, session);
        Ok(true)
    }

    /// 使用 session 加密
    pub async fn encrypt(&self, key: &Vec<u8>, plaintext: &[u8]) -> Result<Vec<u8>> {
        let mut sessions = self.main.write().await;

        let sk = sessions
            .get_mut(key)
            .ok_or_else(|| anyhow!("session not found for address"))?;

        let ct = sk.encrypt(plaintext)?;
        Ok(ct)
    }

    /// 使用 session 解密
    pub async fn decrypt(&self, key: &Vec<u8>, data: &[u8]) -> Result<Vec<u8>> {
        let mut sessions = self.main.write().await;

        let sk = sessions
            .get_mut(key)
            .ok_or_else(|| anyhow!("session not found for address"))?;

        let pt = sk.decrypt(data)?;
        Ok(pt)
    }
}
