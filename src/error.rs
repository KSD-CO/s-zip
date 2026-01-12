//! Error types for s-zip

use std::io;

/// Result type for s-zip operations
pub type Result<T> = std::result::Result<T, SZipError>;

/// Error types that can occur during ZIP operations
#[derive(Debug)]
pub enum SZipError {
    /// I/O error
    Io(io::Error),
    /// Invalid ZIP format or structure
    InvalidFormat(String),
    /// Entry not found in ZIP archive
    EntryNotFound(String),
    /// Unsupported compression method
    UnsupportedCompression(u16),
    /// Encryption/decryption error
    #[cfg(feature = "encryption")]
    EncryptionError(String),
    /// Incorrect password
    #[cfg(feature = "encryption")]
    IncorrectPassword,
}

impl std::fmt::Display for SZipError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SZipError::Io(e) => write!(f, "I/O error: {}", e),
            SZipError::InvalidFormat(msg) => write!(f, "Invalid ZIP format: {}", msg),
            SZipError::EntryNotFound(name) => write!(f, "Entry not found: {}", name),
            SZipError::UnsupportedCompression(method) => {
                write!(f, "Unsupported compression method: {}", method)
            }
            #[cfg(feature = "encryption")]
            SZipError::EncryptionError(msg) => write!(f, "Encryption error: {}", msg),
            #[cfg(feature = "encryption")]
            SZipError::IncorrectPassword => write!(f, "Incorrect password"),
        }
    }
}

impl std::error::Error for SZipError {}

impl From<io::Error> for SZipError {
    fn from(err: io::Error) -> Self {
        SZipError::Io(err)
    }
}
