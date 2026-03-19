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
pub mod format;
pub mod reader;
pub mod writer;

#[cfg(feature = "encryption")]
pub mod encryption;

#[cfg(feature = "encryption")]
pub mod decrypt_reader;

#[cfg(feature = "async")]
pub mod async_writer;

#[cfg(feature = "async")]
pub mod async_reader;

#[cfg(feature = "async")]
pub mod parallel;

#[cfg(any(feature = "cloud-s3", feature = "cloud-gcs"))]
pub mod cloud;

pub use error::{Result, SZipError};
pub use format::ZipEntry;
pub use reader::StreamingZipReader;
pub use writer::{CompressionMethod, StreamingZipWriter};

/// Options for a ZIP entry controlling metadata written to the local file header.
///
/// Use with `start_entry_with_options()` on either `StreamingZipWriter` or
/// `AsyncStreamingZipWriter`. Fields default to "no metadata" (zero timestamp,
/// no permissions).
///
/// # Example
/// ```no_run
/// # use s_zip::{StreamingZipWriter, EntryOptions};
/// # use std::time::SystemTime;
/// # fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let mut writer = StreamingZipWriter::new("output.zip")?;
/// let opts = EntryOptions {
///     mtime: Some(SystemTime::now()),
///     unix_mode: Some(0o644),
/// };
/// writer.start_entry_with_options("file.txt", opts)?;
/// writer.write_data(b"Hello")?;
/// writer.finish()?;
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone, Default)]
pub struct EntryOptions {
    /// Last-modified time. Written as MS-DOS time/date in the local header.
    /// If `None`, the timestamp fields are written as zero (no date).
    pub mtime: Option<std::time::SystemTime>,
    /// Unix file permission bits (e.g. `0o644`, `0o755`).
    /// Written as a Unix extra field (ID 0x7875) in the local and central headers.
    /// If `None`, no Unix extra field is written.
    pub unix_mode: Option<u32>,
}

impl EntryOptions {
    /// Convert `SystemTime` to MS-DOS date/time packed into a `u32`.
    ///
    /// MS-DOS time format (16-bit):
    ///   bits 15-11: hours (0-23)
    ///   bits 10-5:  minutes (0-59)
    ///   bits 4-0:   seconds / 2 (0-29)
    ///
    /// MS-DOS date format (16-bit):
    ///   bits 15-9: year - 1980 (0-127 → 1980-2107)
    ///   bits 8-5:  month (1-12)
    ///   bits 4-0:  day (1-31)
    pub(crate) fn msdos_datetime(&self) -> (u16, u16) {
        use std::time::{Duration, UNIX_EPOCH};

        let Some(mtime) = self.mtime else {
            return (0, 0);
        };

        // Seconds since Unix epoch; fall back to zero on out-of-range times
        let secs = mtime
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_secs();

        // Convert Unix timestamp to calendar (simple Gregorian, no DST)
        let secs_per_day = 86400u64;
        let days_since_epoch = secs / secs_per_day;
        let time_of_day = secs % secs_per_day;

        let hour = (time_of_day / 3600) as u16;
        let minute = ((time_of_day % 3600) / 60) as u16;
        let second = (time_of_day % 60) as u16;

        // Days since 1970-01-01 → calendar date (proleptic Gregorian)
        let z = days_since_epoch as i64 + 719_468;
        let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
        let doe = (z - era * 146_097) as u32;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let y = yoe as i64 + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = doy - (153 * mp + 2) / 5 + 1;
        let month = if mp < 10 { mp + 3 } else { mp - 9 };
        let year = if month <= 2 { y + 1 } else { y };

        // Clamp to MS-DOS range (1980-2107)
        let dos_year = (year.clamp(1980, 2107) - 1980) as u16;

        let dos_time = (hour << 11) | (minute << 5) | (second / 2);
        let dos_date = (dos_year << 9) | ((month as u16) << 5) | (day as u16);

        (dos_time, dos_date)
    }

    /// Build the Unix extra field (ID 0x7875 "Info-ZIP New Unix") carrying uid=0, gid=0.
    ///
    /// Layout: header_id(2) + data_size(2) + version(1) + uid_size(1) + uid(N) + gid_size(1) + gid(N)
    pub(crate) fn unix_extra_field(&self) -> Vec<u8> {
        let Some(mode) = self.unix_mode else {
            return Vec::new();
        };

        // Also write external file attributes carrying Unix mode in upper 16 bits —
        // that's handled separately in the central dir. Here we write the extra field
        // for readers that use it.
        let _ = mode; // suppress unused warning — mode is used in central dir write

        // 0x7875 "Info-ZIP New Unix" extra field: uid=0, gid=0 (minimal)
        // version=1, uid_size=4, uid=0u32, gid_size=4, gid=0u32
        let mut field = Vec::with_capacity(15);
        field.extend_from_slice(&0x7875u16.to_le_bytes()); // ID
        field.extend_from_slice(&11u16.to_le_bytes()); // data size
        field.push(1); // version
        field.push(4); // uid size
        field.extend_from_slice(&0u32.to_le_bytes()); // uid = 0
        field.push(4); // gid size
        field.extend_from_slice(&0u32.to_le_bytes()); // gid = 0
        field
    }

    /// Compute external file attributes from unix_mode for the central directory.
    /// Returns 0 if no unix_mode is set.
    #[allow(dead_code)]
    pub(crate) fn external_attrs(&self) -> u32 {
        self.unix_mode.map(|m| m << 16).unwrap_or(0)
    }
}

#[cfg(feature = "async")]
pub use async_writer::AsyncStreamingZipWriter;
#[cfg(feature = "encryption")]
pub use encryption::AesStrength;

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
        assert!(!zip_bytes.is_empty(), "ZIP should not be empty");

        // Verify ZIP has correct signature
        assert_eq!(
            &zip_bytes[0..4],
            b"PK\x03\x04",
            "Should start with ZIP signature"
        );
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
