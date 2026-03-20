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
use aes::{Aes128, Aes192, Aes256};
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
/// Specifies the AES key length used for ZIP entry encryption.
/// AES-256 is recommended for maximum security; AES-128 and AES-192 are
/// provided for compatibility with archives created by older tools.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AesStrength {
    /// AES-128 (16-byte key, 8-byte salt). WinZip strength code 0x01.
    Aes128,
    /// AES-192 (24-byte key, 12-byte salt). WinZip strength code 0x02.
    Aes192,
    /// AES-256 (32-byte key, 16-byte salt). WinZip strength code 0x03. Recommended.
    Aes256,
}

impl AesStrength {
    /// Get salt size in bytes
    pub fn salt_size(&self) -> usize {
        match self {
            AesStrength::Aes128 => 8,
            AesStrength::Aes192 => 12,
            AesStrength::Aes256 => 16,
        }
    }

    /// Get key size in bytes
    pub fn key_size(&self) -> usize {
        match self {
            AesStrength::Aes128 => 16,
            AesStrength::Aes192 => 24,
            AesStrength::Aes256 => 32,
        }
    }

    /// Get total derived key material size (key + auth_key + password verification)
    pub fn derived_key_size(&self) -> usize {
        self.key_size() * 2 + 2
    }

    /// Get WinZip encryption strength code
    pub fn to_winzip_code(&self) -> u16 {
        match self {
            AesStrength::Aes128 => 0x01,
            AesStrength::Aes192 => 0x02,
            AesStrength::Aes256 => 0x03,
        }
    }
}

/// AES encryption context for a ZIP entry
///
/// Maintains a running byte offset so that AES-CTR keystream blocks are
/// never reused across multiple `encrypt()` calls on the same entry.
///
/// CTR block counter layout (Ctr128BE, big-endian 128-bit counter):
///
///   encrypt(chunk1, len=N)            encrypt(chunk2, len=M)
///        │                                   │
///        ▼                                   ▼
///   block_num = byte_offset / 16       block_num = (byte_offset+N) / 16
///   IV[8..16] = block_num.to_be_bytes  IV[8..16] = block_num.to_be_bytes
///   keystream: K[0..N]                 keystream: K[N..N+M]  ← no reuse
///   byte_offset += N                   byte_offset += M
pub struct AesEncryptor {
    strength: AesStrength,
    salt: Vec<u8>,
    password_verify: [u8; 2],
    encryption_key: Vec<u8>,
    #[allow(dead_code)] // Used by HMAC, kept for future direct access
    auth_key: Vec<u8>,
    hmac: HmacSha1,
    /// Running byte offset into the keystream; used to advance the CTR block
    /// counter so successive encrypt() calls use non-overlapping keystream segments.
    byte_offset: u64,
}

impl AesEncryptor {
    /// Create a new AES encryptor with the given password
    pub fn new(password: &str, strength: AesStrength) -> Result<Self> {
        // Generate random salt — returns Err rather than panicking on RNG failure
        let salt = generate_salt(strength.salt_size())?;

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
            byte_offset: 0,
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

    /// Encrypt data in-place using AES-256-CTR (call AFTER compression).
    ///
    /// The CTR block counter advances by `data.len()` bytes on each call so
    /// that successive calls on the same entry never reuse keystream blocks.
    pub fn encrypt(&mut self, data: &mut [u8]) -> Result<()> {
        apply_ctr_keystream(self.strength, &self.encryption_key, self.byte_offset, data);
        self.byte_offset += data.len() as u64;
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
///
/// Mirrors `AesEncryptor`: maintains a `byte_offset` so that `decrypt()` can
/// be called multiple times on the same entry (e.g., streaming decrypt in a
/// future implementation) without reusing CTR keystream blocks.
pub struct AesDecryptor {
    #[allow(dead_code)] // Kept for future API extensions
    strength: AesStrength,
    encryption_key: Vec<u8>,
    #[allow(dead_code)] // Used by HMAC, kept for future direct access
    auth_key: Vec<u8>,
    #[allow(dead_code)] // Used for password validation, kept for debugging
    password_verify: [u8; 2],
    hmac: HmacSha1,
    /// Running byte offset into the keystream; mirrors AesEncryptor so that
    /// streaming decryption [P2-3] can reuse this struct without modification.
    byte_offset: u64,
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
            return Err(SZipError::IncorrectPassword);
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
            byte_offset: 0,
        })
    }

    /// Decrypt data in-place using AES-CTR (call on compressed encrypted data).
    ///
    /// Uses the same CTR block-counter advance logic as `AesEncryptor::encrypt()`
    /// so that successive calls on the same entry remain byte-aligned.
    pub fn decrypt(&mut self, data: &mut [u8]) -> Result<()> {
        // AES-CTR decryption is identical to encryption (XOR with keystream)
        apply_ctr_keystream(self.strength, &self.encryption_key, self.byte_offset, data);
        self.byte_offset += data.len() as u64;
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

/// Apply AES-CTR keystream to `data` in-place.
///
/// Dispatches to the correct AES variant based on `strength`.
/// The CTR counter is set so that the keystream starts at `byte_offset`
/// (not at the beginning of the stream), matching WinZip AE-2 behavior.
///
/// This function is shared between `AesEncryptor::encrypt()` and
/// `AesDecryptor::decrypt()` — AES-CTR encryption and decryption are
/// identical operations.
fn apply_ctr_keystream(strength: AesStrength, key: &[u8], byte_offset: u64, data: &mut [u8]) {
    // CTR block counter: stored in the upper 64 bits of the 128-bit IV.
    let block_number = byte_offset / 16;
    let mut iv = [0u8; 16];
    iv[8..16].copy_from_slice(&block_number.to_be_bytes());

    // Partial-block alignment: fast-forward past bytes already consumed
    // within the current block.
    let partial = (byte_offset % 16) as usize;

    macro_rules! run_cipher {
        ($Aes:ty) => {{
            let mut cipher = Ctr128BE::<$Aes>::new(key.into(), &iv.into());
            if partial != 0 {
                let mut discard = vec![0u8; partial];
                cipher.apply_keystream(&mut discard);
            }
            cipher.apply_keystream(data);
        }};
    }

    match strength {
        AesStrength::Aes128 => run_cipher!(Aes128),
        AesStrength::Aes192 => run_cipher!(Aes192),
        AesStrength::Aes256 => run_cipher!(Aes256),
    }
}

/// Generate cryptographically secure random salt.
///
/// Returns `Err(SZipError::EncryptionError)` if the OS CSPRNG is unavailable,
/// rather than panicking — libraries must never panic on external failures.
fn generate_salt(size: usize) -> Result<Vec<u8>> {
    #[cfg(feature = "encryption")]
    {
        let mut salt = vec![0u8; size];
        getrandom::getrandom(&mut salt).map_err(|e| {
            SZipError::EncryptionError(format!("Failed to generate random salt: {}", e))
        })?;
        Ok(salt)
    }

    #[cfg(not(feature = "encryption"))]
    {
        use std::time::{SystemTime, UNIX_EPOCH};
        let mut salt = vec![0u8; size];
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64;

        for (i, byte) in salt.iter_mut().enumerate() {
            *byte = ((seed.wrapping_mul(i as u64 + 1).wrapping_add(i as u64)) % 256) as u8;
        }

        Ok(salt)
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

    /// T1: Multi-chunk encrypt/decrypt round-trip.
    ///
    /// Simulates writing a large entry in two separate write_data() calls —
    /// the second call lands in a different CTR keystream block than the first.
    /// If byte_offset is not advanced correctly, decryption will produce garbage.
    ///
    ///   chunk1 (3MB) ──encrypt──▶ ciphertext1  ──decrypt──▶ plaintext1 ✓
    ///   chunk2 (2MB) ──encrypt──▶ ciphertext2  ──decrypt──▶ plaintext2 ✓
    ///                 (byte_offset=3MB)          (byte_offset=3MB)
    #[test]
    fn test_multi_chunk_encrypt_decrypt_roundtrip() {
        let password = "multi_chunk_password";
        let chunk1 = vec![0xAAu8; 3 * 1024 * 1024]; // 3MB
        let chunk2 = vec![0xBBu8; 2 * 1024 * 1024]; // 2MB

        let mut encryptor = AesEncryptor::new(password, AesStrength::Aes256).unwrap();
        let salt = encryptor.salt().to_vec();
        let password_verify = *encryptor.password_verify();

        // Encrypt chunk1 then chunk2 (simulating two write_data() calls with buffer flush)
        let mut enc1 = chunk1.clone();
        encryptor.encrypt(&mut enc1).unwrap();

        let mut enc2 = chunk2.clone();
        encryptor.encrypt(&mut enc2).unwrap();

        let auth_code = encryptor.finalize();

        // Encrypted chunks must differ from plaintext
        assert_ne!(enc1, chunk1, "chunk1 should be encrypted");
        assert_ne!(enc2, chunk2, "chunk2 should be encrypted");

        // Decryptor must advance its counter by chunk1.len() between calls
        let mut decryptor =
            AesDecryptor::new(password, AesStrength::Aes256, &salt, &password_verify).unwrap();

        decryptor.decrypt(&mut enc1).unwrap();
        decryptor.decrypt(&mut enc2).unwrap();
        decryptor.verify_auth_code(&auth_code).unwrap();

        assert_eq!(enc1, chunk1, "chunk1 decryption mismatch");
        assert_eq!(enc2, chunk2, "chunk2 decryption mismatch");
    }

    /// T2: CTR keystreams must differ across chunks.
    ///
    /// Encrypting two identical plaintext chunks with the same encryptor must
    /// produce different ciphertext — proof that the keystream is not reused.
    ///
    ///   chunk1 = [0xCC; N]  ──encrypt──▶ c1  (keystream from offset 0)
    ///   chunk2 = [0xCC; N]  ──encrypt──▶ c2  (keystream from offset N)
    ///   c1 != c2  ✓  (different keystream blocks)
    #[test]
    fn test_ctr_keystreams_differ_across_chunks() {
        let password = "keystream_test_password";
        let plaintext_block = vec![0xCCu8; 1024]; // 1KB of identical bytes

        let mut encryptor = AesEncryptor::new(password, AesStrength::Aes256).unwrap();

        let mut c1 = plaintext_block.clone();
        encryptor.encrypt(&mut c1).unwrap();

        let mut c2 = plaintext_block.clone();
        encryptor.encrypt(&mut c2).unwrap();

        // Same plaintext encrypted at different offsets must yield different ciphertext
        assert_ne!(c1, c2, "CTR keystream must not repeat across calls");

        // Also verify neither equals the plaintext
        assert_ne!(c1, plaintext_block);
        assert_ne!(c2, plaintext_block);
    }

    /// T3: Single-chunk behavior is unchanged (regression guard).
    ///
    /// Ensures that the byte_offset fix does not break the existing single-call
    /// encrypt/decrypt path used by all current entries.
    #[test]
    fn test_single_chunk_still_works_after_offset_fix() {
        let password = "single_chunk_regression";
        let plaintext = b"The quick brown fox jumps over the lazy dog.".to_vec();

        let mut encryptor = AesEncryptor::new(password, AesStrength::Aes256).unwrap();
        let salt = encryptor.salt().to_vec();
        let password_verify = *encryptor.password_verify();

        let mut ciphertext = plaintext.clone();
        encryptor.encrypt(&mut ciphertext).unwrap();
        let auth_code = encryptor.finalize();

        assert_ne!(ciphertext, plaintext);

        let mut decryptor =
            AesDecryptor::new(password, AesStrength::Aes256, &salt, &password_verify).unwrap();
        decryptor.decrypt(&mut ciphertext).unwrap();
        decryptor.verify_auth_code(&auth_code).unwrap();

        assert_eq!(ciphertext, plaintext);
    }
}
