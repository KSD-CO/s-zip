//! Generic async ZIP reader for arbitrary async sources (files, HTTP, S3, in-memory, etc.)
//!
//! This module provides a generic async ZIP reader that works with any source
//! implementing AsyncRead + AsyncSeek + Unpin + Send.

use crate::error::{Result, SZipError};
use async_compression::tokio::bufread::DeflateDecoder;
#[cfg(feature = "async-zstd")]
use async_compression::tokio::bufread::ZstdDecoder;
use std::io::SeekFrom;
use std::path::Path;
use tokio::fs::File;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncSeek, AsyncSeekExt, BufReader};

/// ZIP local file header signature
const LOCAL_FILE_HEADER_SIGNATURE: u32 = 0x04034b50;

/// ZIP central directory signature
const CENTRAL_DIRECTORY_SIGNATURE: u32 = 0x02014b50;

/// ZIP end of central directory signature
const END_OF_CENTRAL_DIRECTORY_SIGNATURE: u32 = 0x06054b50;

/// ZIP64 end of central directory record signature
const ZIP64_END_OF_CENTRAL_DIRECTORY_SIGNATURE: u32 = 0x06064b50;

/// Entry in the ZIP central directory
#[derive(Debug, Clone)]
pub struct ZipEntry {
    pub name: String,
    pub compressed_size: u64,
    pub uncompressed_size: u64,
    pub compression_method: u16,
    pub offset: u64,
}

/// Generic async streaming ZIP reader that works with any async reader + seeker
///
/// Supports adaptive buffering for optimized read performance based on file size.
pub struct GenericAsyncZipReader<R: AsyncRead + AsyncSeek + Unpin + Send> {
    reader: BufReader<R>,
    entries: Vec<ZipEntry>,
}

/// Type alias for file-based async ZIP reader (convenience)
pub type AsyncStreamingZipReader = GenericAsyncZipReader<File>;

impl AsyncStreamingZipReader {
    /// Open a ZIP file and read its central directory with default buffer
    pub async fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        Self::open_with_buffer_size(path, None).await
    }

    /// Open a ZIP file with custom buffer size for optimized reading
    ///
    /// Providing a buffer size hint can improve read performance:
    /// - Small ZIPs (<10MB): 64KB buffer
    /// - Medium ZIPs (<100MB): 256KB buffer  
    /// - Large ZIPs (≥100MB): 1MB buffer (default)
    ///
    /// # Example
    /// ```no_run
    /// # use s_zip::AsyncStreamingZipReader;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// // Optimize for very large ZIP files
    /// let reader = AsyncStreamingZipReader::open_with_buffer_size(
    ///     "huge_archive.zip",
    ///     Some(2 * 1024 * 1024) // 2MB buffer
    /// ).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn open_with_buffer_size<P: AsRef<Path>>(
        path: P,
        buffer_size: Option<usize>,
    ) -> Result<Self> {
        let file = File::open(path).await?;
        GenericAsyncZipReader::new_with_buffer_size(file, buffer_size).await
    }
}

impl<R: AsyncRead + AsyncSeek + Unpin + Send> GenericAsyncZipReader<R> {
    /// Create a new generic async ZIP reader from any reader that supports AsyncRead + AsyncSeek
    pub async fn new(reader: R) -> Result<Self> {
        Self::new_with_buffer_size(reader, None).await
    }

    /// Create a new generic async ZIP reader with custom buffer size
    ///
    /// Allows fine-tuning read performance based on expected data patterns.
    pub async fn new_with_buffer_size(reader: R, buffer_size: Option<usize>) -> Result<Self> {
        // Use adaptive buffer size
        let buf_size = buffer_size.unwrap_or(1024 * 1024); // Default 1MB for async
        let mut reader = BufReader::with_capacity(buf_size, reader);

        // Find and read central directory
        let entries = Self::read_central_directory(&mut reader).await?;

        Ok(GenericAsyncZipReader { reader, entries })
    }

    /// Get list of all entries in the ZIP
    pub fn entries(&self) -> &[ZipEntry] {
        &self.entries
    }

    /// Find an entry by name
    pub fn find_entry(&self, name: &str) -> Option<&ZipEntry> {
        self.entries.iter().find(|e| e.name == name)
    }

    /// Read an entry's decompressed data into a vector
    pub async fn read_entry(&mut self, entry: &ZipEntry) -> Result<Vec<u8>> {
        // Seek to local file header
        self.reader.seek(SeekFrom::Start(entry.offset)).await?;

        // Read and verify local file header
        let signature = self.read_u32_le().await?;
        if signature != LOCAL_FILE_HEADER_SIGNATURE {
            return Err(SZipError::InvalidFormat(
                "Invalid local file header signature".to_string(),
            ));
        }

        // Skip version, flags, compression method
        self.reader.seek(SeekFrom::Current(6)).await?;

        // Skip modification time and date, CRC-32
        self.reader.seek(SeekFrom::Current(8)).await?;

        // Read compressed and uncompressed sizes (already known from central directory)
        self.reader.seek(SeekFrom::Current(8)).await?;

        // Read filename length and extra field length
        let filename_len = self.read_u16_le().await? as i64;
        let extra_len = self.read_u16_le().await? as i64;

        // Skip filename and extra field
        self.reader
            .seek(SeekFrom::Current(filename_len + extra_len))
            .await?;

        // Now read the compressed data
        let mut compressed_data = vec![0u8; entry.compressed_size as usize];
        self.reader.read_exact(&mut compressed_data).await?;

        // Decompress if needed
        let data = if entry.compression_method == 8 {
            // DEFLATE compression
            let mut decoder = DeflateDecoder::new(&compressed_data[..]);
            let mut decompressed = Vec::new();
            decoder.read_to_end(&mut decompressed).await?;
            decompressed
        } else if entry.compression_method == 0 {
            // No compression (stored)
            compressed_data
        } else if entry.compression_method == 93 {
            // Zstd compression
            #[cfg(feature = "async-zstd")]
            {
                let mut decoder = ZstdDecoder::new(&compressed_data[..]);
                let mut decompressed = Vec::new();
                decoder.read_to_end(&mut decompressed).await?;
                decompressed
            }
            #[cfg(not(feature = "async-zstd"))]
            {
                return Err(SZipError::UnsupportedCompression(entry.compression_method));
            }
        } else {
            return Err(SZipError::UnsupportedCompression(entry.compression_method));
        };

        Ok(data)
    }

    /// Read an entry by name
    pub async fn read_entry_by_name(&mut self, name: &str) -> Result<Vec<u8>> {
        let entry = self
            .find_entry(name)
            .ok_or_else(|| SZipError::EntryNotFound(name.to_string()))?
            .clone();

        self.read_entry(&entry).await
    }

    /// Get a streaming reader for an entry by name (for large files)
    /// Returns a reader that decompresses data on-the-fly without loading everything into memory
    pub async fn read_entry_streaming_by_name(
        &mut self,
        name: &str,
    ) -> Result<Box<dyn AsyncRead + Unpin + Send + '_>> {
        let entry = self
            .find_entry(name)
            .ok_or_else(|| SZipError::EntryNotFound(name.to_string()))?
            .clone();

        self.read_entry_streaming(&entry).await
    }

    /// Get a streaming reader for an entry (for large files)
    /// Returns a reader that decompresses data on-the-fly without loading everything into memory
    pub async fn read_entry_streaming(
        &mut self,
        entry: &ZipEntry,
    ) -> Result<Box<dyn AsyncRead + Unpin + Send + '_>> {
        // Seek to local file header
        self.reader.seek(SeekFrom::Start(entry.offset)).await?;

        // Read and verify local file header
        let signature = self.read_u32_le().await?;
        if signature != LOCAL_FILE_HEADER_SIGNATURE {
            return Err(SZipError::InvalidFormat(
                "Invalid local file header signature".to_string(),
            ));
        }

        // Skip version, flags, compression method
        self.reader.seek(SeekFrom::Current(6)).await?;

        // Skip modification time and date, CRC-32
        self.reader.seek(SeekFrom::Current(8)).await?;

        // Read compressed and uncompressed sizes
        self.reader.seek(SeekFrom::Current(8)).await?;

        // Read filename length and extra field length
        let filename_len = self.read_u16_le().await? as i64;
        let extra_len = self.read_u16_le().await? as i64;

        // Skip filename and extra field
        self.reader
            .seek(SeekFrom::Current(filename_len + extra_len))
            .await?;

        // Create a reader limited to compressed data size
        let limited_reader = (&mut self.reader).take(entry.compressed_size);

        // Wrap with decompressor if needed
        if entry.compression_method == 8 {
            // DEFLATE compression
            Ok(Box::new(DeflateDecoder::new(BufReader::new(
                limited_reader,
            ))))
        } else if entry.compression_method == 0 {
            // No compression (stored)
            Ok(Box::new(limited_reader))
        } else if entry.compression_method == 93 {
            // Zstd compression
            #[cfg(feature = "async-zstd")]
            {
                Ok(Box::new(ZstdDecoder::new(BufReader::new(limited_reader))))
            }
            #[cfg(not(feature = "async-zstd"))]
            {
                Err(SZipError::UnsupportedCompression(entry.compression_method))
            }
        } else {
            Err(SZipError::UnsupportedCompression(entry.compression_method))
        }
    }

    /// Get a streaming reader for an entry by name
    pub async fn read_entry_by_name_streaming(
        &mut self,
        name: &str,
    ) -> Result<Box<dyn AsyncRead + Unpin + Send + '_>> {
        let entry = self
            .find_entry(name)
            .ok_or_else(|| SZipError::EntryNotFound(name.to_string()))?
            .clone();

        self.read_entry_streaming(&entry).await
    }

    /// Read the central directory from the ZIP file
    async fn read_central_directory(reader: &mut BufReader<R>) -> Result<Vec<ZipEntry>> {
        // Find end of central directory record
        let eocd_offset = Self::find_eocd(reader).await?;

        // Seek to EOCD
        reader.seek(SeekFrom::Start(eocd_offset)).await?;

        // Read EOCD
        let signature = Self::read_u32_le_static(reader).await?;
        if signature != END_OF_CENTRAL_DIRECTORY_SIGNATURE {
            return Err(SZipError::InvalidFormat(format!(
                "Invalid end of central directory signature: 0x{:08x}",
                signature
            )));
        }

        // Skip disk number fields (4 bytes)
        reader.seek(SeekFrom::Current(4)).await?;

        // Read number of entries on this disk (2 bytes)
        let _entries_on_disk = Self::read_u16_le_static(reader).await?;

        // Read total number of entries (2 bytes)
        // These values may be placeholder 0xFFFF/0xFFFFFFFF when ZIP64 is used
        let total_entries_16 = Self::read_u16_le_static(reader).await?;

        // Read central directory size (4 bytes)
        let cd_size_32 = Self::read_u32_le_static(reader).await?;

        // Read central directory offset (4 bytes)
        let cd_offset_32 = Self::read_u32_le_static(reader).await? as u64;

        // Promote to u64 and handle ZIP64 if markers present
        let mut total_entries = total_entries_16 as usize;
        let mut cd_offset = cd_offset_32;
        let _cd_size = cd_size_32 as u64;

        if total_entries_16 == 0xFFFF || cd_size_32 == 0xFFFFFFFF || cd_offset_32 == 0xFFFFFFFF {
            // Need to find ZIP64 EOCD locator and read ZIP64 EOCD record
            let (zip64_total_entries, zip64_cd_size, zip64_cd_offset) =
                Self::read_zip64_eocd(reader, eocd_offset).await?;
            total_entries = zip64_total_entries as usize;
            cd_offset = zip64_cd_offset;
            // _cd_size can be used if needed (zip64_cd_size)
            let _ = zip64_cd_size;
        }

        // Seek to central directory
        reader.seek(SeekFrom::Start(cd_offset)).await?;

        // Read all central directory entries
        let mut entries = Vec::with_capacity(total_entries);
        for _ in 0..total_entries {
            let signature = Self::read_u32_le_static(reader).await?;
            if signature != CENTRAL_DIRECTORY_SIGNATURE {
                break;
            }

            // Skip version made by, version needed, flags
            reader.seek(SeekFrom::Current(6)).await?;

            let compression_method = Self::read_u16_le_static(reader).await?;

            // Skip modification time, date, CRC-32
            reader.seek(SeekFrom::Current(8)).await?;

            // Read sizes as 32-bit placeholders (may be 0xFFFFFFFF meaning ZIP64)
            let compressed_size_32 = Self::read_u32_le_static(reader).await? as u64;
            let uncompressed_size_32 = Self::read_u32_le_static(reader).await? as u64;
            let filename_len = Self::read_u16_le_static(reader).await? as usize;
            let extra_len = Self::read_u16_le_static(reader).await? as usize;
            let comment_len = Self::read_u16_le_static(reader).await? as usize;

            // Skip disk number, internal attributes, external attributes
            reader.seek(SeekFrom::Current(8)).await?;

            let mut offset = Self::read_u32_le_static(reader).await? as u64;

            // Read filename
            let mut filename_buf = vec![0u8; filename_len];
            reader.read_exact(&mut filename_buf).await?;
            let name = String::from_utf8_lossy(&filename_buf).to_string();

            // Read extra field so we can parse ZIP64 extra if present
            let mut extra_buf = vec![0u8; extra_len];
            if extra_len > 0 {
                reader.read_exact(&mut extra_buf).await?;
            }

            // If sizes/offsets are 0xFFFFFFFF, parse ZIP64 extra field (0x0001)
            let mut compressed_size = compressed_size_32;
            let mut uncompressed_size = uncompressed_size_32;

            if compressed_size_32 == 0xFFFFFFFF
                || uncompressed_size_32 == 0xFFFFFFFF
                || offset == 0xFFFFFFFF
            {
                // parse extra fields
                let mut i = 0usize;
                while i + 4 <= extra_buf.len() {
                    let id = u16::from_le_bytes([extra_buf[i], extra_buf[i + 1]]);
                    let data_len =
                        u16::from_le_bytes([extra_buf[i + 2], extra_buf[i + 3]]) as usize;
                    i += 4;
                    if i + data_len > extra_buf.len() {
                        break;
                    }
                    if id == 0x0001 {
                        // ZIP64 extra field: contains values in order: original size, compressed size, relative header offset, disk start
                        let mut cursor = 0usize;
                        // read uncompressed size if placeholder present
                        if uncompressed_size_32 == 0xFFFFFFFF && cursor + 8 <= data_len {
                            uncompressed_size = u64::from_le_bytes([
                                extra_buf[i + cursor],
                                extra_buf[i + cursor + 1],
                                extra_buf[i + cursor + 2],
                                extra_buf[i + cursor + 3],
                                extra_buf[i + cursor + 4],
                                extra_buf[i + cursor + 5],
                                extra_buf[i + cursor + 6],
                                extra_buf[i + cursor + 7],
                            ]);
                            cursor += 8;
                        }
                        // read compressed size if placeholder present
                        if compressed_size_32 == 0xFFFFFFFF && cursor + 8 <= data_len {
                            compressed_size = u64::from_le_bytes([
                                extra_buf[i + cursor],
                                extra_buf[i + cursor + 1],
                                extra_buf[i + cursor + 2],
                                extra_buf[i + cursor + 3],
                                extra_buf[i + cursor + 4],
                                extra_buf[i + cursor + 5],
                                extra_buf[i + cursor + 6],
                                extra_buf[i + cursor + 7],
                            ]);
                            cursor += 8;
                        }
                        // read offset if placeholder present
                        if offset == 0xFFFFFFFF && cursor + 8 <= data_len {
                            offset = u64::from_le_bytes([
                                extra_buf[i + cursor],
                                extra_buf[i + cursor + 1],
                                extra_buf[i + cursor + 2],
                                extra_buf[i + cursor + 3],
                                extra_buf[i + cursor + 4],
                                extra_buf[i + cursor + 5],
                                extra_buf[i + cursor + 6],
                                extra_buf[i + cursor + 7],
                            ]);
                        }
                        // we don't need disk start here
                        break;
                    }
                    i += data_len;
                }
            }

            // Skip comment
            if comment_len > 0 {
                reader.seek(SeekFrom::Current(comment_len as i64)).await?;
            }

            entries.push(ZipEntry {
                name,
                compressed_size,
                uncompressed_size,
                compression_method,
                offset,
            });
        }

        Ok(entries)
    }

    /// When EOCD indicates ZIP64 usage, find and read ZIP64 EOCD locator and record
    async fn read_zip64_eocd(
        reader: &mut BufReader<R>,
        eocd_offset: u64,
    ) -> Result<(u64, u64, u64)> {
        // Search backwards from EOCD for ZIP64 EOCD locator signature (50 4b 06 07)
        let search_start = eocd_offset.saturating_sub(65557);
        reader.seek(SeekFrom::Start(search_start)).await?;
        let mut buffer = Vec::new();
        reader.read_to_end(&mut buffer).await?;

        let mut locator_pos: Option<usize> = None;
        for i in (0..buffer.len().saturating_sub(3)).rev() {
            if buffer[i] == 0x50
                && buffer[i + 1] == 0x4b
                && buffer[i + 2] == 0x06
                && buffer[i + 3] == 0x07
            {
                locator_pos = Some(i);
                break;
            }
        }

        let locator_pos = locator_pos
            .ok_or_else(|| SZipError::InvalidFormat("ZIP64 EOCD locator not found".to_string()))?;

        // Read locator fields from buffer
        // locator layout: signature(4), number of the disk with the start of the zip64 eocd(4), relative offset of the zip64 eocd(8), total number of disks(4)
        let rel_off_bytes = &buffer[locator_pos + 8..locator_pos + 16];
        let zip64_eocd_offset = u64::from_le_bytes([
            rel_off_bytes[0],
            rel_off_bytes[1],
            rel_off_bytes[2],
            rel_off_bytes[3],
            rel_off_bytes[4],
            rel_off_bytes[5],
            rel_off_bytes[6],
            rel_off_bytes[7],
        ]);

        // Seek to ZIP64 EOCD record
        reader.seek(SeekFrom::Start(zip64_eocd_offset)).await?;

        let sig = Self::read_u32_le_static(reader).await?;
        if sig != ZIP64_END_OF_CENTRAL_DIRECTORY_SIGNATURE {
            return Err(SZipError::InvalidFormat(format!(
                "Invalid ZIP64 EOCD signature: 0x{:08x}",
                sig
            )));
        }

        // size of ZIP64 EOCD record (8 bytes)
        let _size = {
            let mut buf = [0u8; 8];
            reader.read_exact(&mut buf).await?;
            u64::from_le_bytes(buf)
        };

        // skip version made by (2), version needed (2), disk number (4), disk where central dir starts (4)
        reader.seek(SeekFrom::Current(12)).await?;

        // total number of entries on this disk (8)
        let total_entries = {
            let mut buf = [0u8; 8];
            reader.read_exact(&mut buf).await?;
            u64::from_le_bytes(buf)
        };

        // total number of entries (8) - some implementations write both; ignore the second value
        {
            let mut buf = [0u8; 8];
            reader.read_exact(&mut buf).await?;
            // ignore u64::from_le_bytes(buf)
        }

        // central directory size (8)
        let cd_size = {
            let mut buf = [0u8; 8];
            reader.read_exact(&mut buf).await?;
            u64::from_le_bytes(buf)
        };

        // central directory offset (8)
        let cd_offset = {
            let mut buf = [0u8; 8];
            reader.read_exact(&mut buf).await?;
            u64::from_le_bytes(buf)
        };

        Ok((total_entries, cd_size, cd_offset))
    }

    /// Find the end of central directory record by scanning from the end of the file
    async fn find_eocd(reader: &mut BufReader<R>) -> Result<u64> {
        let file_size = reader.seek(SeekFrom::End(0)).await?;

        // EOCD is at least 22 bytes, search last 65KB (max comment size + EOCD)
        let search_start = file_size.saturating_sub(65557);
        reader.seek(SeekFrom::Start(search_start)).await?;

        let mut buffer = Vec::new();
        reader.read_to_end(&mut buffer).await?;

        // Search for EOCD signature from the end
        for i in (0..buffer.len().saturating_sub(3)).rev() {
            if buffer[i] == 0x50
                && buffer[i + 1] == 0x4b
                && buffer[i + 2] == 0x05
                && buffer[i + 3] == 0x06
            {
                return Ok(search_start + i as u64);
            }
        }

        Err(SZipError::InvalidFormat(
            "End of central directory not found".to_string(),
        ))
    }

    async fn read_u16_le(&mut self) -> Result<u16> {
        let mut buf = [0u8; 2];
        self.reader.read_exact(&mut buf).await?;
        Ok(u16::from_le_bytes(buf))
    }

    async fn read_u32_le(&mut self) -> Result<u32> {
        let mut buf = [0u8; 4];
        self.reader.read_exact(&mut buf).await?;
        Ok(u32::from_le_bytes(buf))
    }

    async fn read_u16_le_static(reader: &mut BufReader<R>) -> Result<u16> {
        let mut buf = [0u8; 2];
        reader.read_exact(&mut buf).await?;
        Ok(u16::from_le_bytes(buf))
    }

    async fn read_u32_le_static(reader: &mut BufReader<R>) -> Result<u32> {
        let mut buf = [0u8; 4];
        reader.read_exact(&mut buf).await?;
        Ok(u32::from_le_bytes(buf))
    }
}
