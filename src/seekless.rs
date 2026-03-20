//! Seekless (pipe-friendly) async ZIP writer.
//!
//! [`SeeklessZipWriter`] writes valid ZIP archives to any `AsyncWrite` sink —
//! HTTP response bodies, pipes, stdin, in-memory `Vec<u8>`, network sockets —
//! **without requiring `AsyncSeek`**.
//!
//! ## Trade-offs vs [`AsyncStreamingZipWriter`](crate::AsyncStreamingZipWriter)
//!
//! | | `AsyncStreamingZipWriter` | `SeeklessZipWriter` |
//! |---|---|---|
//! | Sink | `AsyncWrite + AsyncSeek` | `AsyncWrite` only |
//! | Memory | O(chunk) per entry | O(compressed entry) |
//! | Streaming to HTTP/pipe | ✗ | ✓ |
//! | Entries > 2 GiB | ✓ (ZIP64) | ✓ (ZIP64) |
//!
//! Each entry is fully compressed into a `Vec<u8>` before any bytes are sent
//! to the sink — this is necessary because the local file header must contain
//! the compressed size before the data, and there is no way to go back and
//! patch it without `Seek`.  For small-to-medium entries (< ~100 MB) the
//! memory cost is acceptable.
//!
//! ## Example
//!
//! ```no_run
//! use s_zip::seekless::SeeklessZipWriter;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Stream a ZIP directly to an in-memory buffer (works for any AsyncWrite)
//! let mut out = Vec::<u8>::new();
//! let mut writer = SeeklessZipWriter::new(&mut out);
//! writer.add_entry("hello.txt", b"Hello, world!").await?;
//! writer.add_entry("data.csv", b"id,name\n1,foo\n").await?;
//! writer.finish().await?;
//! println!("ZIP size: {} bytes", out.len());
//! # Ok(())
//! # }
//! ```

use crate::error::{Result, SZipError};
use crate::writer::CompressionMethod;
use crate::EntryOptions;
use flate2::write::DeflateEncoder;
use flate2::Compression;
use std::io::Write;
use tokio::io::{AsyncWrite, AsyncWriteExt};

/// A completed entry ready to be serialised into the output stream.
struct PreparedEntry {
    name: Vec<u8>,
    uncompressed_size: u64,
    compressed_size: u64,
    crc32: u32,
    compression_method: u16,
    local_header_offset: u64,
    /// MS-DOS mod time / date (0 if not set)
    dos_time: u16,
    dos_date: u16,
    unix_extra: Vec<u8>,
    external_attrs: u32,
}

/// Seekless async ZIP writer — works with any `AsyncWrite + Unpin` sink.
///
/// See [module-level docs](self) for the trade-off discussion.
pub struct SeeklessZipWriter<W: AsyncWrite + Unpin> {
    output: W,
    entries: Vec<PreparedEntry>,
    bytes_out: u64,
    compression_level: u32,
    compression_method: CompressionMethod,
}

impl<W: AsyncWrite + Unpin> SeeklessZipWriter<W> {
    /// Create a writer with default DEFLATE level 6 compression.
    pub fn new(output: W) -> Self {
        Self::with_compression(output, 6)
    }

    /// Create a writer with a custom compression level (0 = store, 1–9 for deflate).
    pub fn with_compression(output: W, level: u32) -> Self {
        Self {
            output,
            entries: Vec::new(),
            bytes_out: 0,
            compression_level: level,
            compression_method: CompressionMethod::Deflate,
        }
    }

    /// Create a writer with an explicit compression method and level.
    pub fn with_method(output: W, method: CompressionMethod, level: u32) -> Self {
        Self {
            output,
            entries: Vec::new(),
            bytes_out: 0,
            compression_level: level,
            compression_method: method,
        }
    }

    /// Number of entries staged so far.
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    /// Total uncompressed bytes across all staged entries.
    pub fn bytes_written(&self) -> u64 {
        self.entries.iter().map(|e| e.uncompressed_size).sum()
    }

    /// Stage a complete entry. The data is compressed immediately (in RAM) and
    /// the local file header + compressed data are flushed to the sink.
    pub async fn add_entry(&mut self, name: &str, data: &[u8]) -> Result<()> {
        self.add_entry_with_options(name, data, EntryOptions::default())
            .await
    }

    /// Stage a complete entry with metadata (mtime, Unix permissions).
    pub async fn add_entry_with_options(
        &mut self,
        name: &str,
        data: &[u8],
        options: EntryOptions,
    ) -> Result<()> {
        let name_bytes = name.as_bytes().to_vec();

        // Compress synchronously (no async compressor needed — data is already in memory)
        let (compressed, crc32, method_u16) = self.compress(data)?;

        let uncompressed_size = data.len() as u64;
        let compressed_size = compressed.len() as u64;
        let (dos_time, dos_date) = options.msdos_datetime();
        let unix_extra = options.unix_extra_field();
        let external_attrs = options.external_attrs();

        // Record the local-header offset *before* writing
        let local_header_offset = self.bytes_out;

        // Build and write local file header + compressed data
        let use_zip64 = uncompressed_size > u32::MAX as u64 || compressed_size > u32::MAX as u64;
        let extra_field_local = if use_zip64 {
            build_zip64_extra(uncompressed_size, compressed_size)
        } else {
            unix_extra.clone()
        };

        let version_needed: u16 = if use_zip64 { 45 } else { 20 };

        let mut header = Vec::with_capacity(30 + name_bytes.len() + extra_field_local.len());
        header.extend_from_slice(&0x04034b50u32.to_le_bytes()); // local sig
        header.extend_from_slice(&version_needed.to_le_bytes());
        header.extend_from_slice(&0u16.to_le_bytes()); // flags
        header.extend_from_slice(&method_u16.to_le_bytes());
        header.extend_from_slice(&dos_time.to_le_bytes());
        header.extend_from_slice(&dos_date.to_le_bytes());
        header.extend_from_slice(&crc32.to_le_bytes());
        if use_zip64 {
            header.extend_from_slice(&0xFFFFFFFFu32.to_le_bytes()); // compressed placeholder
            header.extend_from_slice(&0xFFFFFFFFu32.to_le_bytes()); // uncompressed placeholder
        } else {
            header.extend_from_slice(&(compressed_size as u32).to_le_bytes());
            header.extend_from_slice(&(uncompressed_size as u32).to_le_bytes());
        }
        header.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
        header.extend_from_slice(&(extra_field_local.len() as u16).to_le_bytes());
        header.extend_from_slice(&name_bytes);
        header.extend_from_slice(&extra_field_local);

        self.output.write_all(&header).await?;
        self.output.write_all(&compressed).await?;
        self.bytes_out += header.len() as u64 + compressed_size;

        // Store metadata for central directory
        let entry_unix_extra = if use_zip64 {
            unix_extra // put unix extra only in central dir for zip64 entries
        } else {
            Vec::new() // already included in local header
        };

        self.entries.push(PreparedEntry {
            name: name_bytes,
            uncompressed_size,
            compressed_size,
            crc32,
            compression_method: method_u16,
            local_header_offset,
            dos_time,
            dos_date,
            unix_extra: entry_unix_extra,
            external_attrs,
        });

        Ok(())
    }

    /// Finish the archive — write the central directory and EOCD record.
    ///
    /// Consumes the writer and returns the underlying sink.
    pub async fn finish(mut self) -> Result<W> {
        crate::trace!(entries = self.entries.len(), "seekless_finish");
        let cd_offset = self.bytes_out;
        let entry_count = self.entries.len();

        // Write central directory entries
        for entry in &self.entries {
            let use_zip64 = entry.uncompressed_size > u32::MAX as u64
                || entry.compressed_size > u32::MAX as u64
                || entry.local_header_offset > u32::MAX as u64;

            let extra_cd = if use_zip64 {
                let mut v = build_zip64_extra(entry.uncompressed_size, entry.compressed_size);
                // include local header offset in zip64 extra if needed
                if entry.local_header_offset > u32::MAX as u64 {
                    v.truncate(4); // re-build with offset
                    v = build_zip64_extra_with_offset(
                        entry.uncompressed_size,
                        entry.compressed_size,
                        entry.local_header_offset,
                    );
                }
                let mut combined = v;
                combined.extend_from_slice(&entry.unix_extra);
                combined
            } else {
                entry.unix_extra.clone()
            };

            let version_needed: u16 = if use_zip64 { 45 } else { 20 };
            let local_offset: u32 = if entry.local_header_offset > u32::MAX as u64 {
                0xFFFFFFFF
            } else {
                entry.local_header_offset as u32
            };

            let mut cd = Vec::new();
            cd.extend_from_slice(&0x02014b50u32.to_le_bytes()); // central dir sig
            cd.extend_from_slice(&version_needed.to_le_bytes()); // version made by
            cd.extend_from_slice(&version_needed.to_le_bytes()); // version needed
            cd.extend_from_slice(&0u16.to_le_bytes()); // flags
            cd.extend_from_slice(&entry.compression_method.to_le_bytes());
            cd.extend_from_slice(&entry.dos_time.to_le_bytes());
            cd.extend_from_slice(&entry.dos_date.to_le_bytes());
            cd.extend_from_slice(&entry.crc32.to_le_bytes());
            if use_zip64 {
                cd.extend_from_slice(&0xFFFFFFFFu32.to_le_bytes());
                cd.extend_from_slice(&0xFFFFFFFFu32.to_le_bytes());
            } else {
                cd.extend_from_slice(&(entry.compressed_size as u32).to_le_bytes());
                cd.extend_from_slice(&(entry.uncompressed_size as u32).to_le_bytes());
            }
            cd.extend_from_slice(&(entry.name.len() as u16).to_le_bytes());
            cd.extend_from_slice(&(extra_cd.len() as u16).to_le_bytes());
            cd.extend_from_slice(&0u16.to_le_bytes()); // comment len
            cd.extend_from_slice(&0u16.to_le_bytes()); // disk start
            cd.extend_from_slice(&0u16.to_le_bytes()); // internal attrs
            cd.extend_from_slice(&entry.external_attrs.to_le_bytes());
            cd.extend_from_slice(&local_offset.to_le_bytes());
            cd.extend_from_slice(&entry.name);
            cd.extend_from_slice(&extra_cd);

            self.output.write_all(&cd).await?;
            self.bytes_out += cd.len() as u64;
        }

        let cd_size = self.bytes_out - cd_offset;
        let use_zip64_eocd = entry_count > u16::MAX as usize
            || cd_offset > u32::MAX as u64
            || cd_size > u32::MAX as u64;

        if use_zip64_eocd {
            // ZIP64 end of central directory record
            let mut zip64_eocd = Vec::new();
            zip64_eocd.extend_from_slice(&0x06064b50u32.to_le_bytes()); // sig
            zip64_eocd.extend_from_slice(&44u64.to_le_bytes()); // size of record
            zip64_eocd.extend_from_slice(&45u16.to_le_bytes()); // version made by
            zip64_eocd.extend_from_slice(&45u16.to_le_bytes()); // version needed
            zip64_eocd.extend_from_slice(&0u32.to_le_bytes()); // disk number
            zip64_eocd.extend_from_slice(&0u32.to_le_bytes()); // disk with cd
            zip64_eocd.extend_from_slice(&(entry_count as u64).to_le_bytes());
            zip64_eocd.extend_from_slice(&(entry_count as u64).to_le_bytes());
            zip64_eocd.extend_from_slice(&cd_size.to_le_bytes());
            zip64_eocd.extend_from_slice(&cd_offset.to_le_bytes());

            // ZIP64 end of central directory locator
            let zip64_eocd_offset = self.bytes_out;
            let mut locator = Vec::new();
            locator.extend_from_slice(&0x07064b50u32.to_le_bytes()); // sig
            locator.extend_from_slice(&0u32.to_le_bytes()); // disk with zip64 eocd
            locator.extend_from_slice(&zip64_eocd_offset.to_le_bytes());
            locator.extend_from_slice(&1u32.to_le_bytes()); // total disks

            self.output.write_all(&zip64_eocd).await?;
            self.output.write_all(&locator).await?;
            self.bytes_out += zip64_eocd.len() as u64 + locator.len() as u64;
        }

        // End of central directory record
        let eocd_entry_count = entry_count.min(u16::MAX as usize) as u16;
        let eocd_cd_size = cd_size.min(u32::MAX as u64) as u32;
        let eocd_cd_offset = cd_offset.min(u32::MAX as u64) as u32;

        let mut eocd = Vec::new();
        eocd.extend_from_slice(&0x06054b50u32.to_le_bytes()); // sig
        eocd.extend_from_slice(&0u16.to_le_bytes()); // disk number
        eocd.extend_from_slice(&0u16.to_le_bytes()); // disk with cd
        eocd.extend_from_slice(&eocd_entry_count.to_le_bytes());
        eocd.extend_from_slice(&eocd_entry_count.to_le_bytes());
        eocd.extend_from_slice(&eocd_cd_size.to_le_bytes());
        eocd.extend_from_slice(&eocd_cd_offset.to_le_bytes());
        eocd.extend_from_slice(&0u16.to_le_bytes()); // comment len

        self.output.write_all(&eocd).await?;
        self.output.flush().await?;

        Ok(self.output)
    }

    fn compress(&self, data: &[u8]) -> Result<(Vec<u8>, u32, u16)> {
        let crc32 = crc32fast::hash(data);

        match self.compression_method {
            CompressionMethod::Stored => Ok((data.to_vec(), crc32, 0)),
            CompressionMethod::Deflate => {
                let level = Compression::new(self.compression_level.min(9));
                let mut encoder = DeflateEncoder::new(Vec::new(), level);
                encoder.write_all(data).map_err(SZipError::Io)?;
                let compressed = encoder.finish().map_err(SZipError::Io)?;
                Ok((compressed, crc32, 8))
            }
            #[cfg(feature = "zstd-support")]
            CompressionMethod::Zstd => {
                let compressed =
                    zstd::encode_all(data, self.compression_level as i32).map_err(SZipError::Io)?;
                Ok((compressed, crc32, 93))
            }
        }
    }
}

fn build_zip64_extra(uncompressed: u64, compressed: u64) -> Vec<u8> {
    let mut v = Vec::with_capacity(20);
    v.extend_from_slice(&0x0001u16.to_le_bytes()); // tag
    v.extend_from_slice(&16u16.to_le_bytes()); // data size
    v.extend_from_slice(&uncompressed.to_le_bytes());
    v.extend_from_slice(&compressed.to_le_bytes());
    v
}

fn build_zip64_extra_with_offset(uncompressed: u64, compressed: u64, offset: u64) -> Vec<u8> {
    let mut v = Vec::with_capacity(28);
    v.extend_from_slice(&0x0001u16.to_le_bytes()); // tag
    v.extend_from_slice(&24u16.to_le_bytes()); // data size
    v.extend_from_slice(&uncompressed.to_le_bytes());
    v.extend_from_slice(&compressed.to_le_bytes());
    v.extend_from_slice(&offset.to_le_bytes());
    v
}
