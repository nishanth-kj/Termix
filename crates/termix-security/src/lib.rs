//! AES-256-GCM secret storage.
//!
//! Passwords and private keys are encrypted before they touch SQLite.
//! The 32-byte master key is random. It is not derived from the secret itself.
//! A user passphrase can wrap that master key with Argon2id.
//!
//! OS keychain backends are not implemented in this revision. On Unix the key
//! file is created with mode 0600 and a looser file is refused. On Windows it
//! relies on the permissions of the user's profile directory. Losing the key
//! file makes stored secrets unrecoverable.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use argon2::{Argon2, Params};
use rand::RngCore;
use zeroize::{Zeroize, Zeroizing};

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
    let wrapper = Vault::from_key(*derive(passphrase, &salt)?);
    let blob = wrapper.encrypt(vault.key_bytes())?;
    Ok(WrappedMasterKey { salt, blob })
}

pub fn unwrap_master_key(
    passphrase: &str,
    wrapped: &WrappedMasterKey,
) -> Result<Vault, CryptoError> {
    let wrapper = Vault::from_key(*derive(passphrase, &wrapped.salt)?);
    let key = Zeroizing::new(wrapper.decrypt(&wrapped.blob)?);
    if key.len() != KEY_LEN {
        return Err(CryptoError::Malformed);
    }
    let mut arr = [0u8; KEY_LEN];
    arr.copy_from_slice(&key);
    Ok(Vault::from_key(arr))
}

fn derive(passphrase: &str, salt: &[u8]) -> Result<Zeroizing<[u8; KEY_LEN]>, CryptoError> {
    let params = Params::new(19_456, 2, 1, Some(KEY_LEN)).map_err(|_| CryptoError::Derive)?;
    let argon = Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);
    let mut out = Zeroizing::new([0u8; KEY_LEN]);
    argon
        .hash_password_into(passphrase.as_bytes(), salt, out.as_mut())
        .map_err(|_| CryptoError::Derive)?;
    Ok(out)
}

/// Create a master key file that only the owner can read on Unix. The mode is
/// set when the file is created, so it is never readable by anyone else, and an
/// existing key is never overwritten.
pub fn store_key_file(path: &std::path::Path, vault: &Vault) -> std::io::Result<()> {
    use std::io::Write;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)?.write_all(vault.key_bytes())
}

/// Load the master key, creating it the first time. Two processes starting
/// together end up with the same key.
pub fn load_or_create_key_file(path: &std::path::Path) -> std::io::Result<Vault> {
    if path.exists() {
        return load_key_file(path);
    }
    let vault = Vault::generate();
    match store_key_file(path, &vault) {
        Ok(()) => Ok(vault),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => load_key_file(path),
        Err(error) => Err(error),
    }
}

pub fn load_key_file(path: &std::path::Path) -> std::io::Result<Vault> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(path)?.permissions().mode();
        if mode & 0o077 != 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                format!(
                    "{} is readable by other users; run chmod 600 on it",
                    path.display()
                ),
            ));
        }
    }
    let bytes = Zeroizing::new(std::fs::read(path)?);
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

    fn temp_key_path(name: &str) -> std::path::PathBuf {
        let mut dir = std::env::temp_dir();
        dir.push(format!("termix-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir.join("master.key")
    }

    #[test]
    fn key_file_is_created_once_and_never_overwritten() {
        let path = temp_key_path("create");
        let first = load_or_create_key_file(&path).unwrap();
        let again = load_or_create_key_file(&path).unwrap();
        assert_eq!(first.key_bytes(), again.key_bytes());
        assert!(store_key_file(&path, &Vault::generate()).is_err());
        assert_eq!(load_key_file(&path).unwrap().key_bytes(), first.key_bytes());
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn short_key_file_is_rejected() {
        let path = temp_key_path("short");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"short").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        assert!(load_key_file(&path).is_err());
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn key_file_mode_is_enforced() {
        use std::os::unix::fs::PermissionsExt;
        let path = temp_key_path("mode");
        load_or_create_key_file(&path).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(load_key_file(&path).is_err());
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
}
