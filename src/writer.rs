//! Streaming ZIP writer that compresses data on-the-fly without temp files
//!
//! This eliminates:
//! - Temp file disk I/O
//! - File read buffers
//! - Intermediate storage
//!
//! Expected RAM savings: 5-8 MB per file

use crate::error::{Result, SZipError};
use crc32fast::Hasher as Crc32;
use flate2::write::DeflateEncoder;
use flate2::Compression;
use std::fs::File;
use std::io::{Seek, Write};
use std::path::Path;

/// Entry being written to ZIP
struct ZipEntry {
    name: String,
    local_header_offset: u64,
    crc32: u32,
    compressed_size: u32,
    uncompressed_size: u32,
}

/// Streaming ZIP writer that compresses data on-the-fly
pub struct StreamingZipWriter {
    output: File,
    entries: Vec<ZipEntry>,
    current_entry: Option<CurrentEntry>,
    compression_level: u32,
}

struct CurrentEntry {
    name: String,
    local_header_offset: u64,
    encoder: DeflateEncoder<CrcCountingWriter>,
}

/// Writer that counts bytes and computes CRC32 while writing to output
struct CrcCountingWriter {
    output: File,
    crc: Crc32,
    uncompressed_count: u64,
    compressed_count: u64,
}

impl CrcCountingWriter {
    fn new(output: File) -> Self {
        Self {
            output,
            crc: Crc32::new(),
            uncompressed_count: 0,
            compressed_count: 0,
        }
    }
}

impl Write for CrcCountingWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        // This is the compressed data being written
        let n = self.output.write(buf)?;
        self.compressed_count += n as u64;
        Ok(n)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.output.flush()
    }
}

impl StreamingZipWriter {
    /// Create a new ZIP writer with default compression level (6)
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        Self::with_compression(path, 6)
    }

    /// Create a new ZIP writer with custom compression level (0-9)
    pub fn with_compression<P: AsRef<Path>>(path: P, compression_level: u32) -> Result<Self> {
        let output = File::create(path)?;
        Ok(Self {
            output,
            entries: Vec::new(),
            current_entry: None,
            compression_level: compression_level.min(9),
        })
    }

    /// Start a new entry (file) in the ZIP
    pub fn start_entry(&mut self, name: &str) -> Result<()> {
        // Finish previous entry if any
        self.finish_current_entry()?;

        let local_header_offset = self.output.stream_position()?;

        // Write local file header with data descriptor flag (bit 3)
        self.output.write_all(&[0x50, 0x4b, 0x03, 0x04])?; // signature
        self.output.write_all(&[20, 0])?; // version needed
        self.output.write_all(&[8, 0])?; // general purpose bit flag (bit 3 set)
        self.output.write_all(&[8, 0])?; // compression method = deflate
        self.output.write_all(&[0, 0, 0, 0])?; // mod time/date
        self.output.write_all(&0u32.to_le_bytes())?; // crc32 placeholder
        self.output.write_all(&0u32.to_le_bytes())?; // compressed size placeholder
        self.output.write_all(&0u32.to_le_bytes())?; // uncompressed size placeholder
        self.output.write_all(&(name.len() as u16).to_le_bytes())?;
        self.output.write_all(&0u16.to_le_bytes())?; // extra len
        self.output.write_all(name.as_bytes())?;

        // Create encoder for this entry
        let counting_writer = CrcCountingWriter::new(self.output.try_clone()?);
        let encoder =
            DeflateEncoder::new(counting_writer, Compression::new(self.compression_level));

        self.current_entry = Some(CurrentEntry {
            name: name.to_string(),
            local_header_offset,
            encoder,
        });

        Ok(())
    }

    /// Write uncompressed data to current entry (will be compressed on-the-fly)
    pub fn write_data(&mut self, data: &[u8]) -> Result<()> {
        if let Some(ref mut entry) = self.current_entry {
            // Update CRC with uncompressed data
            entry.encoder.get_mut().crc.update(data);
            entry.encoder.get_mut().uncompressed_count += data.len() as u64;

            // Write to encoder (compresses and writes to output)
            entry.encoder.write_all(data)?;
            Ok(())
        } else {
            Err(SZipError::InvalidFormat("No entry started".to_string()))
        }
    }

    /// Finish current entry and write data descriptor
    fn finish_current_entry(&mut self) -> Result<()> {
        if let Some(entry) = self.current_entry.take() {
            // Finish compression
            let counting_writer = entry.encoder.finish()?;

            let crc = counting_writer.crc.finalize();
            let compressed_size = counting_writer.compressed_count as u32;
            let uncompressed_size = counting_writer.uncompressed_count as u32;

            // Write data descriptor
            self.output.write_all(&[0x50, 0x4b, 0x07, 0x08])?;
            self.output.write_all(&crc.to_le_bytes())?;
            self.output.write_all(&compressed_size.to_le_bytes())?;
            self.output.write_all(&uncompressed_size.to_le_bytes())?;

            // Save entry info for central directory
            self.entries.push(ZipEntry {
                name: entry.name,
                local_header_offset: entry.local_header_offset,
                crc32: crc,
                compressed_size,
                uncompressed_size,
            });
        }
        Ok(())
    }

    /// Finish ZIP file (write central directory and close)
    pub fn finish(mut self) -> Result<()> {
        // Finish last entry
        self.finish_current_entry()?;

        let central_dir_offset = self.output.stream_position()?;

        // Write central directory
        for entry in &self.entries {
            self.output.write_all(&[0x50, 0x4b, 0x01, 0x02])?; // central dir sig
            self.output.write_all(&[20, 0])?; // version made by
            self.output.write_all(&[20, 0])?; // version needed
            self.output.write_all(&[8, 0])?; // general purpose bit flag (bit 3 set)
            self.output.write_all(&[8, 0])?; // compression method
            self.output.write_all(&[0, 0, 0, 0])?; // mod time/date
            self.output.write_all(&entry.crc32.to_le_bytes())?;
            self.output
                .write_all(&entry.compressed_size.to_le_bytes())?;
            self.output
                .write_all(&entry.uncompressed_size.to_le_bytes())?;
            self.output
                .write_all(&(entry.name.len() as u16).to_le_bytes())?;
            self.output.write_all(&0u16.to_le_bytes())?; // extra len
            self.output.write_all(&0u16.to_le_bytes())?; // file comment len
            self.output.write_all(&0u16.to_le_bytes())?; // disk number start
            self.output.write_all(&0u16.to_le_bytes())?; // internal attrs
            self.output.write_all(&0u32.to_le_bytes())?; // external attrs
            self.output
                .write_all(&(entry.local_header_offset as u32).to_le_bytes())?;
            self.output.write_all(entry.name.as_bytes())?;
        }

        let central_dir_size = self.output.stream_position()? - central_dir_offset;

        // Write end of central directory
        self.output.write_all(&[0x50, 0x4b, 0x05, 0x06])?;
        self.output.write_all(&0u16.to_le_bytes())?; // disk number
        self.output.write_all(&0u16.to_le_bytes())?; // disk with central dir
        self.output
            .write_all(&(self.entries.len() as u16).to_le_bytes())?;
        self.output
            .write_all(&(self.entries.len() as u16).to_le_bytes())?;
        self.output
            .write_all(&(central_dir_size as u32).to_le_bytes())?;
        self.output
            .write_all(&(central_dir_offset as u32).to_le_bytes())?;
        self.output.write_all(&0u16.to_le_bytes())?; // comment len

        self.output.flush()?;
        Ok(())
    }
}
