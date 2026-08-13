//! AES-256-GCM encryption with scrypt key derivation.
//!
//! Format contract (byte-identical to the Go version, so artifacts remain
//! cross-compatible):
//!
//! ```text
//! [salt(32B)][nonce(12B)][ciphertext + 16B GCM tag]
//! ```
//!
//! - KDF: scrypt(N = 32768, r = 8, p = 1) → 32-byte key
//! - Cipher: AES-256-GCM, random salt + random nonce per encryption
//! - Decryption failure with a valid-length payload ⇒ wrong passphrase

use aes_gcm::aead::common::array::typenum::consts::U12;
use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use anyhow::{Context, Result, bail};
use rand::RngCore;
use scrypt::Params;

pub const SALT_SIZE: usize = 32;
pub const NONCE_SIZE: usize = 12;
pub const KEY_SIZE: usize = 32;
pub const GCM_TAG_SIZE: usize = 16;

/// scrypt parameters (OWASP recommended minimum; N = 2^15 = 32768)
pub const SCRYPT_N_LOG: u8 = 15;
pub const SCRYPT_R: u32 = 8;
pub const SCRYPT_P: u32 = 1;

/// Encrypt `data` with a passphrase-derived key.
///
/// Layout of the returned bytes: `[salt(32)][nonce(12)][ciphertext+tag]`.
pub fn encrypt(data: &[u8], passphrase: &str) -> Result<Vec<u8>> {
    // Generate random salt
    let mut salt = [0u8; SALT_SIZE];
    rand::rngs::OsRng.fill_bytes(&mut salt);

    let key = derive_key(passphrase, &salt)?;
    let cipher = Aes256Gcm::new_from_slice(&key).expect("32-byte key is valid");

    // Generate random nonce
    let mut nonce = [0u8; NONCE_SIZE];
    rand::rngs::OsRng.fill_bytes(&mut nonce);

    let ciphertext = cipher
        .encrypt(&nonce_from_slice(&nonce)?, data)
        .map_err(|_| anyhow::anyhow!("encryption failed"))?;

    let mut result = Vec::with_capacity(SALT_SIZE + NONCE_SIZE + ciphertext.len());
    result.extend_from_slice(&salt);
    result.extend_from_slice(&nonce);
    result.extend_from_slice(&ciphertext);
    Ok(result)
}

/// Decrypt bytes previously produced by [`encrypt`].
pub fn decrypt(data: &[u8], passphrase: &str) -> Result<Vec<u8>> {
    let min_len = SALT_SIZE + NONCE_SIZE + GCM_TAG_SIZE;
    if data.len() < min_len {
        bail!("ciphertext too short");
    }

    let salt = &data[..SALT_SIZE];
    let nonce = &data[SALT_SIZE..SALT_SIZE + NONCE_SIZE];
    let ciphertext = &data[SALT_SIZE + NONCE_SIZE..];

    let key = derive_key(passphrase, salt)?;
    let cipher = Aes256Gcm::new_from_slice(&key).expect("32-byte key is valid");

    cipher
        .decrypt(&nonce_from_slice(nonce)?, ciphertext)
        .map_err(|_| anyhow::anyhow!("decryption failed: wrong passphrase?"))
}

/// Build an AES-GCM nonce from raw bytes (12 bytes required).
fn nonce_from_slice(bytes: &[u8]) -> Result<Nonce<U12>> {
    Nonce::<U12>::try_from(bytes).context("invalid nonce length (expected 12 bytes)")
}

/// Derive a 32-byte AES key from a passphrase and salt using scrypt.
fn derive_key(passphrase: &str, salt: &[u8]) -> Result<[u8; KEY_SIZE]> {
    let params = Params::new(SCRYPT_N_LOG, SCRYPT_R, SCRYPT_P)
        .map_err(|e| anyhow::anyhow!("invalid scrypt params: {e}"))?;
    let mut key = [0u8; KEY_SIZE];
    scrypt::scrypt(passphrase.as_bytes(), salt, &params, &mut key)
        .map_err(|e| anyhow::anyhow!("scrypt key derivation failed: {e}"))?;
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let plaintext = b"hello oci-sync, this is a secret message";
        let encrypted = encrypt(plaintext, "correct horse battery staple").unwrap();
        assert_ne!(encrypted, plaintext);
        let decrypted = decrypt(&encrypted, "correct horse battery staple").unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn wrong_passphrase_fails() {
        let encrypted = encrypt(b"secret", "right").unwrap();
        assert!(decrypt(&encrypted, "wrong").is_err());
    }

    #[test]
    fn same_plaintext_different_ciphertext() {
        let a = encrypt(b"same", "p").unwrap();
        let b = encrypt(b"same", "p").unwrap();
        assert_ne!(a, b); // random salt + nonce
    }

    #[test]
    fn layout_is_salt_nonce_ciphertext() {
        let plaintext = b"0123456789abcdef";
        let encrypted = encrypt(plaintext, "p").unwrap();
        assert_eq!(
            encrypted.len(),
            SALT_SIZE + NONCE_SIZE + plaintext.len() + GCM_TAG_SIZE
        );
        assert_eq!(&encrypted[..SALT_SIZE].len(), &SALT_SIZE);
        assert_eq!(
            &encrypted[SALT_SIZE..SALT_SIZE + NONCE_SIZE].len(),
            &NONCE_SIZE
        );
    }

    #[test]
    fn empty_plaintext_roundtrip() {
        let encrypted = encrypt(b"", "p").unwrap();
        assert_eq!(encrypted.len(), SALT_SIZE + NONCE_SIZE + GCM_TAG_SIZE);
        assert_eq!(decrypt(&encrypted, "p").unwrap(), b"");
    }

    #[test]
    fn short_ciphertext_rejected() {
        assert!(decrypt(&[0u8; 10], "p").is_err());
        assert!(decrypt(&[0u8; SALT_SIZE + NONCE_SIZE + 15], "p").is_err());
    }

    #[test]
    fn chinese_passphrase() {
        let data = b"data";
        let encrypted = encrypt(data, "我的密码").unwrap();
        assert_eq!(decrypt(&encrypted, "我的密码").unwrap(), data);
        assert!(decrypt(&encrypted, "错误密码").is_err());
    }
}
