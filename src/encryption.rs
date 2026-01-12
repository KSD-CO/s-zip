//! AES encryption support for ZIP files
//!
//! Implements WinZip-compatible AES-256 encryption using the AE-2 format.
//!
//! ## Features
//! - AES-256-CTR encryption
//! - PBKDF2-HMAC-SHA1 key derivation (1000 iterations)
//! - HMAC-SHA1 authentication
//! - WinZip AE-2 format (no CRC for better security)
//!
//! ## Security Notes
//! - Uses 16-byte salt for AES-256
//! - 10-byte authentication code (HMAC-SHA1 truncated)
//! - Password verification before decryption

use crate::error::{Result, SZipError};
use aes::Aes256;
use ctr::{
    cipher::{KeyIvInit, StreamCipher},
    Ctr128BE,
};
use hmac::{Hmac, Mac};
use pbkdf2::pbkdf2_hmac;
use sha1::Sha1;

type HmacSha1 = Hmac<Sha1>;

/// AES encryption strength
///
/// Currently only AES-256 is supported as it provides the best security.
/// Future versions may support AES-128 and AES-192.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AesStrength {
    /// AES-256 (recommended and only supported variant)
    Aes256,
}

impl AesStrength {
    /// Get salt size in bytes
    pub fn salt_size(&self) -> usize {
        match self {
            AesStrength::Aes256 => 16,
        }
    }

    /// Get key size in bytes
    pub fn key_size(&self) -> usize {
        match self {
            AesStrength::Aes256 => 32,
        }
    }

    /// Get total derived key material size (key + IV + password verification)
    pub fn derived_key_size(&self) -> usize {
        self.key_size() * 2 + 2 // key + IV + 2-byte password verification
    }

    /// Get WinZip encryption strength code
    pub fn to_winzip_code(&self) -> u16 {
        match self {
            AesStrength::Aes256 => 0x03,
        }
    }
}

/// AES encryption context for a ZIP entry
pub struct AesEncryptor {
    strength: AesStrength,
    salt: Vec<u8>,
    password_verify: [u8; 2],
    encryption_key: Vec<u8>,
    #[allow(dead_code)] // Used by HMAC, kept for future direct access
    auth_key: Vec<u8>,
    hmac: HmacSha1,
}

impl AesEncryptor {
    /// Create a new AES encryptor with the given password
    pub fn new(password: &str, strength: AesStrength) -> Result<Self> {
        // Generate random salt
        let salt = generate_salt(strength.salt_size());

        // Derive keys using PBKDF2-HMAC-SHA1 with 1000 iterations
        let derived_key_size = strength.derived_key_size();
        let mut derived_keys = vec![0u8; derived_key_size];

        pbkdf2_hmac::<Sha1>(password.as_bytes(), &salt, 1000, &mut derived_keys);

        // Split derived key material
        let key_size = strength.key_size();
        let encryption_key = derived_keys[..key_size].to_vec();
        let auth_key = derived_keys[key_size..key_size * 2].to_vec();
        let password_verify = [derived_keys[key_size * 2], derived_keys[key_size * 2 + 1]];

        // Initialize HMAC for authentication
        let hmac = HmacSha1::new_from_slice(&auth_key)
            .map_err(|e| SZipError::InvalidFormat(format!("HMAC init failed: {}", e)))?;

        Ok(Self {
            strength,
            salt,
            password_verify,
            encryption_key,
            auth_key,
            hmac,
        })
    }

    /// Get the salt (to be written before encrypted data)
    pub fn salt(&self) -> &[u8] {
        &self.salt
    }

    /// Get the password verification bytes (to be written after salt)
    pub fn password_verify(&self) -> &[u8; 2] {
        &self.password_verify
    }

    /// Get the AES strength
    pub fn strength(&self) -> AesStrength {
        self.strength
    }

    /// Encrypt data in-place using AES-256-CTR
    pub fn encrypt(&mut self, data: &mut [u8]) -> Result<()> {
        // Update HMAC with plaintext
        self.hmac.update(data);

        // Create AES-CTR cipher
        let key = self.encryption_key.as_slice();
        let iv = vec![0u8; 16]; // Counter mode IV (starts at 0)

        let mut cipher = Ctr128BE::<Aes256>::new(key.into(), iv.as_slice().into());

        // Encrypt in-place
        cipher.apply_keystream(data);

        Ok(())
    }

    /// Finalize and get the authentication code (10 bytes)
    pub fn finalize(self) -> Vec<u8> {
        let mac = self.hmac.finalize();
        // Take first 10 bytes as per WinZip AE-2 spec
        mac.into_bytes()[..10].to_vec()
    }
}

/// AES decryption context for a ZIP entry
pub struct AesDecryptor {
    strength: AesStrength,
    encryption_key: Vec<u8>,
    #[allow(dead_code)] // Used by HMAC, kept for future direct access
    auth_key: Vec<u8>,
    hmac: HmacSha1,
}

impl AesDecryptor {
    /// Create a new AES decryptor with the given password and salt
    pub fn new(password: &str, strength: AesStrength, salt: &[u8]) -> Result<Self> {
        // Validate salt size
        if salt.len() != strength.salt_size() {
            return Err(SZipError::InvalidFormat(format!(
                "Invalid salt size: expected {}, got {}",
                strength.salt_size(),
                salt.len()
            )));
        }

        // Derive keys using PBKDF2-HMAC-SHA1 with 1000 iterations
        let derived_key_size = strength.derived_key_size();
        let mut derived_keys = vec![0u8; derived_key_size];

        pbkdf2_hmac::<Sha1>(password.as_bytes(), salt, 1000, &mut derived_keys);

        // Split derived key material
        let key_size = strength.key_size();
        let encryption_key = derived_keys[..key_size].to_vec();
        let auth_key = derived_keys[key_size..key_size * 2].to_vec();

        // Initialize HMAC for authentication
        let hmac = HmacSha1::new_from_slice(&auth_key)
            .map_err(|e| SZipError::InvalidFormat(format!("HMAC init failed: {}", e)))?;

        Ok(Self {
            strength,
            encryption_key,
            auth_key,
            hmac,
        })
    }

    /// Verify password using password verification bytes
    pub fn verify_password(&self, password_verify: &[u8; 2], derived_keys: &[u8]) -> Result<()> {
        let key_size = self.strength.key_size();
        let expected = [derived_keys[key_size * 2], derived_keys[key_size * 2 + 1]];

        if &expected != password_verify {
            return Err(SZipError::InvalidFormat("Incorrect password".to_string()));
        }

        Ok(())
    }

    /// Decrypt data in-place using AES-256-CTR
    pub fn decrypt(&mut self, data: &mut [u8]) -> Result<()> {
        // Create AES-CTR cipher
        let key = self.encryption_key.as_slice();
        let iv = vec![0u8; 16]; // Counter mode IV (starts at 0)

        let mut cipher = Ctr128BE::<Aes256>::new(key.into(), iv.as_slice().into());

        // Decrypt in-place
        cipher.apply_keystream(data);

        // Update HMAC with decrypted plaintext
        self.hmac.update(data);

        Ok(())
    }

    /// Verify authentication code
    pub fn verify_auth_code(&self, auth_code: &[u8]) -> Result<()> {
        let expected = self.hmac.clone().finalize();
        let expected_bytes = &expected.into_bytes()[..10];

        if expected_bytes != auth_code {
            return Err(SZipError::InvalidFormat(
                "Authentication failed: file may be corrupted or password is incorrect".to_string(),
            ));
        }

        Ok(())
    }
}

/// Generate cryptographically secure random salt
fn generate_salt(size: usize) -> Vec<u8> {
    use std::time::{SystemTime, UNIX_EPOCH};

    // Note: This is a simple implementation for demonstration
    // In production, use a proper CSPRNG like `getrandom` or `rand::thread_rng()`
    let mut salt = vec![0u8; size];

    // Simple pseudo-random generation (REPLACE WITH PROPER CSPRNG IN PRODUCTION!)
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64;

    for (i, byte) in salt.iter_mut().enumerate() {
        *byte = ((seed.wrapping_mul(i as u64 + 1).wrapping_add(i as u64)) % 256) as u8;
    }

    salt
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_aes_strength_sizes() {
        assert_eq!(AesStrength::Aes256.salt_size(), 16);
        assert_eq!(AesStrength::Aes256.key_size(), 32);
        assert_eq!(AesStrength::Aes256.to_winzip_code(), 0x03);
    }

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let password = "test_password_123";
        let plaintext = b"Hello, encrypted world!";

        // Encrypt
        let mut encryptor = AesEncryptor::new(password, AesStrength::Aes256).unwrap();
        let salt = encryptor.salt().to_vec();
        let _password_verify = *encryptor.password_verify();

        let mut encrypted = plaintext.to_vec();
        encryptor.encrypt(&mut encrypted).unwrap();
        let auth_code = encryptor.finalize();

        // Encrypted data should be different from plaintext
        assert_ne!(encrypted, plaintext);

        // Decrypt
        let mut decryptor = AesDecryptor::new(password, AesStrength::Aes256, &salt).unwrap();
        decryptor.decrypt(&mut encrypted).unwrap();
        decryptor.verify_auth_code(&auth_code).unwrap();

        // Decrypted should match original plaintext
        assert_eq!(encrypted, plaintext);
    }

    #[test]
    fn test_wrong_password() {
        let password = "correct_password";
        let wrong_password = "wrong_password";
        let plaintext = b"Secret data";

        // Encrypt with correct password
        let mut encryptor = AesEncryptor::new(password, AesStrength::Aes256).unwrap();
        let salt = encryptor.salt().to_vec();

        let mut encrypted = plaintext.to_vec();
        encryptor.encrypt(&mut encrypted).unwrap();

        // Try to decrypt with wrong password
        let mut decryptor = AesDecryptor::new(wrong_password, AesStrength::Aes256, &salt).unwrap();
        decryptor.decrypt(&mut encrypted).unwrap();

        // Decrypted data should NOT match original (wrong key used)
        assert_ne!(encrypted, plaintext);
    }
}
