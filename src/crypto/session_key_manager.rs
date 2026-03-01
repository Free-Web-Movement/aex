use std::{collections::HashMap, sync::Arc};

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

    pub async fn create(&self, sk: &Arc<Mutex<HashMap<Vec<u8>, SessionKey>>>) -> Vec<u8> {
        let mut session_id = vec![0u8; self.length];
        OsRng.fill_bytes(&mut session_id);

        let session_key = SessionKey::new();

        sk.lock().await.insert(session_id.clone(), session_key);

        session_id
    }

    pub async fn add(&self, sk: &Arc<RwLock<HashMap<Vec<u8>, SessionKey>>>) -> Vec<u8> {
        let mut session_id = vec![0u8; self.length];
        OsRng.fill_bytes(&mut session_id);

        let session_key = SessionKey::new();

        sk.write().await.insert(session_id.clone(), session_key);

        session_id
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

    /// 完成 session 握手（ACK 阶段）
    pub async fn session_establish(&self, key: &Vec<u8>, peer_public: &PublicKey) -> Result<()> {
        let mut sessions = self.main.write().await;

        let sk = sessions
            .get_mut(key)
            .ok_or_else(|| anyhow!("session not found for address"))?;

        sk.establish(peer_public)?;
        sk.touch();

        Ok(())
    }

    /// 使用 session 加密
    pub async fn encrypt(&self, key: &Vec<u8>, plaintext: &[u8]) -> Result<Vec<u8>> {
        let mut sessions = self.main.write().await;

        let sk = sessions
            .get_mut(key)
            .ok_or_else(|| anyhow!("session not found for address"))?;

        let ct = sk.encrypt(plaintext)?;
        sk.touch();

        Ok(ct)
    }

    /// 使用 session 解密
    pub async fn decrypt(&self, key: &Vec<u8>, data: &[u8]) -> Result<Vec<u8>> {
        let mut sessions = self.main.write().await;

        let sk = sessions
            .get_mut(key)
            .ok_or_else(|| anyhow!("session not found for address"))?;

        let pt = sk.decrypt(data)?;
        sk.touch();

        Ok(pt)
    }
}
