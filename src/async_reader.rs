//! Generic async ZIP reader for arbitrary async sources (files, HTTP, S3, in-memory, etc.)
//!
//! This module provides a generic async ZIP reader that works with any source
//! implementing AsyncRead + AsyncSeek + Unpin + Send.

use crate::error::{Result, SZipError};
use crate::format::{
    find_eocd_in_buffer, find_zip64_eocd_offset, parse_aes_extra_field_buf,
    parse_zip64_extra_field, CENTRAL_DIRECTORY_SIGNATURE, END_OF_CENTRAL_DIRECTORY_SIGNATURE,
    LOCAL_FILE_HEADER_SIGNATURE, MAX_ENTRY_ALLOC, ZIP64_END_OF_CENTRAL_DIRECTORY_SIGNATURE,
};
use async_compression::tokio::bufread::DeflateDecoder;
#[cfg(feature = "async-zstd")]
use async_compression::tokio::bufread::ZstdDecoder;
use std::io::SeekFrom;
use std::path::Path;
use tokio::fs::File;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncSeek, AsyncSeekExt, BufReader};

// Re-export ZipEntry so existing `use s_zip::async_reader::ZipEntry` paths still compile.
pub use crate::format::ZipEntry;

/// Generic async streaming ZIP reader that works with any async reader + seeker
///
/// Supports adaptive buffering for optimized read performance based on file size.
pub struct GenericAsyncZipReader<R: AsyncRead + AsyncSeek + Unpin + Send> {
    reader: BufReader<R>,
    entries: Vec<ZipEntry>,
    #[cfg(feature = "encryption")]
    password: Option<String>,
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

    /// Read multiple entries concurrently, each in its own file handle.
    ///
    /// Opens a new `File` handle per task (bounded by `max_concurrent`) so
    /// that entries are decompressed in parallel without blocking each other.
    /// Useful for extracting many entries from a large archive.
    ///
    /// # Arguments
    /// * `path` — path to the ZIP file (re-opened per task)
    /// * `names` — entry names to extract; missing entries are silently skipped
    /// * `max_concurrent` — maximum parallel tasks (defaults to 4)
    ///
    /// # Returns
    /// Vec of `(name, data)` pairs in the same order as `names` (skipping
    /// any entries not found).
    ///
    /// # Example
    /// ```no_run
    /// # use s_zip::AsyncStreamingZipReader;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let results = AsyncStreamingZipReader::read_entries_parallel(
    ///     "archive.zip",
    ///     vec!["a.txt".to_string(), "b.txt".to_string(), "c.txt".to_string()],
    ///     Some(4),
    /// ).await?;
    /// for (name, data) in &results {
    ///     println!("{}: {} bytes", name, data.len());
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn read_entries_parallel(
        path: impl AsRef<std::path::Path>,
        names: Vec<String>,
        max_concurrent: Option<usize>,
    ) -> Result<Vec<(String, Vec<u8>)>> {
        use std::sync::Arc;
        use tokio::sync::{Semaphore, mpsc};

        let path = Arc::new(path.as_ref().to_path_buf());
        let concurrency = max_concurrent.unwrap_or(4).max(1);
        let sem = Arc::new(Semaphore::new(concurrency));

        // First open the file once to read the central directory
        let index = AsyncStreamingZipReader::open(path.as_ref()).await?;
        let entry_map: std::collections::HashMap<String, crate::format::ZipEntry> = index
            .entries()
            .iter()
            .map(|e| (e.name.clone(), e.clone()))
            .collect();

        // Filter to only names that exist
        let targets: Vec<(String, crate::format::ZipEntry)> = names
            .into_iter()
            .filter_map(|n| entry_map.get(&n).map(|e| (n.clone(), e.clone())))
            .collect();

        if targets.is_empty() {
            return Ok(Vec::new());
        }

        let (tx, mut rx) = mpsc::channel::<Result<(String, Vec<u8>)>>(targets.len());

        for (name, entry) in targets {
            let path = Arc::clone(&path);
            let sem = Arc::clone(&sem);
            let tx = tx.clone();

            tokio::spawn(async move {
                let _permit = sem.acquire().await.expect("semaphore closed");
                let result = async {
                    let mut reader = AsyncStreamingZipReader::open(path.as_ref()).await?;
                    let data = reader.read_entry(&entry).await?;
                    Ok((name, data))
                }
                .await;
                let _ = tx.send(result).await;
            });
        }
        drop(tx); // close sender so receiver can drain

        let mut results = Vec::new();
        while let Some(r) = rx.recv().await {
            results.push(r?);
        }
        Ok(results)
    }
}

impl<R: AsyncRead + AsyncSeek + Unpin + Send> GenericAsyncZipReader<R> {
    /// Create a new generic async ZIP reader with custom buffer size
    ///
    /// Allows fine-tuning read performance based on expected data patterns.
    pub async fn new_with_buffer_size(reader: R, buffer_size: Option<usize>) -> Result<Self> {
        // Use adaptive buffer size
        let buf_size = buffer_size.unwrap_or(1024 * 1024); // Default 1MB for async
        let mut reader = BufReader::with_capacity(buf_size, reader);

        // Find and read central directory
        let entries = Self::read_central_directory(&mut reader).await?;

        Ok(GenericAsyncZipReader {
            reader,
            entries,
            #[cfg(feature = "encryption")]
            password: None,
        })
    }

    /// Get list of all entries in the ZIP
    pub fn entries(&self) -> &[ZipEntry] {
        &self.entries
    }

    /// Find an entry by name
    pub fn find_entry(&self, name: &str) -> Option<&ZipEntry> {
        self.entries.iter().find(|e| e.name == name)
    }

    /// Set the password for decrypting AES-256 encrypted entries.
    ///
    /// Call this before `read_entry()` when the ZIP contains encrypted entries.
    /// Requires the `encryption` feature.
    ///
    /// ```no_run
    /// # use s_zip::AsyncStreamingZipReader;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let mut reader = AsyncStreamingZipReader::open("secure.zip").await?;
    /// reader.set_password("my_password");
    /// let data = reader.read_entry_by_name("secret.txt").await?;
    /// # Ok(())
    /// # }
    /// ```
    #[cfg(feature = "encryption")]
    pub fn set_password(&mut self, password: impl Into<String>) {
        self.password = Some(password.into());
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

        // Skip version (2 bytes)
        self.reader.seek(SeekFrom::Current(2)).await?;

        // Read flags to check for encryption
        let flags = self.read_u16_le().await?;
        let is_encrypted_local = (flags & 0x01) != 0;

        // Read compression method (skip — use value from central directory)
        self.reader.seek(SeekFrom::Current(2)).await?;

        // Skip modification time and date, CRC-32
        self.reader.seek(SeekFrom::Current(8)).await?;

        // Skip compressed and uncompressed sizes (use from central directory)
        self.reader.seek(SeekFrom::Current(8)).await?;

        // Read filename length and extra field length
        let filename_len = self.read_u16_le().await? as i64;
        let extra_len = self.read_u16_le().await? as usize;

        // Skip filename
        self.reader.seek(SeekFrom::Current(filename_len)).await?;

        // Parse AES extra field if encrypted
        #[cfg(feature = "encryption")]
        let encryption_info = if is_encrypted_local {
            self.parse_aes_extra_field(extra_len).await?
        } else {
            self.reader
                .seek(SeekFrom::Current(extra_len as i64))
                .await?;
            None
        };

        #[cfg(not(feature = "encryption"))]
        {
            if is_encrypted_local {
                return Err(SZipError::InvalidFormat(
                    "Encrypted entry found but encryption feature not enabled".to_string(),
                ));
            }
            self.reader
                .seek(SeekFrom::Current(extra_len as i64))
                .await?;
        }

        // Guard against OOM from corrupt/malicious compressed_size values.
        // For encrypted entries, the actual data size is smaller (salt + pw_verify already consumed).
        #[cfg(feature = "encryption")]
        let data_size = if let Some((strength, _, _)) = encryption_info {
            entry
                .compressed_size
                .saturating_sub((strength.salt_size() + 2 + 10) as u64)
        } else {
            entry.compressed_size
        };
        #[cfg(not(feature = "encryption"))]
        let data_size = entry.compressed_size;

        if data_size > MAX_ENTRY_ALLOC {
            return Err(SZipError::InvalidFormat(format!(
                "Entry '{}' is too large to read into memory ({} bytes). \
                 Use read_entry_streaming() for entries larger than 2 GiB.",
                entry.name, data_size
            )));
        }

        // Read compressed (and possibly encrypted) data
        let mut compressed_data = vec![0u8; data_size as usize];
        self.reader.read_exact(&mut compressed_data).await?;

        // Read auth code if encrypted (10 bytes HMAC-SHA1 truncated)
        #[cfg(feature = "encryption")]
        let auth_code = if encryption_info.is_some() {
            let mut ac = vec![0u8; 10];
            self.reader.read_exact(&mut ac).await?;
            Some(ac)
        } else {
            None
        };

        // Decrypt compressed data in-place (Step 1)
        #[cfg(feature = "encryption")]
        let decryptor_opt = if let Some((strength, salt, pw_verify)) = encryption_info {
            use crate::encryption::AesDecryptor;
            let password = self.password.as_ref().ok_or_else(|| {
                SZipError::EncryptionError(
                    "Encrypted entry but no password set. Call set_password() first.".to_string(),
                )
            })?;
            let mut decryptor = AesDecryptor::new(password, strength, &salt, &pw_verify)?;
            decryptor.decrypt(&mut compressed_data)?;
            Some(decryptor)
        } else {
            None
        };

        // Decompress if needed (Step 2)
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

        // Verify HMAC authentication for encrypted entries (Step 3)
        #[cfg(feature = "encryption")]
        if let Some(mut decryptor) = decryptor_opt {
            decryptor.update_hmac(&data);
            if let Some(ac) = auth_code {
                decryptor.verify_auth_code(&ac)?;
            }
        }

        // Verify CRC-32 integrity — catches bit-rot and truncated downloads.
        // Skip for encrypted entries (HMAC provides stronger authentication).
        if entry.crc32 != 0 && !entry.is_encrypted {
            let actual_crc = crc32fast::hash(&data);
            if actual_crc != entry.crc32 {
                return Err(SZipError::InvalidFormat(format!(
                    "CRC-32 mismatch for '{}': expected {:#010x}, got {:#010x}. \
                     The entry may be corrupt or the download may be incomplete.",
                    entry.name, entry.crc32, actual_crc
                )));
            }
        }

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

    /// Get a streaming reader for an entry (for large files).
    ///
    /// Returns an `AsyncRead` impl that decompresses (and, if encrypted, decrypts)
    /// data on-the-fly without loading the entire entry into memory.
    ///
    /// # Encrypted entries
    ///
    /// When both `encryption` and `async` features are enabled and the entry is
    /// encrypted, decryption happens on-the-fly.  **Callers must read all bytes
    /// and then call `finish()` on the returned reader to verify the HMAC-SHA1
    /// authentication tag.**  The HMAC covers the *compressed ciphertext* bytes,
    /// not the decompressed plaintext (a known limitation of the streaming path).
    /// For full WinZip AE-2 compliance use `read_entry()` instead.
    ///
    /// # Errors
    /// Returns `SZipError::EncryptionError` if the entry is encrypted but
    /// `set_password()` was not called or the password is wrong.
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

        // Read compressed and uncompressed sizes (use from central directory)
        self.reader.seek(SeekFrom::Current(8)).await?;

        // Read filename length and extra field length
        let filename_len = self.read_u16_le().await? as i64;
        let extra_len = self.read_u16_le().await? as usize;

        // Skip filename
        self.reader.seek(SeekFrom::Current(filename_len)).await?;

        // For encrypted entries: decrypt on-the-fly.
        #[cfg(feature = "encryption")]
        if entry.is_encrypted {
            let encryption_info = self.parse_aes_extra_field(extra_len).await?;

            if let Some((strength, salt, pw_verify)) = encryption_info {
                let password = self.password.as_ref().ok_or_else(|| {
                    SZipError::EncryptionError(
                        "Encrypted entry but no password set. Call set_password() first."
                            .to_string(),
                    )
                })?;

                let overhead = (strength.salt_size() + 2 + 10) as u64;
                let cipher_size = entry.compressed_size.saturating_sub(overhead);

                // Read auth code (seek past ciphertext, read 10 bytes, seek back)
                let current_pos = self.reader.stream_position().await?;
                self.reader
                    .seek(SeekFrom::Start(current_pos + cipher_size))
                    .await?;
                let mut auth_code = vec![0u8; 10];
                self.reader.read_exact(&mut auth_code).await?;
                self.reader.seek(SeekFrom::Start(current_pos)).await?;

                let limited_reader = (&mut self.reader).take(cipher_size);

                use crate::decrypt_reader::r#async::AsyncDecryptingReader;
                let decrypt_reader = AsyncDecryptingReader::new(
                    limited_reader,
                    password,
                    strength,
                    &salt,
                    &pw_verify,
                    auth_code,
                )?;

                return if entry.compression_method == 8 {
                    Ok(Box::new(DeflateDecoder::new(BufReader::new(
                        decrypt_reader,
                    ))))
                } else if entry.compression_method == 0 {
                    Ok(Box::new(decrypt_reader))
                } else {
                    Err(SZipError::UnsupportedCompression(entry.compression_method))
                };
            } else {
                self.reader
                    .seek(SeekFrom::Current(extra_len as i64))
                    .await?;
            }
        }

        // Non-encrypted path: skip extra field
        #[cfg(feature = "encryption")]
        if !entry.is_encrypted {
            self.reader
                .seek(SeekFrom::Current(extra_len as i64))
                .await?;
        }

        #[cfg(not(feature = "encryption"))]
        self.reader
            .seek(SeekFrom::Current(extra_len as i64))
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

            // Skip version made by (2), version needed (2); read flags (2)
            reader.seek(SeekFrom::Current(4)).await?;
            let flags = Self::read_u16_le_static(reader).await?;
            let is_encrypted = (flags & 0x01) != 0;

            let compression_method = Self::read_u16_le_static(reader).await?;

            // Read modification time, date, and CRC-32
            reader.seek(SeekFrom::Current(4)).await?; // mod time + date
            let crc32 = Self::read_u32_le_static(reader).await?;

            // Read sizes as 32-bit placeholders (may be 0xFFFFFFFF meaning ZIP64)
            let compressed_size_32 = Self::read_u32_le_static(reader).await? as u64;
            let uncompressed_size_32 = Self::read_u32_le_static(reader).await? as u64;
            let filename_len = Self::read_u16_le_static(reader).await? as usize;
            let extra_len = Self::read_u16_le_static(reader).await? as usize;
            let comment_len = Self::read_u16_le_static(reader).await? as usize;

            // Skip disk number, internal attributes, external attributes
            reader.seek(SeekFrom::Current(8)).await?;

            let offset_32 = Self::read_u32_le_static(reader).await? as u64;

            // Read filename
            let mut filename_buf = vec![0u8; filename_len];
            reader.read_exact(&mut filename_buf).await?;
            let name = String::from_utf8_lossy(&filename_buf).to_string();

            // Read extra field so we can parse ZIP64 extra if present
            let mut extra_buf = vec![0u8; extra_len];
            if extra_len > 0 {
                reader.read_exact(&mut extra_buf).await?;
            }

            // Resolve ZIP64 placeholders using shared pure helper
            let (uncompressed_size, compressed_size, offset) = if compressed_size_32 == 0xFFFFFFFF
                || uncompressed_size_32 == 0xFFFFFFFF
                || offset_32 == 0xFFFFFFFF
            {
                parse_zip64_extra_field(
                    &extra_buf,
                    compressed_size_32,
                    uncompressed_size_32,
                    offset_32,
                )
            } else {
                (uncompressed_size_32, compressed_size_32, offset_32)
            };

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
                crc32,
                is_encrypted,
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

        let zip64_eocd_offset = find_zip64_eocd_offset(&buffer)
            .ok_or_else(|| SZipError::InvalidFormat("ZIP64 EOCD locator not found".to_string()))?;

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

        find_eocd_in_buffer(&buffer, search_start).ok_or_else(|| {
            SZipError::InvalidFormat("End of central directory not found".to_string())
        })
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

    /// Parse the AES extra field (ID 0x9901) from the local file header extra data,
    /// then read the salt and password-verification bytes that follow the extra field.
    ///
    /// Returns `Some((strength, salt, pw_verify))` if AES encryption is present,
    /// or `None` if no AES extra field is found (extra field is consumed either way).
    #[cfg(feature = "encryption")]
    async fn parse_aes_extra_field(
        &mut self,
        extra_len: usize,
    ) -> Result<Option<(crate::encryption::AesStrength, Vec<u8>, [u8; 2])>> {
        use crate::encryption::AesStrength;

        if extra_len == 0 {
            return Ok(None);
        }

        let mut extra_buf = vec![0u8; extra_len];
        self.reader.read_exact(&mut extra_buf).await?;

        // Use shared pure helper to find the strength code
        let strength_code = match parse_aes_extra_field_buf(&extra_buf) {
            Some(code) => code,
            None => return Ok(None),
        };

        let strength = match strength_code {
            0x01 => AesStrength::Aes128,
            0x02 => AesStrength::Aes192,
            0x03 => AesStrength::Aes256,
            _ => {
                return Err(SZipError::InvalidFormat(format!(
                    "Unsupported AES strength code: {}",
                    strength_code
                )))
            }
        };

        // Salt and password-verify bytes follow the extra field in the file data
        let salt_size = strength.salt_size();
        let mut salt = vec![0u8; salt_size];
        self.reader.read_exact(&mut salt).await?;

        let mut pw_verify = [0u8; 2];
        self.reader.read_exact(&mut pw_verify).await?;

        Ok(Some((strength, salt, pw_verify)))
    }
}
