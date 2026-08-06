use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit},
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::error::ApiError;

pub fn encrypt_value(secret: &str, value: &str) -> Result<String, ApiError> {
    let cipher = token_cipher(secret)?;
    let nonce_bytes = random_nonce();
    let ciphertext = cipher
        .encrypt(Nonce::from_slice(&nonce_bytes), value.as_bytes())
        .map_err(|_| ApiError::service_unavailable("数据加密失败，请检查后台加密配置"))?;

    Ok(format!(
        "v1:{}:{}",
        STANDARD.encode(nonce_bytes),
        STANDARD.encode(ciphertext)
    ))
}

pub fn decrypt_value(secret: &str, value: &str) -> Result<String, ApiError> {
    let Some(encoded) = value.strip_prefix("v1:") else {
        tracing::warn!("数据不是 v1 加密格式，按历史明文返回");
        return Ok(value.to_string());
    };
    let (nonce, ciphertext) = encoded
        .split_once(':')
        .ok_or_else(|| ApiError::unauthorized("数据解密失败，请重新授权"))?;
    let nonce = STANDARD
        .decode(nonce)
        .map_err(|_| ApiError::unauthorized("数据解密失败，请重新授权"))?;
    if nonce.len() != 12 {
        return Err(ApiError::unauthorized("数据解密失败，请重新授权"));
    }
    let ciphertext = STANDARD
        .decode(ciphertext)
        .map_err(|_| ApiError::unauthorized("数据解密失败，请重新授权"))?;

    let cipher = token_cipher(secret)?;
    let plaintext = cipher
        .decrypt(Nonce::from_slice(&nonce), ciphertext.as_ref())
        .map_err(|_| ApiError::unauthorized("数据解密失败，请重新授权"))?;

    String::from_utf8(plaintext).map_err(|_| ApiError::unauthorized("数据解密失败，请重新授权"))
}

fn token_cipher(secret: &str) -> Result<Aes256Gcm, ApiError> {
    if secret.trim().is_empty() {
        return Err(ApiError::service_unavailable(
            "后台加密密钥未配置，无法保存数据",
        ));
    }

    Aes256Gcm::new_from_slice(&token_key(secret))
        .map_err(|_| ApiError::service_unavailable("后台加密密钥无效"))
}

fn token_key(secret: &str) -> [u8; 32] {
    let digest = Sha256::digest(secret.as_bytes());
    let mut key = [0_u8; 32];
    key.copy_from_slice(&digest);
    key
}

fn random_nonce() -> [u8; 12] {
    let uuid = Uuid::new_v4();
    let mut nonce = [0_u8; 12];
    nonce.copy_from_slice(&uuid.as_bytes()[..12]);
    nonce
}

#[cfg(test)]
mod tests {
    use super::{decrypt_value, encrypt_value};

    const TEST_SECRET: &str = "test-encryption-secret";

    #[test]
    fn encrypts_and_decrypts_roundtrip() {
        let encrypted = encrypt_value(TEST_SECRET, "secret-cdk-123").expect("encrypt");
        assert!(encrypted.starts_with("v1:"));

        let decrypted = decrypt_value(TEST_SECRET, &encrypted).expect("decrypt");
        assert_eq!(decrypted, "secret-cdk-123");
    }

    #[test]
    fn decrypts_legacy_plaintext_without_error() {
        let decrypted = decrypt_value(TEST_SECRET, "legacy-plain-cdk").expect("decrypt plaintext");
        assert_eq!(decrypted, "legacy-plain-cdk");
    }

    #[test]
    fn encryption_changes_with_each_call() {
        let first = encrypt_value(TEST_SECRET, "same-value").expect("encrypt first");
        let second = encrypt_value(TEST_SECRET, "same-value").expect("encrypt second");
        assert_ne!(first, second);
    }
}
