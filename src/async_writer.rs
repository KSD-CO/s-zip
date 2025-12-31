//! Async streaming ZIP writer that compresses data on-the-fly without temp files
//!
//! This module provides async/await versions of the ZIP writer, compatible with
//! the Tokio runtime. It eliminates:
//! - Temp file disk I/O
//! - File read buffers
//! - Intermediate storage
//!
//! Expected RAM savings: 5-8 MB per file
//!
//! Supports arbitrary async writers (File, Vec<u8>, network streams, etc.)

use crate::error::{Result, SZipError};
use crate::writer::CompressionMethod;
use async_compression::tokio::write::DeflateEncoder;
#[cfg(feature = "async-zstd")]
use async_compression::tokio::write::ZstdEncoder;
use crc32fast::Hasher as Crc32;
use std::io::Write;
use std::path::Path;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::{AsyncSeek, AsyncSeekExt, AsyncWrite, AsyncWriteExt};

/// Entry being written to ZIP
struct ZipEntry {
    name: String,
    local_header_offset: u64,
    crc32: u32,
    compressed_size: u64,
    uncompressed_size: u64,
    compression_method: u16,
}

/// Async streaming ZIP writer that compresses data on-the-fly
pub struct AsyncStreamingZipWriter<W: AsyncWrite + AsyncSeek + Unpin> {
    output: W,
    entries: Vec<ZipEntry>,
    current_entry: Option<CurrentEntry>,
    compression_level: u32,
    compression_method: CompressionMethod,
}

struct CurrentEntry {
    name: String,
    local_header_offset: u64,
    encoder: Box<dyn AsyncCompressorWrite>,
    counter: CrcCounter,
    compression_method: u16,
}

/// Trait for async compression encoders
trait AsyncCompressorWrite: AsyncWrite + Unpin + Send {
    fn finish_compression(
        self: Box<Self>,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<CompressedBuffer>> + Send>>;
    fn get_buffer_mut(&mut self) -> &mut CompressedBuffer;
}

struct DeflateCompressor {
    encoder: DeflateEncoder<CompressedBuffer>,
}

impl AsyncWrite for DeflateCompressor {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.encoder).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.encoder).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.encoder).poll_shutdown(cx)
    }
}

impl AsyncCompressorWrite for DeflateCompressor {
    fn finish_compression(
        mut self: Box<Self>,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<CompressedBuffer>> + Send>> {
        Box::pin(async move {
            self.encoder.shutdown().await?;
            Ok(self.encoder.into_inner())
        })
    }

    fn get_buffer_mut(&mut self) -> &mut CompressedBuffer {
        self.encoder.get_mut()
    }
}

#[cfg(feature = "async-zstd")]
struct ZstdCompressor {
    encoder: ZstdEncoder<CompressedBuffer>,
}

#[cfg(feature = "async-zstd")]
impl AsyncWrite for ZstdCompressor {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.encoder).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.encoder).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.encoder).poll_shutdown(cx)
    }
}

#[cfg(feature = "async-zstd")]
impl AsyncCompressorWrite for ZstdCompressor {
    fn finish_compression(
        mut self: Box<Self>,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<CompressedBuffer>> + Send>> {
        Box::pin(async move {
            self.encoder.shutdown().await?;
            Ok(self.encoder.into_inner())
        })
    }

    fn get_buffer_mut(&mut self) -> &mut CompressedBuffer {
        self.encoder.get_mut()
    }
}

/// Metadata tracker for CRC and byte counts (reused from sync version)
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

/// Buffered writer for compressed data with size limit
pub struct CompressedBuffer {
    buffer: Vec<u8>,
    flush_threshold: usize,
}

impl CompressedBuffer {
    fn new() -> Self {
        Self {
            buffer: Vec::with_capacity(64 * 1024), // 64KB initial capacity
            flush_threshold: 1024 * 1024,          // 1MB threshold
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

impl AsyncWrite for CompressedBuffer {
    fn poll_write(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        self.buffer.extend_from_slice(buf);
        Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

impl AsyncStreamingZipWriter<tokio::fs::File> {
    /// Create a new async ZIP writer with default compression level (6) using DEFLATE
    pub async fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        Self::with_compression(path, 6).await
    }

    /// Create a new async ZIP writer with custom compression level (0-9) using DEFLATE
    pub async fn with_compression<P: AsRef<Path>>(path: P, compression_level: u32) -> Result<Self> {
        Self::with_method(path, CompressionMethod::Deflate, compression_level).await
    }

    /// Create a new async ZIP writer with specified compression method and level
    ///
    /// # Arguments
    /// * `path` - Path to the output ZIP file
    /// * `method` - Compression method to use (Deflate, Zstd, or Stored)
    /// * `compression_level` - Compression level (0-9 for DEFLATE, 1-21 for Zstd)
    pub async fn with_method<P: AsRef<Path>>(
        path: P,
        method: CompressionMethod,
        compression_level: u32,
    ) -> Result<Self> {
        let output = tokio::fs::File::create(path).await?;
        Ok(Self {
            output,
            entries: Vec::new(),
            current_entry: None,
            compression_level,
            compression_method: method,
        })
    }

    /// Create a new async ZIP writer with Zstd compression (requires async-zstd feature)
    #[cfg(feature = "async-zstd")]
    pub async fn with_zstd<P: AsRef<Path>>(path: P, compression_level: i32) -> Result<Self> {
        let output = tokio::fs::File::create(path).await?;
        Ok(Self {
            output,
            entries: Vec::new(),
            current_entry: None,
            compression_level: compression_level as u32,
            compression_method: CompressionMethod::Zstd,
        })
    }
}

impl<W: AsyncWrite + AsyncSeek + Unpin> AsyncStreamingZipWriter<W> {
    /// Create a new async ZIP writer from an arbitrary writer with default compression level (6) using DEFLATE
    pub fn from_writer(writer: W) -> Self {
        Self::from_writer_with_compression(writer, 6)
    }

    /// Create a new async ZIP writer from an arbitrary writer with custom compression level
    pub fn from_writer_with_compression(writer: W, compression_level: u32) -> Self {
        Self::from_writer_with_method(writer, CompressionMethod::Deflate, compression_level)
    }

    /// Create a new async ZIP writer from an arbitrary writer with specified compression method and level
    ///
    /// # Arguments
    /// * `writer` - Any writer implementing AsyncWrite + AsyncSeek + Unpin
    /// * `method` - Compression method to use (Deflate, Zstd, or Stored)
    /// * `compression_level` - Compression level (0-9 for DEFLATE, 1-21 for Zstd)
    pub fn from_writer_with_method(
        writer: W,
        method: CompressionMethod,
        compression_level: u32,
    ) -> Self {
        Self {
            output: writer,
            entries: Vec::new(),
            current_entry: None,
            compression_level,
            compression_method: method,
        }
    }

    /// Start a new entry (file) in the ZIP
    pub async fn start_entry(&mut self, name: &str) -> Result<()> {
        // Finish previous entry if any
        self.finish_current_entry().await?;

        let local_header_offset = self.output.stream_position().await?;
        let compression_method = self.compression_method.to_zip_method();

        // Write local file header with data descriptor flag (bit 3)
        self.output.write_all(&[0x50, 0x4b, 0x03, 0x04]).await?; // signature
        self.output.write_all(&[20, 0]).await?; // version needed
        self.output.write_all(&[8, 0]).await?; // general purpose bit flag (bit 3 set)
        self.output
            .write_all(&compression_method.to_le_bytes())
            .await?; // compression method
        self.output.write_all(&[0, 0, 0, 0]).await?; // mod time/date
        self.output.write_all(&0u32.to_le_bytes()).await?; // crc32 placeholder
        self.output.write_all(&0u32.to_le_bytes()).await?; // compressed size placeholder
        self.output.write_all(&0u32.to_le_bytes()).await?; // uncompressed size placeholder
        self.output
            .write_all(&(name.len() as u16).to_le_bytes())
            .await?;
        self.output.write_all(&0u16.to_le_bytes()).await?; // extra len
        self.output.write_all(name.as_bytes()).await?;

        // Create encoder for this entry based on compression method
        let encoder: Box<dyn AsyncCompressorWrite> = match self.compression_method {
            CompressionMethod::Deflate => {
                let level = match self.compression_level {
                    0 => async_compression::Level::Fastest,
                    1..=3 => async_compression::Level::Precise(self.compression_level as i32),
                    4..=6 => async_compression::Level::Default,
                    7..=9 => async_compression::Level::Best,
                    _ => async_compression::Level::Default,
                };
                Box::new(DeflateCompressor {
                    encoder: DeflateEncoder::with_quality(CompressedBuffer::new(), level),
                })
            }
            #[cfg(feature = "async-zstd")]
            CompressionMethod::Zstd => {
                let level = async_compression::Level::Precise(self.compression_level as i32);
                Box::new(ZstdCompressor {
                    encoder: ZstdEncoder::with_quality(CompressedBuffer::new(), level),
                })
            }
            CompressionMethod::Stored => {
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
        });

        Ok(())
    }

    /// Write uncompressed data to current entry (will be compressed on-the-fly)
    pub async fn write_data(&mut self, data: &[u8]) -> Result<()> {
        let entry = self
            .current_entry
            .as_mut()
            .ok_or_else(|| SZipError::InvalidFormat("No entry started".to_string()))?;

        // Update CRC and size with uncompressed data
        entry.counter.update_uncompressed(data);

        // Write to encoder (compresses data into buffer)
        entry.encoder.write_all(data).await?;

        // Flush encoder to ensure all data is in buffer
        entry.encoder.flush().await?;

        // Check if buffer should be flushed to output
        let buffer = entry.encoder.get_buffer_mut();
        if buffer.should_flush() {
            // Flush buffer to output to keep memory usage low
            let compressed_data = buffer.take();
            self.output.write_all(&compressed_data).await?;
            entry.counter.add_compressed(compressed_data.len() as u64);
        }

        Ok(())
    }

    /// Finish current entry and write data descriptor
    async fn finish_current_entry(&mut self) -> Result<()> {
        if let Some(mut entry) = self.current_entry.take() {
            // Finish compression and get remaining buffered data
            let mut buffer = entry.encoder.finish_compression().await?;

            // Flush any remaining data from buffer to output
            let remaining_data = buffer.take();
            if !remaining_data.is_empty() {
                self.output.write_all(&remaining_data).await?;
                entry.counter.add_compressed(remaining_data.len() as u64);
            }

            let crc = entry.counter.finalize();
            let compressed_size = entry.counter.compressed_count;
            let uncompressed_size = entry.counter.uncompressed_count;

            // Write data descriptor
            self.output.write_all(&[0x50, 0x4b, 0x07, 0x08]).await?; // signature
            self.output.write_all(&crc.to_le_bytes()).await?;
            // If sizes exceed 32-bit, write 64-bit sizes (ZIP64 data descriptor)
            if compressed_size > u32::MAX as u64 || uncompressed_size > u32::MAX as u64 {
                self.output
                    .write_all(&compressed_size.to_le_bytes())
                    .await?;
                self.output
                    .write_all(&uncompressed_size.to_le_bytes())
                    .await?;
            } else {
                self.output
                    .write_all(&(compressed_size as u32).to_le_bytes())
                    .await?;
                self.output
                    .write_all(&(uncompressed_size as u32).to_le_bytes())
                    .await?;
            }

            // Save entry info for central directory
            self.entries.push(ZipEntry {
                name: entry.name,
                local_header_offset: entry.local_header_offset,
                crc32: crc,
                compressed_size,
                uncompressed_size,
                compression_method: entry.compression_method,
            });
        }
        Ok(())
    }

    /// Finish ZIP file (write central directory and return the writer)
    pub async fn finish(mut self) -> Result<W> {
        // Finish last entry
        self.finish_current_entry().await?;

        let central_dir_offset = self.output.stream_position().await?;

        // Write central directory
        for entry in &self.entries {
            self.output.write_all(&[0x50, 0x4b, 0x01, 0x02]).await?; // central dir sig
            self.output.write_all(&[20, 0]).await?; // version made by
            self.output.write_all(&[20, 0]).await?; // version needed
            self.output.write_all(&[8, 0]).await?; // general purpose bit flag (bit 3 set)
            self.output
                .write_all(&entry.compression_method.to_le_bytes())
                .await?; // compression method
            self.output.write_all(&[0, 0, 0, 0]).await?; // mod time/date
            self.output.write_all(&entry.crc32.to_le_bytes()).await?;

            // Write sizes (32-bit placeholders or actual values)
            if entry.compressed_size > u32::MAX as u64 {
                self.output.write_all(&0xFFFFFFFFu32.to_le_bytes()).await?;
            } else {
                self.output
                    .write_all(&(entry.compressed_size as u32).to_le_bytes())
                    .await?;
            }

            if entry.uncompressed_size > u32::MAX as u64 {
                self.output.write_all(&0xFFFFFFFFu32.to_le_bytes()).await?;
            } else {
                self.output
                    .write_all(&(entry.uncompressed_size as u32).to_le_bytes())
                    .await?;
            }

            self.output
                .write_all(&(entry.name.len() as u16).to_le_bytes())
                .await?;

            // Prepare ZIP64 extra field if needed
            let mut extra_field: Vec<u8> = Vec::new();
            if entry.uncompressed_size > u32::MAX as u64
                || entry.compressed_size > u32::MAX as u64
                || entry.local_header_offset > u32::MAX as u64
            {
                // ZIP64 extra header ID 0x0001
                extra_field.extend_from_slice(&0x0001u16.to_le_bytes());
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
                .write_all(&(extra_field.len() as u16).to_le_bytes())
                .await?; // extra len
            self.output.write_all(&0u16.to_le_bytes()).await?; // file comment len
            self.output.write_all(&0u16.to_le_bytes()).await?; // disk number start
            self.output.write_all(&0u16.to_le_bytes()).await?; // internal attrs
            self.output.write_all(&0u32.to_le_bytes()).await?; // external attrs

            // local header offset (32-bit or 0xFFFFFFFF)
            if entry.local_header_offset > u32::MAX as u64 {
                self.output.write_all(&0xFFFFFFFFu32.to_le_bytes()).await?;
            } else {
                self.output
                    .write_all(&(entry.local_header_offset as u32).to_le_bytes())
                    .await?;
            }

            self.output.write_all(entry.name.as_bytes()).await?;
            if !extra_field.is_empty() {
                self.output.write_all(&extra_field).await?;
            }
        }

        let central_dir_size = self.output.stream_position().await? - central_dir_offset;

        // Determine if we need ZIP64 EOCD
        let need_zip64 = self.entries.len() > u16::MAX as usize
            || central_dir_size > u32::MAX as u64
            || central_dir_offset > u32::MAX as u64;

        if need_zip64 {
            // Write ZIP64 End of Central Directory Record
            self.output.write_all(&[0x50, 0x4b, 0x06, 0x06]).await?;
            let zip64_eocd_size: u64 = 44;
            self.output
                .write_all(&zip64_eocd_size.to_le_bytes())
                .await?;
            self.output.write_all(&[20, 0]).await?;
            self.output.write_all(&[20, 0]).await?;
            self.output.write_all(&0u32.to_le_bytes()).await?;
            self.output.write_all(&0u32.to_le_bytes()).await?;
            self.output
                .write_all(&(self.entries.len() as u64).to_le_bytes())
                .await?;
            self.output
                .write_all(&(self.entries.len() as u64).to_le_bytes())
                .await?;
            self.output
                .write_all(&central_dir_size.to_le_bytes())
                .await?;
            self.output
                .write_all(&central_dir_offset.to_le_bytes())
                .await?;

            // Write ZIP64 EOCD locator
            self.output.write_all(&[0x50, 0x4b, 0x06, 0x07]).await?;
            self.output.write_all(&0u32.to_le_bytes()).await?;
            let zip64_eocd_pos = central_dir_offset + central_dir_size;
            self.output.write_all(&zip64_eocd_pos.to_le_bytes()).await?;
            self.output.write_all(&0u32.to_le_bytes()).await?;
        }

        // Write end of central directory (classic)
        self.output.write_all(&[0x50, 0x4b, 0x05, 0x06]).await?;
        self.output.write_all(&0u16.to_le_bytes()).await?; // disk number
        self.output.write_all(&0u16.to_le_bytes()).await?; // disk with central dir

        // number of entries (16-bit or 0xFFFF if ZIP64 used)
        if self.entries.len() > u16::MAX as usize {
            self.output.write_all(&0xFFFFu16.to_le_bytes()).await?;
            self.output.write_all(&0xFFFFu16.to_le_bytes()).await?;
        } else {
            self.output
                .write_all(&(self.entries.len() as u16).to_le_bytes())
                .await?;
            self.output
                .write_all(&(self.entries.len() as u16).to_le_bytes())
                .await?;
        }

        // central dir size and offset (32-bit or 0xFFFFFFFF)
        if central_dir_size > u32::MAX as u64 {
            self.output.write_all(&0xFFFFFFFFu32.to_le_bytes()).await?;
        } else {
            self.output
                .write_all(&(central_dir_size as u32).to_le_bytes())
                .await?;
        }

        if central_dir_offset > u32::MAX as u64 {
            self.output.write_all(&0xFFFFFFFFu32.to_le_bytes()).await?;
        } else {
            self.output
                .write_all(&(central_dir_offset as u32).to_le_bytes())
                .await?;
        }

        self.output.write_all(&0u16.to_le_bytes()).await?; // comment len

        // CRITICAL: Must call shutdown() to ensure cloud uploads complete
        // For cloud writers like S3ZipWriter, shutdown() completes the multipart upload
        self.output.flush().await?;
        self.output.shutdown().await?;

        Ok(self.output)
    }
}
