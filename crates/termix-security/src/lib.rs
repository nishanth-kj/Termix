//! AES-256-GCM secret storage.
//!
//! Passwords and private keys are encrypted before they touch SQLite.
//! The 32-byte master key is random. It is not derived from the secret itself.
//! A user passphrase can wrap that master key with Argon2id.
//!
//! OS keychain backends are not implemented in this revision. The master key
//! file must be created with mode 0600 by the caller. Losing the key file
//! makes stored secrets unrecoverable.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use argon2::{Argon2, Params};
use rand::RngCore;
use zeroize::Zeroize;

const NONCE_LEN: usize = 12;
const KEY_LEN: usize = 32;
const SALT_LEN: usize = 16;

#[derive(Debug, thiserror::Error)]
pub enum CryptoError {
    #[error("ciphertext is malformed")]
    Malformed,
    #[error("decryption failed")]
    Decrypt,
    #[error("key derivation failed")]
    Derive,
}

#[derive(Clone)]
pub struct Vault {
    key: [u8; KEY_LEN],
}

impl Drop for Vault {
    fn drop(&mut self) {
        self.key.zeroize();
    }
}

impl Vault {
    pub fn generate() -> Self {
        let mut key = [0u8; KEY_LEN];
        rand::rngs::OsRng.fill_bytes(&mut key);
        Self { key }
    }

    pub fn from_key(key: [u8; KEY_LEN]) -> Self {
        Self { key }
    }

    pub fn key_bytes(&self) -> &[u8; KEY_LEN] {
        &self.key
    }

    pub fn encrypt(&self, plaintext: &[u8]) -> Result<String, CryptoError> {
        let cipher = Aes256Gcm::new_from_slice(&self.key).map_err(|_| CryptoError::Malformed)?;
        let mut nonce_bytes = [0u8; NONCE_LEN];
        rand::rngs::OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);
        let mut ciphertext = cipher
            .encrypt(nonce, plaintext)
            .map_err(|_| CryptoError::Decrypt)?;
        let mut packed = nonce_bytes.to_vec();
        packed.append(&mut ciphertext);
        Ok(base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            packed,
        ))
    }

    pub fn decrypt(&self, blob: &str) -> Result<Vec<u8>, CryptoError> {
        let packed = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, blob)
            .map_err(|_| CryptoError::Malformed)?;
        if packed.len() <= NONCE_LEN {
            return Err(CryptoError::Malformed);
        }
        let (nonce_bytes, ciphertext) = packed.split_at(NONCE_LEN);
        let cipher = Aes256Gcm::new_from_slice(&self.key).map_err(|_| CryptoError::Malformed)?;
        cipher
            .decrypt(Nonce::from_slice(nonce_bytes), ciphertext)
            .map_err(|_| CryptoError::Decrypt)
    }
}

#[derive(Clone)]
pub struct WrappedMasterKey {
    pub salt: [u8; SALT_LEN],
    pub blob: String,
}

/// Wrap a random master key with a passphrase. The passphrase is not stored.
pub fn wrap_master_key(passphrase: &str, vault: &Vault) -> Result<WrappedMasterKey, CryptoError> {
    let mut salt = [0u8; SALT_LEN];
    rand::rngs::OsRng.fill_bytes(&mut salt);
    let kek = derive(passphrase, &salt)?;
    let wrapper = Vault::from_key(kek);
    let blob = wrapper.encrypt(vault.key_bytes())?;
    Ok(WrappedMasterKey { salt, blob })
}

pub fn unwrap_master_key(
    passphrase: &str,
    wrapped: &WrappedMasterKey,
) -> Result<Vault, CryptoError> {
    let kek = derive(passphrase, &wrapped.salt)?;
    let wrapper = Vault::from_key(kek);
    let key = wrapper.decrypt(&wrapped.blob)?;
    if key.len() != KEY_LEN {
        return Err(CryptoError::Malformed);
    }
    let mut arr = [0u8; KEY_LEN];
    arr.copy_from_slice(&key);
    Ok(Vault::from_key(arr))
}

fn derive(passphrase: &str, salt: &[u8]) -> Result<[u8; KEY_LEN], CryptoError> {
    let params = Params::new(19_456, 2, 1, Some(KEY_LEN)).map_err(|_| CryptoError::Derive)?;
    let argon = Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);
    let mut out = [0u8; KEY_LEN];
    argon
        .hash_password_into(passphrase.as_bytes(), salt, &mut out)
        .map_err(|_| CryptoError::Derive)?;
    Ok(out)
}

/// Write a master key so only the owner can read it on Unix.
pub fn store_key_file(path: &std::path::Path, vault: &Vault) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, vault.key_bytes())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

pub fn load_key_file(path: &std::path::Path) -> std::io::Result<Vault> {
    let bytes = std::fs::read(path)?;
    if bytes.len() != KEY_LEN {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "master key must be 32 bytes",
        ));
    }
    let mut key = [0u8; KEY_LEN];
    key.copy_from_slice(&bytes);
    Ok(Vault::from_key(key))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let vault = Vault::generate();
        let blob = vault.encrypt(b"secret-password").unwrap();
        assert_eq!(vault.decrypt(&blob).unwrap(), b"secret-password");
        let other = Vault::generate();
        assert!(other.decrypt(&blob).is_err());
    }

    #[test]
    fn passphrase_wrap() {
        let vault = Vault::generate();
        let wrapped = wrap_master_key("correct horse", &vault).unwrap();
        let opened = unwrap_master_key("correct horse", &wrapped).unwrap();
        let blob = vault.encrypt(b"key-material").unwrap();
        assert_eq!(opened.decrypt(&blob).unwrap(), b"key-material");
        assert!(unwrap_master_key("wrong", &wrapped).is_err());
    }
}
