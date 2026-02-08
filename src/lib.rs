//! # s-zip: High-Performance Streaming ZIP Library
//!
//! `s-zip` is a lightweight, high-performance ZIP library focused on streaming operations
//! with minimal memory footprint. Perfect for working with large ZIP files without loading
//! everything into memory.
//!
//! ## Features
//!
//! - **Streaming Read**: Read ZIP entries on-the-fly without loading entire archive
//! - **Streaming Write**: Write ZIP files with on-the-fly compression, no temp files
//! - **Low Memory**: Constant memory usage regardless of ZIP file size
//! - **Fast**: Optimized for performance with minimal allocations
//! - **Simple API**: Easy to use, intuitive interface
//!
//! ## Quick Start
//!
//! ### Reading a ZIP file
//!
//! ```no_run
//! use s_zip::StreamingZipReader;
//!
//! let mut reader = StreamingZipReader::open("archive.zip")?;
//!
//! // List all entries
//! for entry in reader.entries() {
//!     println!("{}: {} bytes", entry.name, entry.uncompressed_size);
//! }
//!
//! // Read a specific file
//! let data = reader.read_entry_by_name("file.txt")?;
//! # Ok::<(), s_zip::SZipError>(())
//! ```
//!
//! ### Writing a ZIP file
//!
//! ```no_run
//! use s_zip::StreamingZipWriter;
//!
//! let mut writer = StreamingZipWriter::new("output.zip")?;
//!
//! writer.start_entry("file1.txt")?;
//! writer.write_data(b"Hello, World!")?;
//!
//! writer.start_entry("file2.txt")?;
//! writer.write_data(b"Another file")?;
//!
//! writer.finish()?;
//! # Ok::<(), s_zip::SZipError>(())
//! ```
//!
//! ### Using arbitrary writers (in-memory, network, etc.)
//!
//! ```no_run
//! use s_zip::StreamingZipWriter;
//! use std::io::Cursor;
//!
//! // Write ZIP to in-memory buffer
//! let buffer = Vec::new();
//! let cursor = Cursor::new(buffer);
//! let mut writer = StreamingZipWriter::from_writer(cursor)?;
//!
//! writer.start_entry("data.txt")?;
//! writer.write_data(b"In-memory ZIP content")?;
//!
//! // finish() returns the writer, allowing you to extract the data
//! let cursor = writer.finish()?;
//! let zip_bytes = cursor.into_inner();
//!
//! println!("Created ZIP with {} bytes", zip_bytes.len());
//! # Ok::<(), s_zip::SZipError>(())
//! ```

pub mod error;
pub mod reader;
pub mod writer;

#[cfg(feature = "encryption")]
pub mod encryption;

#[cfg(feature = "async")]
pub mod async_writer;

#[cfg(feature = "async")]
pub mod async_reader;

#[cfg(feature = "async")]
pub mod parallel;

#[cfg(any(feature = "cloud-s3", feature = "cloud-gcs"))]
pub mod cloud;

pub use error::{Result, SZipError};
pub use reader::{StreamingZipReader, ZipEntry};
pub use writer::{CompressionMethod, StreamingZipWriter};

#[cfg(feature = "encryption")]
pub use encryption::AesStrength;

#[cfg(feature = "async")]
pub use async_writer::AsyncStreamingZipWriter;

#[cfg(feature = "async")]
pub use async_reader::{AsyncStreamingZipReader, GenericAsyncZipReader};

#[cfg(feature = "async")]
pub use parallel::{ParallelConfig, ParallelEntry};

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_basic_write_read_roundtrip() {
        // Create ZIP in memory
        let buffer = Vec::new();
        let cursor = Cursor::new(buffer);
        let mut writer = StreamingZipWriter::from_writer(cursor).unwrap();

        // Add first file
        writer.start_entry("test1.txt").unwrap();
        writer.write_data(b"Hello, World!").unwrap();

        // Add second file
        writer.start_entry("test2.txt").unwrap();
        writer.write_data(b"Testing s-zip library").unwrap();

        // Finish and get ZIP bytes
        let cursor = writer.finish().unwrap();
        let zip_bytes = cursor.into_inner();

        // Verify ZIP was created
        assert!(zip_bytes.len() > 0, "ZIP should not be empty");

        // Verify ZIP has correct signature
        assert_eq!(&zip_bytes[0..4], b"PK\x03\x04", "Should start with ZIP signature");
    }

    #[test]
    fn test_compression_method_to_zip_method() {
        assert_eq!(CompressionMethod::Stored.to_zip_method(), 0);
        assert_eq!(CompressionMethod::Deflate.to_zip_method(), 8);
        
        #[cfg(feature = "zstd-support")]
        assert_eq!(CompressionMethod::Zstd.to_zip_method(), 93);
    }

    #[test]
    fn test_empty_entry_name() {
        let buffer = Vec::new();
        let cursor = Cursor::new(buffer);
        let mut writer = StreamingZipWriter::from_writer(cursor).unwrap();

        // Try to create entry with empty name - should succeed (valid in ZIP spec)
        assert!(writer.start_entry("").is_ok());
    }

    #[test]
    fn test_multiple_small_entries() {
        let buffer = Vec::new();
        let cursor = Cursor::new(buffer);
        let mut writer = StreamingZipWriter::from_writer(cursor).unwrap();

        // Add 10 small files
        for i in 0..10 {
            let entry_name = format!("file_{}.txt", i);
            let entry_data = format!("Content of file {}", i);
            
            writer.start_entry(&entry_name).unwrap();
            writer.write_data(entry_data.as_bytes()).unwrap();
        }

        let cursor = writer.finish().unwrap();
        let zip_bytes = cursor.into_inner();

        // Verify ZIP was created and has reasonable size
        assert!(zip_bytes.len() > 100, "ZIP with 10 files should be larger");
    }

    #[test]
    fn test_error_display() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let err = SZipError::from(io_err);
        assert!(format!("{}", err).contains("I/O error"));

        let invalid_err = SZipError::InvalidFormat("bad format".to_string());
        assert!(format!("{}", invalid_err).contains("Invalid ZIP format"));

        let not_found_err = SZipError::EntryNotFound("missing.txt".to_string());
        assert!(format!("{}", not_found_err).contains("Entry not found"));
    }

    #[cfg(feature = "encryption")]
    #[test]
    fn test_aes_strength() {
        assert_eq!(AesStrength::Aes256.salt_size(), 16);
        assert_eq!(AesStrength::Aes256.key_size(), 32);
        assert_eq!(AesStrength::Aes256.to_winzip_code(), 0x03);
    }
}
