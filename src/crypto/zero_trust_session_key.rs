use anyhow::{Result, anyhow};
use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, KeyInit},
};
use chrono::{DateTime, Utc};
use x25519_dalek::{EphemeralSecret, PublicKey};

use crate::time::SystemTime;

pub struct SessionKey {
    pub key: Option<[u8; 32]>,                     // 对称 session_key
    pub ephemeral_secret: Option<EphemeralSecret>, // 一次性
    pub ephemeral_public: PublicKey,               // 可缓存
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl SessionKey {
    pub fn new() -> Self {
        let secret = EphemeralSecret::random();
        let public = PublicKey::from(&secret);

        Self {
            key: None,
            ephemeral_secret: Some(secret),
            ephemeral_public: public,
            created_at: SystemTime::now(),
            updated_at: SystemTime::now(),
        }
    }

    #[inline]
    pub fn touch(&mut self) {
        self.updated_at = SystemTime::now();
    }

    pub fn establish(&mut self, peer_public: &PublicKey) -> Result<()> {
        let secret = self
            .ephemeral_secret
            .take()
            .ok_or_else(|| anyhow!("session already established"))?;

        let shared = secret.diffie_hellman(peer_public);

        let mut key = [0u8; 32];
        key.copy_from_slice(&shared.as_bytes()[..32]);
        self.key = Some(key);

        Ok(())
    }

    pub fn encrypt(&self, plaintext: &[u8]) -> Result<Vec<u8>> {
        let key = self.key.ok_or_else(|| anyhow!("session not established"))?;
        let cipher = XChaCha20Poly1305::new(&key.into());

        let mut nonce_bytes = [0u8; 24];
        getrandom::fill(&mut nonce_bytes).expect("OS RNG");

        let nonce = XNonce::try_from(nonce_bytes.as_slice()).expect("24-byte nonce");
        let ct = cipher
            .encrypt(&nonce, plaintext)
            .map_err(|_| anyhow!("encrypt failed"))?;

        Ok([nonce_bytes.to_vec(), ct].concat())
    }

    pub fn decrypt(&self, data: &[u8]) -> Result<Vec<u8>> {
        let key = self.key.ok_or_else(|| anyhow!("session not established"))?;

        if data.len() < 24 {
            return Err(anyhow!("ciphertext too short"));
        }

        let (nonce_bytes, ct) = data.split_at(24);
        let cipher = XChaCha20Poly1305::new(&key.into());

        let nonce = XNonce::try_from(nonce_bytes).expect("24-byte nonce");
        cipher
            .decrypt(&nonce, ct)
            .map_err(|_| anyhow!("decrypt failed"))
    }

    /// Create a copy with the same derived key (no ephemeral_secret).
    /// Used to mirror the session under a second address key.
    pub fn duplicate(&self) -> Self {
        Self {
            key: self.key,
            ephemeral_secret: None,
            ephemeral_public: self.ephemeral_public,
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}
