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
