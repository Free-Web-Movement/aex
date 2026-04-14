#[cfg(test)]
mod tests {
    use aex::crypto::session_key_manager::PairedSessionKey;
    use aex::crypto::zero_trust_session_key::SessionKey;
    use std::sync::Arc;

    #[test]
    fn test_session_key_creation() {
        let key = SessionKey::new();
        assert!(!key.ephemeral_public.as_bytes().is_empty());
    }

    #[test]
    fn test_session_key_public_key() {
        let key = SessionKey::new();
        assert!(key.ephemeral_public.as_bytes().len() == 32);
    }

    #[test]
    fn test_paired_session_key_new() {
        let paired = PairedSessionKey::new(16);
        assert_eq!(paired.length, 16);
    }

    #[tokio::test]
    async fn test_paired_session_key_create_main() {
        let paired = PairedSessionKey::new(16);
        let (session_id, public_key) = paired.create(true).await;
        
        assert_eq!(session_id.len(), 16);
        assert_eq!(public_key.as_bytes().len(), 32);
    }

    #[tokio::test]
    async fn test_paired_session_key_create_temp() {
        let paired = PairedSessionKey::new(16);
        let (session_id, public_key) = paired.create(false).await;
        
        assert_eq!(session_id.len(), 16);
        assert_eq!(public_key.as_bytes().len(), 32);
    }

    #[tokio::test]
    async fn test_paired_session_key_save() {
        let paired = PairedSessionKey::new(16);
        
        let (temp_id, _) = paired.create(false).await;
        let main_id = vec![0u8; 16];
        
        let result = paired.save(temp_id.clone(), main_id).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_paired_session_key_with_session() {
        let paired = PairedSessionKey::new(16);
        
        let (main_id, _) = paired.create(true).await;
        
        let result = paired.with_session(&main_id, |sk| {
            Ok(sk.ephemeral_public.as_bytes().len())
        }).await;
        
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 32);
    }

    #[tokio::test]
    async fn test_paired_session_key_establish_begins() {
        let paired = PairedSessionKey::new(16);
        
        let id = vec![1u8; 16];
        let remote = vec![0u8; 32];
        
        let result = paired.establish_begins(id, &remote).await;
        assert!(result.is_ok());
    }

    #[test]
    fn test_crypto_exports() {
        let _ = aex::crypto::session_key_manager::PairedSessionKey::new(16);
        let _ = aex::crypto::zero_trust_session_key::SessionKey::new();
    }
}