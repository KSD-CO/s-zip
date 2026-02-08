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

    /// Update HMAC with plaintext data (call BEFORE encryption)
    pub fn update_hmac(&mut self, data: &[u8]) {
        self.hmac.update(data);
    }

    /// Encrypt data in-place using AES-256-CTR (call AFTER compression)
    pub fn encrypt(&mut self, data: &mut [u8]) -> Result<()> {
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
    #[allow(dead_code)] // Kept for future API extensions
    strength: AesStrength,
    encryption_key: Vec<u8>,
    #[allow(dead_code)] // Used by HMAC, kept for future direct access
    auth_key: Vec<u8>,
    #[allow(dead_code)] // Used for password validation, kept for debugging
    password_verify: [u8; 2],
    hmac: HmacSha1,
}

impl AesDecryptor {
    /// Create a new AES decryptor with the given password, salt, and password verification bytes
    pub fn new(
        password: &str,
        strength: AesStrength,
        salt: &[u8],
        password_verify: &[u8; 2],
    ) -> Result<Self> {
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
        let expected_pw_verify = [derived_keys[key_size * 2], derived_keys[key_size * 2 + 1]];

        // Verify password immediately
        if &expected_pw_verify != password_verify {
            return Err(SZipError::InvalidFormat("Incorrect password".to_string()));
        }

        // Initialize HMAC for authentication
        let hmac = HmacSha1::new_from_slice(&auth_key)
            .map_err(|e| SZipError::InvalidFormat(format!("HMAC init failed: {}", e)))?;

        Ok(Self {
            strength,
            encryption_key,
            auth_key,
            password_verify: *password_verify,
            hmac,
        })
    }

    /// Decrypt data in-place using AES-256-CTR (call on compressed encrypted data)
    pub fn decrypt(&mut self, data: &mut [u8]) -> Result<()> {
        // Create AES-CTR cipher
        let key = self.encryption_key.as_slice();
        let iv = vec![0u8; 16]; // Counter mode IV (starts at 0)

        let mut cipher = Ctr128BE::<Aes256>::new(key.into(), iv.as_slice().into());

        // Decrypt in-place
        cipher.apply_keystream(data);

        Ok(())
    }

    /// Update HMAC with plaintext data (call AFTER decompression)
    pub fn update_hmac(&mut self, data: &[u8]) {
        self.hmac.update(data);
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
    // Use OS CSPRNG via `getrandom` crate when available. This is the
    // recommended secure source of randomness for salts in cryptographic
    // operations.
    #[cfg(feature = "encryption")]
    {
        let mut salt = vec![0u8; size];
        // getrandom should not fail on a normal OS; map failure to panic in
        // this unlikely event. Library constructors return `Result`, so any
        // error during initialization will be propagated from callers.
        getrandom::getrandom(&mut salt).expect("getrandom failed to generate salt");
        salt
    }

    // Fallback for builds without `getrandom` feature (shouldn't occur
    // because the `encryption` feature enables `getrandom` in Cargo.toml).
    #[cfg(not(feature = "encryption"))]
    {
        // As a safe fallback, use the platform RNG from the standard library's
        // randomness support via `rand` is preferred, but to avoid adding an
        // extra dependency here, fall back to a simple time-based seed. This
        // branch is only used when encryption feature is disabled.
        use std::time::{SystemTime, UNIX_EPOCH};
        let mut salt = vec![0u8; size];
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64;

        for (i, byte) in salt.iter_mut().enumerate() {
            *byte = ((seed.wrapping_mul(i as u64 + 1).wrapping_add(i as u64)) % 256) as u8;
        }

        salt
    }
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
        let password_verify = *encryptor.password_verify();

        let mut encrypted = plaintext.to_vec();
        encryptor.encrypt(&mut encrypted).unwrap();
        let auth_code = encryptor.finalize();

        // Encrypted data should be different from plaintext
        assert_ne!(encrypted, plaintext);

        // Decrypt
        let mut decryptor =
            AesDecryptor::new(password, AesStrength::Aes256, &salt, &password_verify).unwrap();
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
        let password_verify = *encryptor.password_verify();

        let mut encrypted = plaintext.to_vec();
        encryptor.encrypt(&mut encrypted).unwrap();

        // Try to decrypt with wrong password - should fail at password verification
        let result =
            AesDecryptor::new(wrong_password, AesStrength::Aes256, &salt, &password_verify);
        assert!(result.is_err(), "Expected password verification to fail");
    }
}
