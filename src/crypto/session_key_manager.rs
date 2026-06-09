use std::{collections::HashMap, sync::Arc};

use anyhow::Ok;
use anyhow::Result;
use anyhow::anyhow;
use async_lock::{Mutex, RwLock};
use chacha20poly1305::aead::{OsRng, rand_core::RngCore};
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

    /// 完成 session 握手（接收侧）
    /// `peer_id`：对端地址（initiator），`local_id`：本机地址（responder）
    ///
    /// Only stores the key under `peer_id`. The initiator's `establish_ends`
    /// stores under `peer_id` as well. Since DH is symmetric, both sides
    /// compute the same shared secret, so `main[peer_id]` on the responder
    /// equals `main[peer_id]` on the initiator. The encrypt side looks up
    /// `main[receiver]`, the decrypt side looks up `main[sender]`. No mirroring
    /// needed — avoids overwriting when a node has multiple connections.
    pub async fn establish_begins(
        &self,
        peer_id: Vec<u8>,
        _local_id: Vec<u8>,
        remote: &[u8],
    ) -> Result<Option<PublicKey>> {
        let mut session_key = SessionKey::new();
        let ephemeral_public = session_key.ephemeral_public.clone();

        let client_pub = Self::parse_public_key(remote)?;

        if let Err(_) = session_key.establish(&client_pub) {
            return Ok(None);
        }

        session_key.touch();
        let peer_debug = String::from_utf8(peer_id.clone()).unwrap_or_default();
        let key_dump = session_key.key.map(|k| {
            let v: Vec<String> = k[..4].iter().map(|b| format!("{:02x}", b)).collect();
            v.join("")
        }).unwrap_or_default();
        tracing::info!(
            "🔑 establish_begins: storing main key for peer='{}' key_prefix={:?}",
            peer_debug, key_dump,
        );
        let mut main = self.main.write().await;
        main.insert(peer_id, session_key.duplicate());

        Ok(Some(ephemeral_public))
    }

    pub async fn establish_ends(
        &self,
        temp_id: Vec<u8>,
        peer_id: Vec<u8>,
        _local_id: Vec<u8>,
        remote: &[u8],
    ) -> Result<bool> {
        let mut temp_sessions = self.temp.lock().await;

        let session = match temp_sessions.remove(&temp_id) {
            Some(s) => s,
            None => {
                let id_debug = String::from_utf8(peer_id.clone()).unwrap_or_default();
                tracing::warn!("⚠️ establish_ends: temp session NOT FOUND for peer_id='{}' (temp_id={:?})", id_debug, temp_id);
                return Ok(false);
            }
        };

        let peer_pub = Self::parse_public_key(remote)?;

        let mut session_key = session;
        if let Err(_) = session_key.establish(&peer_pub) {
            return Ok(false);
        }
        session_key.touch();

        drop(temp_sessions);

        let peer_debug = String::from_utf8(peer_id.clone()).unwrap_or_default();
        let key_dump = session_key.key.map(|k| {
            let v: Vec<String> = k[..4].iter().map(|b| format!("{:02x}", b)).collect();
            v.join("")
        }).unwrap_or_default();
        tracing::info!(
            "🔑 establish_ends: storing main key for peer='{}' key_prefix={:?}",
            peer_debug, key_dump,
        );
        let mut main = self.main.write().await;
        main.insert(peer_id, session_key.duplicate());

        Ok(true)
    }

    /// 使用 session 加密
    pub async fn encrypt(&self, key: &Vec<u8>, plaintext: &[u8]) -> Result<Vec<u8>> {
        let mut sessions = self.main.write().await;

        let key_debug = String::from_utf8(key.clone()).unwrap_or_else(|_| format!("{:?}", key));
        tracing::info!("🔐 encrypt: looking up key='{}' (len={}), main has {} entries", key_debug, key.len(), sessions.len());
        for (k, _) in sessions.iter() {
            let kd = String::from_utf8(k.clone()).unwrap_or_else(|_| format!("{:?}", k));
            tracing::info!("  main key: '{}' (len={})", kd, k.len());
        }

        let sk = sessions
            .get_mut(key)
            .ok_or_else(|| anyhow!("session not found for address '{}'", key_debug))?;

        let sk_dump = sk.key.map(|k| {
            let v: Vec<String> = k[..4].iter().map(|b| format!("{:02x}", b)).collect();
            v.join("")
        }).unwrap_or_default();
        tracing::info!("🔐 encrypt: using key_prefix={:?}", sk_dump);

        let ct = sk.encrypt(plaintext)?;
        Ok(ct)
    }

    /// 使用 session 解密
    pub async fn decrypt(&self, key: &Vec<u8>, data: &[u8]) -> Result<Vec<u8>> {
        let mut sessions = self.main.write().await;

        let key_debug = String::from_utf8(key.clone()).unwrap_or_else(|_| format!("{:?}", key));
        tracing::info!("🔓 decrypt: looking up key='{}' (len={}), main has {} entries", key_debug, key.len(), sessions.len());
        for (k, _) in sessions.iter() {
            let kd = String::from_utf8(k.clone()).unwrap_or_else(|_| format!("{:?}", k));
            tracing::info!("  main key: '{}' (len={})", kd, k.len());
        }

        let sk = sessions
            .get_mut(key)
            .ok_or_else(|| anyhow!("session not found for address '{}'", key_debug))?;

        let sk_dump = sk.key.map(|k| {
            let v: Vec<String> = k[..4].iter().map(|b| format!("{:02x}", b)).collect();
            v.join("")
        }).unwrap_or_default();
        tracing::info!("🔓 decrypt: using key_prefix={:?} (data_len={})", sk_dump, data.len());

        let pt = sk.decrypt(data)?;
        Ok(pt)
    }
}
