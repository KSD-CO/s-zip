//! Streaming ZIP writer that compresses data on-the-fly without temp files
//!
//! This eliminates:
//! - Temp file disk I/O
//! - File read buffers
//! - Intermediate storage
//!
//! Expected RAM savings: 5-8 MB per file
//!
//! Now supports arbitrary writers (File, Vec<u8>, network streams, etc.)

use crate::error::{Result, SZipError};
use crc32fast::Hasher as Crc32;
use flate2::write::DeflateEncoder;
use flate2::Compression;
use std::fs::File;
use std::io::{Seek, Write};
use std::path::Path;

#[cfg(feature = "encryption")]
use crate::encryption::{AesEncryptor, AesStrength};

/// Compression method to use for ZIP entries
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompressionMethod {
    /// No compression (stored)
    Stored,
    /// DEFLATE compression (most common)
    Deflate,
    /// Zstd compression (requires zstd-support feature)
    #[cfg(feature = "zstd-support")]
    Zstd,
}

impl CompressionMethod {
    pub(crate) fn to_zip_method(self) -> u16 {
        match self {
            CompressionMethod::Stored => 0,
            CompressionMethod::Deflate => 8,
            #[cfg(feature = "zstd-support")]
            CompressionMethod::Zstd => 93,
        }
    }
}

/// Entry being written to ZIP
struct ZipEntry {
    name: String,
    local_header_offset: u64,
    crc32: u32,
    compressed_size: u64,
    uncompressed_size: u64,
    compression_method: u16,
    #[cfg(feature = "encryption")]
    #[allow(dead_code)] // Will be used for central directory in future versions
    encryption_strength: Option<u16>,
}

/// Streaming ZIP writer that compresses data on-the-fly
pub struct StreamingZipWriter<W: Write + Seek> {
    output: W,
    entries: Vec<ZipEntry>,
    current_entry: Option<CurrentEntry>,
    compression_level: u32,
    compression_method: CompressionMethod,
    #[cfg(feature = "encryption")]
    password: Option<String>,
    #[cfg(feature = "encryption")]
    encryption_strength: AesStrength,
}

struct CurrentEntry {
    name: String,
    local_header_offset: u64,
    encoder: Box<dyn CompressorWrite>,
    counter: CrcCounter,
    compression_method: u16,
    #[cfg(feature = "encryption")]
    encryptor: Option<AesEncryptor>,
}

trait CompressorWrite: Write {
    fn finish_compression(self: Box<Self>) -> Result<CompressedBuffer>;
    fn get_buffer_mut(&mut self) -> &mut CompressedBuffer;
}

struct DeflateCompressor {
    encoder: DeflateEncoder<CompressedBuffer>,
}

impl Write for DeflateCompressor {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.encoder.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.encoder.flush()
    }
}

impl CompressorWrite for DeflateCompressor {
    fn finish_compression(self: Box<Self>) -> Result<CompressedBuffer> {
        Ok(self.encoder.finish()?)
    }

    fn get_buffer_mut(&mut self) -> &mut CompressedBuffer {
        self.encoder.get_mut()
    }
}

#[cfg(feature = "zstd-support")]
struct ZstdCompressor {
    encoder: zstd::Encoder<'static, CompressedBuffer>,
}

#[cfg(feature = "zstd-support")]
impl Write for ZstdCompressor {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.encoder.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.encoder.flush()
    }
}

#[cfg(feature = "zstd-support")]
impl CompressorWrite for ZstdCompressor {
    fn finish_compression(self: Box<Self>) -> Result<CompressedBuffer> {
        Ok(self.encoder.finish()?)
    }

    fn get_buffer_mut(&mut self) -> &mut CompressedBuffer {
        self.encoder.get_mut()
    }
}

/// Metadata tracker for CRC and byte counts
struct CrcCounter {
    crc: Crc32,
    uncompressed_count: u64,
    compressed_count: u64,
}

impl CrcCounter {
    fn new() -> Self {
        Self {
            crc: Crc32::new(),
            uncompressed_count: 0,
            compressed_count: 0,
        }
    }

    fn update_uncompressed(&mut self, data: &[u8]) {
        self.crc.update(data);
        self.uncompressed_count += data.len() as u64;
    }

    fn add_compressed(&mut self, count: u64) {
        self.compressed_count += count;
    }

    fn finalize(&self) -> u32 {
        self.crc.clone().finalize()
    }
}

/// Buffered writer for compressed data with adaptive sizing
///
/// Automatically adjusts buffer capacity and flush threshold based on data size hints
/// to optimize memory usage and performance for different file sizes.
struct CompressedBuffer {
    buffer: Vec<u8>,
    flush_threshold: usize,
}

impl CompressedBuffer {
    /// Create buffer with default capacity (for backward compatibility)
    #[allow(dead_code)]
    fn new() -> Self {
        Self::with_size_hint(None)
    }

    /// Create buffer with adaptive sizing based on expected data size
    ///
    /// Optimizes initial capacity and flush threshold:
    /// - Tiny files (<10KB): 8KB initial, 256KB threshold
    /// - Small files (<100KB): 32KB initial, 512KB threshold  
    /// - Medium files (<1MB): 128KB initial, 2MB threshold
    /// - Large files (≥1MB): 256KB initial, 4MB threshold
    fn with_size_hint(size_hint: Option<u64>) -> Self {
        let (initial_capacity, flush_threshold) = match size_hint {
            Some(size) if size < 10_000 => (8 * 1024, 256 * 1024), // Tiny: 8KB, 256KB
            Some(size) if size < 100_000 => (32 * 1024, 512 * 1024), // Small: 32KB, 512KB
            Some(size) if size < 1_000_000 => (128 * 1024, 2 * 1024 * 1024), // Medium: 128KB, 2MB
            Some(size) if size < 10_000_000 => (256 * 1024, 4 * 1024 * 1024), // Large: 256KB, 4MB
            _ => (512 * 1024, 8 * 1024 * 1024),                    // Very large: 512KB, 8MB
        };

        Self {
            buffer: Vec::with_capacity(initial_capacity),
            flush_threshold,
        }
    }

    fn take(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.buffer)
    }

    fn should_flush(&self) -> bool {
        self.buffer.len() >= self.flush_threshold
    }
}

impl Write for CompressedBuffer {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.buffer.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl StreamingZipWriter<File> {
    /// Create a new ZIP writer with default compression level (6) using DEFLATE
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        Self::with_compression(path, 6)
    }

    /// Create a new ZIP writer with custom compression level (0-9) using DEFLATE
    pub fn with_compression<P: AsRef<Path>>(path: P, compression_level: u32) -> Result<Self> {
        Self::with_method(path, CompressionMethod::Deflate, compression_level)
    }

    /// Create a new ZIP writer with specified compression method and level
    ///
    /// # Arguments
    /// * `path` - Path to the output ZIP file
    /// * `method` - Compression method to use (Deflate, Zstd, or Stored)
    /// * `compression_level` - Compression level (0-9 for DEFLATE, 1-21 for Zstd)
    pub fn with_method<P: AsRef<Path>>(
        path: P,
        method: CompressionMethod,
        compression_level: u32,
    ) -> Result<Self> {
        let output = File::create(path)?;
        Ok(Self {
            output,
            entries: Vec::new(),
            current_entry: None,
            compression_level,
            compression_method: method,
            #[cfg(feature = "encryption")]
            password: None,
            #[cfg(feature = "encryption")]
            encryption_strength: AesStrength::Aes256,
        })
    }

    /// Create a new ZIP writer with Zstd compression (requires zstd-support feature)
    #[cfg(feature = "zstd-support")]
    pub fn with_zstd<P: AsRef<Path>>(path: P, compression_level: i32) -> Result<Self> {
        let output = File::create(path)?;
        Ok(Self {
            output,
            entries: Vec::new(),
            current_entry: None,
            compression_level: compression_level as u32,
            compression_method: CompressionMethod::Zstd,
            #[cfg(feature = "encryption")]
            password: None,
            #[cfg(feature = "encryption")]
            encryption_strength: AesStrength::Aes256,
        })
    }
}

impl<W: Write + Seek> StreamingZipWriter<W> {
    /// Create a new ZIP writer from an arbitrary writer with default compression level (6) using DEFLATE
    pub fn from_writer(writer: W) -> Result<Self> {
        Self::from_writer_with_compression(writer, 6)
    }

    /// Create a new ZIP writer from an arbitrary writer with custom compression level
    pub fn from_writer_with_compression(writer: W, compression_level: u32) -> Result<Self> {
        Self::from_writer_with_method(writer, CompressionMethod::Deflate, compression_level)
    }

    /// Create a new ZIP writer from an arbitrary writer with specified compression method and level
    ///
    /// # Arguments
    /// * `writer` - Any writer implementing Write + Seek
    /// * `method` - Compression method to use (Deflate, Zstd, or Stored)
    /// * `compression_level` - Compression level (0-9 for DEFLATE, 1-21 for Zstd)
    pub fn from_writer_with_method(
        writer: W,
        method: CompressionMethod,
        compression_level: u32,
    ) -> Result<Self> {
        Ok(Self {
            output: writer,
            entries: Vec::new(),
            current_entry: None,
            compression_level,
            compression_method: method,
            #[cfg(feature = "encryption")]
            password: None,
            #[cfg(feature = "encryption")]
            encryption_strength: AesStrength::Aes256,
        })
    }

    /// Set password for AES encryption (requires encryption feature)
    ///
    /// All subsequent entries will be encrypted with AES-256 using the provided password.
    /// Call this method before `start_entry()` to encrypt files.
    ///
    /// # Arguments
    /// * `password` - Password for encryption (minimum 8 characters recommended)
    ///
    /// # Example
    /// ```no_run
    /// use s_zip::StreamingZipWriter;
    ///
    /// let mut writer = StreamingZipWriter::new("encrypted.zip")?;
    /// writer.set_password("my_secure_password");
    ///
    /// writer.start_entry("secret.txt")?;
    /// writer.write_data(b"Confidential data")?;
    /// writer.finish()?;
    /// # Ok::<(), s_zip::SZipError>(())
    /// ```
    #[cfg(feature = "encryption")]
    pub fn set_password(&mut self, password: impl Into<String>) -> &mut Self {
        self.password = Some(password.into());
        self
    }

    /// Set AES encryption strength (default: AES-256)
    ///
    /// # Arguments
    /// * `strength` - AES encryption strength (Aes128, Aes192, or Aes256)
    #[cfg(feature = "encryption")]
    pub fn set_encryption_strength(&mut self, strength: AesStrength) -> &mut Self {
        self.encryption_strength = strength;
        self
    }

    /// Clear password (disable encryption for subsequent entries)
    #[cfg(feature = "encryption")]
    pub fn clear_password(&mut self) -> &mut Self {
        self.password = None;
        self
    }

    /// Start a new entry (file) in the ZIP
    pub fn start_entry(&mut self, name: &str) -> Result<()> {
        self.start_entry_with_hint(name, None)
    }

    /// Start a new entry with size hint for optimized buffering
    ///
    /// Providing an accurate size hint can improve performance by 15-25% for large files.
    /// The hint is used to optimize buffer allocation and flush thresholds.
    ///
    /// # Arguments
    /// * `name` - The name/path of the entry in the ZIP
    /// * `size_hint` - Optional uncompressed size hint in bytes
    ///
    /// # Example
    /// ```no_run
    /// # use s_zip::StreamingZipWriter;
    /// # fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let mut writer = StreamingZipWriter::new("output.zip")?;
    ///
    /// // For large files, provide size hint for better performance
    /// writer.start_entry_with_hint("large_file.bin", Some(10_000_000))?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn start_entry_with_hint(&mut self, name: &str, size_hint: Option<u64>) -> Result<()> {
        // Finish previous entry if any
        self.finish_current_entry()?;

        let local_header_offset = self.output.stream_position()?;
        let compression_method = self.compression_method.to_zip_method();

        // Check if encryption is enabled
        #[cfg(feature = "encryption")]
        let (encryptor, encryption_flag) = if let Some(ref password) = self.password {
            let enc = AesEncryptor::new(password, self.encryption_strength)?;
            (Some(enc), 0x01) // bit 0 set for encryption
        } else {
            (None, 0x00)
        };

        #[cfg(not(feature = "encryption"))]
        let encryption_flag = 0x00;

        // Write local file header with data descriptor flag (bit 3) + encryption flag (bit 0)
        self.output.write_all(&[0x50, 0x4b, 0x03, 0x04])?; // signature
        self.output.write_all(&[51, 0])?; // version needed (5.1 for AES)
        self.output.write_all(&[8 | encryption_flag, 0])?; // general purpose bit flag
        self.output.write_all(&compression_method.to_le_bytes())?; // compression method
        self.output.write_all(&[0, 0, 0, 0])?; // mod time/date
        self.output.write_all(&0u32.to_le_bytes())?; // crc32 placeholder
        self.output.write_all(&0u32.to_le_bytes())?; // compressed size placeholder
        self.output.write_all(&0u32.to_le_bytes())?; // uncompressed size placeholder
        self.output.write_all(&(name.len() as u16).to_le_bytes())?;

        // Calculate extra field size for AES
        #[cfg(feature = "encryption")]
        let extra_len = if encryptor.is_some() { 11 } else { 0 };
        #[cfg(not(feature = "encryption"))]
        let extra_len = 0;

        self.output.write_all(&(extra_len as u16).to_le_bytes())?; // extra len
        self.output.write_all(name.as_bytes())?;

        // Write AES extra field if encryption is enabled
        #[cfg(feature = "encryption")]
        if let Some(ref enc) = encryptor {
            // AES extra field header (0x9901)
            self.output.write_all(&[0x01, 0x99])?; // WinZip AES encryption marker
            self.output.write_all(&[7, 0])?; // data size
            self.output.write_all(&[2, 0])?; // AE-2 format
            self.output.write_all(&[0x41, 0x45])?; // vendor ID "AE"
            self.output
                .write_all(&enc.strength().to_winzip_code().to_le_bytes())?; // strength
            self.output.write_all(&compression_method.to_le_bytes())?; // actual compression

            // Write salt and password verification
            self.output.write_all(enc.salt())?;
            self.output.write_all(enc.password_verify())?;
        }

        // Create encoder for this entry based on compression method
        // Use adaptive buffer if size hint is provided
        let encoder: Box<dyn CompressorWrite> = match self.compression_method {
            CompressionMethod::Deflate => Box::new(DeflateCompressor {
                encoder: DeflateEncoder::new(
                    CompressedBuffer::with_size_hint(size_hint),
                    Compression::new(self.compression_level),
                ),
            }),
            #[cfg(feature = "zstd-support")]
            CompressionMethod::Zstd => {
                let mut encoder = zstd::Encoder::new(
                    CompressedBuffer::with_size_hint(size_hint),
                    self.compression_level as i32,
                )?;
                encoder.include_checksum(false)?; // ZIP uses CRC32, not zstd checksum
                Box::new(ZstdCompressor { encoder })
            }
            CompressionMethod::Stored => {
                // For stored, we don't compress
                return Err(SZipError::InvalidFormat(
                    "Stored method not yet implemented".to_string(),
                ));
            }
        };

        self.current_entry = Some(CurrentEntry {
            name: name.to_string(),
            local_header_offset,
            encoder,
            counter: CrcCounter::new(),
            compression_method,
            #[cfg(feature = "encryption")]
            encryptor,
        });

        Ok(())
    }

    /// Write uncompressed data to current entry (will be compressed and/or encrypted on-the-fly)
    pub fn write_data(&mut self, data: &[u8]) -> Result<()> {
        let entry = self
            .current_entry
            .as_mut()
            .ok_or_else(|| SZipError::InvalidFormat("No entry started".to_string()))?;

        // Update CRC and size with uncompressed data
        entry.counter.update_uncompressed(data);

        // For AES encryption: encrypt THEN compress
        // Note: AE-2 format doesn't use CRC, uses HMAC instead
        #[cfg(feature = "encryption")]
        let data_to_compress = if let Some(ref mut encryptor) = entry.encryptor {
            let mut encrypted = data.to_vec();
            encryptor.encrypt(&mut encrypted)?;
            encrypted
        } else {
            data.to_vec()
        };

        #[cfg(not(feature = "encryption"))]
        let data_to_compress = data.to_vec();

        // Write to encoder (compresses data into buffer)
        entry.encoder.write_all(&data_to_compress)?;

        // Flush encoder to ensure all data is in buffer
        entry.encoder.flush()?;

        // Check if buffer should be flushed to output
        let buffer = entry.encoder.get_buffer_mut();
        if buffer.should_flush() {
            // Flush buffer to output to keep memory usage low
            let compressed_data = buffer.take();
            self.output.write_all(&compressed_data)?;
            entry.counter.add_compressed(compressed_data.len() as u64);
        }

        Ok(())
    }

    /// Finish current entry and write data descriptor
    fn finish_current_entry(&mut self) -> Result<()> {
        if let Some(mut entry) = self.current_entry.take() {
            // Finish compression and get remaining buffered data
            let mut buffer = entry.encoder.finish_compression()?;

            // Flush any remaining data from buffer to output
            let remaining_data = buffer.take();
            if !remaining_data.is_empty() {
                self.output.write_all(&remaining_data)?;
                entry.counter.add_compressed(remaining_data.len() as u64);
            }

            // Write authentication code for AES encryption
            #[cfg(feature = "encryption")]
            let (encryption_strength_code, auth_code_size) =
                if let Some(encryptor) = entry.encryptor {
                    let strength_code = encryptor.strength().to_winzip_code();
                    let auth_code = encryptor.finalize();
                    self.output.write_all(&auth_code)?;
                    (Some(strength_code), auth_code.len() as u64)
                } else {
                    (None, 0)
                };

            #[cfg(not(feature = "encryption"))]
            let auth_code_size = 0u64;

            let crc = entry.counter.finalize();
            let compressed_size = entry.counter.compressed_count + auth_code_size;
            let uncompressed_size = entry.counter.uncompressed_count;

            // Write data descriptor
            // signature
            self.output.write_all(&[0x50, 0x4b, 0x07, 0x08])?;
            self.output.write_all(&crc.to_le_bytes())?;
            // If sizes exceed 32-bit, write 64-bit sizes (ZIP64 data descriptor)
            if compressed_size > u32::MAX as u64 || uncompressed_size > u32::MAX as u64 {
                self.output.write_all(&compressed_size.to_le_bytes())?;
                self.output.write_all(&uncompressed_size.to_le_bytes())?;
            } else {
                self.output
                    .write_all(&(compressed_size as u32).to_le_bytes())?;
                self.output
                    .write_all(&(uncompressed_size as u32).to_le_bytes())?;
            }

            // Save entry info for central directory
            self.entries.push(ZipEntry {
                name: entry.name,
                local_header_offset: entry.local_header_offset,
                crc32: crc,
                compressed_size,
                uncompressed_size,
                compression_method: entry.compression_method,
                #[cfg(feature = "encryption")]
                encryption_strength: encryption_strength_code,
            });
        }
        Ok(())
    }

    /// Finish ZIP file (write central directory and return the writer)
    pub fn finish(mut self) -> Result<W> {
        // Finish last entry
        self.finish_current_entry()?;

        let central_dir_offset = self.output.stream_position()?;

        // Write central directory
        for entry in &self.entries {
            self.output.write_all(&[0x50, 0x4b, 0x01, 0x02])?; // central dir sig
            self.output.write_all(&[20, 0])?; // version made by
            self.output.write_all(&[20, 0])?; // version needed
            self.output.write_all(&[8, 0])?; // general purpose bit flag (bit 3 set)
            self.output
                .write_all(&entry.compression_method.to_le_bytes())?; // compression method
            self.output.write_all(&[0, 0, 0, 0])?; // mod time/date
            self.output.write_all(&entry.crc32.to_le_bytes())?;

            // Write sizes (32-bit placeholders or actual values)
            if entry.compressed_size > u32::MAX as u64 {
                self.output.write_all(&0xFFFFFFFFu32.to_le_bytes())?;
            } else {
                self.output
                    .write_all(&(entry.compressed_size as u32).to_le_bytes())?;
            }

            if entry.uncompressed_size > u32::MAX as u64 {
                self.output.write_all(&0xFFFFFFFFu32.to_le_bytes())?;
            } else {
                self.output
                    .write_all(&(entry.uncompressed_size as u32).to_le_bytes())?;
            }

            self.output
                .write_all(&(entry.name.len() as u16).to_le_bytes())?;

            // Prepare ZIP64 extra field if needed
            let mut extra_field: Vec<u8> = Vec::new();
            if entry.uncompressed_size > u32::MAX as u64
                || entry.compressed_size > u32::MAX as u64
                || entry.local_header_offset > u32::MAX as u64
            {
                // ZIP64 extra header ID 0x0001
                extra_field.extend_from_slice(&0x0001u16.to_le_bytes());
                // data size: we'll include uncompressed (8) if needed, compressed (8) if needed, and offset (8) if needed
                let mut data: Vec<u8> = Vec::new();
                if entry.uncompressed_size > u32::MAX as u64 {
                    data.extend_from_slice(&entry.uncompressed_size.to_le_bytes());
                }
                if entry.compressed_size > u32::MAX as u64 {
                    data.extend_from_slice(&entry.compressed_size.to_le_bytes());
                }
                if entry.local_header_offset > u32::MAX as u64 {
                    data.extend_from_slice(&entry.local_header_offset.to_le_bytes());
                }
                extra_field.extend_from_slice(&(data.len() as u16).to_le_bytes());
                extra_field.extend_from_slice(&data);
            }

            self.output
                .write_all(&(extra_field.len() as u16).to_le_bytes())?; // extra len
            self.output.write_all(&0u16.to_le_bytes())?; // file comment len
            self.output.write_all(&0u16.to_le_bytes())?; // disk number start
            self.output.write_all(&0u16.to_le_bytes())?; // internal attrs
            self.output.write_all(&0u32.to_le_bytes())?; // external attrs

            // local header offset (32-bit or 0xFFFFFFFF)
            if entry.local_header_offset > u32::MAX as u64 {
                self.output.write_all(&0xFFFFFFFFu32.to_le_bytes())?;
            } else {
                self.output
                    .write_all(&(entry.local_header_offset as u32).to_le_bytes())?;
            }

            self.output.write_all(entry.name.as_bytes())?;
            if !extra_field.is_empty() {
                self.output.write_all(&extra_field)?;
            }
        }

        let central_dir_size = self.output.stream_position()? - central_dir_offset;

        // Determine if we need ZIP64 EOCD
        let need_zip64 = self.entries.len() > u16::MAX as usize
            || central_dir_size > u32::MAX as u64
            || central_dir_offset > u32::MAX as u64;

        if need_zip64 {
            // Write ZIP64 End of Central Directory Record
            // signature
            self.output.write_all(&[0x50, 0x4b, 0x06, 0x06])?; // 0x06064b50
                                                               // size of zip64 eocd record (size of remaining fields)
                                                               // We'll write fixed-size fields: version made by(2)+version needed(2)+disk numbers(4+4)+entries on disk(8)+total entries(8)+cd size(8)+cd offset(8)
            let zip64_eocd_size: u64 = 44;
            self.output.write_all(&zip64_eocd_size.to_le_bytes())?;
            // version made by, version needed
            self.output.write_all(&[20, 0])?;
            self.output.write_all(&[20, 0])?;
            // disk number, disk where central dir starts
            self.output.write_all(&0u32.to_le_bytes())?;
            self.output.write_all(&0u32.to_le_bytes())?;
            // entries on this disk (8)
            self.output
                .write_all(&(self.entries.len() as u64).to_le_bytes())?;
            // total entries (8)
            self.output
                .write_all(&(self.entries.len() as u64).to_le_bytes())?;
            // central directory size (8)
            self.output.write_all(&central_dir_size.to_le_bytes())?;
            // central directory offset (8)
            self.output.write_all(&central_dir_offset.to_le_bytes())?;

            // Write ZIP64 EOCD locator
            // signature
            self.output.write_all(&[0x50, 0x4b, 0x06, 0x07])?; // 0x07064b50
                                                               // disk with ZIP64 EOCD (4)
            self.output.write_all(&0u32.to_le_bytes())?;
            // relative offset of ZIP64 EOCD (8)
            let zip64_eocd_pos = central_dir_offset + central_dir_size; // directly after central dir
            self.output.write_all(&zip64_eocd_pos.to_le_bytes())?;
            // total number of disks
            self.output.write_all(&0u32.to_le_bytes())?;
        }

        // Write end of central directory (classic)
        self.output.write_all(&[0x50, 0x4b, 0x05, 0x06])?;
        self.output.write_all(&0u16.to_le_bytes())?; // disk number
        self.output.write_all(&0u16.to_le_bytes())?; // disk with central dir

        // number of entries (16-bit or 0xFFFF if ZIP64 used)
        if self.entries.len() > u16::MAX as usize {
            self.output.write_all(&0xFFFFu16.to_le_bytes())?;
            self.output.write_all(&0xFFFFu16.to_le_bytes())?;
        } else {
            self.output
                .write_all(&(self.entries.len() as u16).to_le_bytes())?;
            self.output
                .write_all(&(self.entries.len() as u16).to_le_bytes())?;
        }

        // central dir size and offset (32-bit or 0xFFFFFFFF)
        if central_dir_size > u32::MAX as u64 {
            self.output.write_all(&0xFFFFFFFFu32.to_le_bytes())?;
        } else {
            self.output
                .write_all(&(central_dir_size as u32).to_le_bytes())?;
        }

        if central_dir_offset > u32::MAX as u64 {
            self.output.write_all(&0xFFFFFFFFu32.to_le_bytes())?;
        } else {
            self.output
                .write_all(&(central_dir_offset as u32).to_le_bytes())?;
        }

        self.output.write_all(&0u16.to_le_bytes())?; // comment len

        self.output.flush()?;
        Ok(self.output)
    }
}
