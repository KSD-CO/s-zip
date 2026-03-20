//! Streaming ZIP reader - reads ZIP files without loading entire central directory
//!
//! This is a minimal ZIP reader that can extract specific files from a ZIP archive
//! without loading the entire central directory into memory.

use crate::error::{Result, SZipError};
use crate::format::{
    find_eocd_in_buffer, find_zip64_eocd_offset, parse_zip64_extra_field,
    CENTRAL_DIRECTORY_SIGNATURE, END_OF_CENTRAL_DIRECTORY_SIGNATURE, LOCAL_FILE_HEADER_SIGNATURE,
    MAX_ENTRY_ALLOC, ZIP64_END_OF_CENTRAL_DIRECTORY_SIGNATURE,
};

#[cfg(feature = "encryption")]
use crate::format::parse_aes_extra_field_buf;
use flate2::read::DeflateDecoder;
use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::Path;

#[cfg(feature = "encryption")]
use crate::encryption::{AesDecryptor, AesStrength};

// Re-export ZipEntry so existing `use s_zip::reader::ZipEntry` paths still compile.
pub use crate::format::ZipEntry;

/// Streaming ZIP archive reader with adaptive buffering
pub struct StreamingZipReader {
    file: BufReader<File>,
    entries: Vec<ZipEntry>,
    #[cfg(feature = "encryption")]
    password: Option<String>,
}

impl StreamingZipReader {
    /// Open a ZIP file and read its central directory with default buffer size
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        Self::open_with_buffer_size(path, None)
    }

    /// Open a ZIP file with custom buffer size for optimized reading
    ///
    /// Providing a buffer size hint can improve read performance:
    /// - Small ZIPs (<10MB): 32KB buffer
    /// - Medium ZIPs (<100MB): 128KB buffer  
    /// - Large ZIPs (≥100MB): 512KB buffer (default)
    ///
    /// # Example
    /// ```no_run
    /// # use s_zip::StreamingZipReader;
    /// # fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// // Optimize for large ZIP files
    /// let reader = StreamingZipReader::open_with_buffer_size(
    ///     "large_archive.zip",
    ///     Some(1024 * 1024) // 1MB buffer for very large files
    /// )?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn open_with_buffer_size<P: AsRef<Path>>(
        path: P,
        buffer_size: Option<usize>,
    ) -> Result<Self> {
        let file = File::open(path)?;

        // Use adaptive buffer size
        let buf_size = buffer_size.unwrap_or(512 * 1024); // Default 512KB
        let mut file = BufReader::with_capacity(buf_size, file);

        // Find and read central directory
        let entries = Self::read_central_directory(&mut file)?;

        Ok(StreamingZipReader {
            file,
            entries,
            #[cfg(feature = "encryption")]
            password: None,
        })
    }

    /// Set password for decrypting encrypted entries
    #[cfg(feature = "encryption")]
    pub fn set_password(&mut self, password: impl Into<String>) -> &mut Self {
        self.password = Some(password.into());
        self
    }

    /// Clear password
    #[cfg(feature = "encryption")]
    pub fn clear_password(&mut self) -> &mut Self {
        self.password = None;
        self
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
    pub fn read_entry(&mut self, entry: &ZipEntry) -> Result<Vec<u8>> {
        // Seek to local file header
        self.file.seek(SeekFrom::Start(entry.offset))?;

        // Read and verify local file header
        let signature = self.read_u32_le()?;
        if signature != LOCAL_FILE_HEADER_SIGNATURE {
            return Err(SZipError::InvalidFormat(
                "Invalid local file header signature".to_string(),
            ));
        }

        // Skip version
        self.file.seek(SeekFrom::Current(2))?;

        // Read flags to check for encryption
        let flags = self.read_u16_le()?;
        let is_encrypted = (flags & 0x01) != 0;

        // Read compression method
        let _compression_method = self.read_u16_le()?;

        // Skip modification time and date, CRC-32
        self.file.seek(SeekFrom::Current(8))?;

        // Read compressed and uncompressed sizes (already known from central directory)
        self.file.seek(SeekFrom::Current(8))?;

        // Read filename length and extra field length
        let filename_len = self.read_u16_le()? as i64;
        let extra_len = self.read_u16_le()? as usize;

        // Skip filename
        self.file.seek(SeekFrom::Current(filename_len))?;

        // Check for AES encryption in extra field
        #[cfg(feature = "encryption")]
        let encryption_info = if is_encrypted {
            self.parse_aes_extra_field(extra_len)?
        } else {
            // Skip extra field if not encrypted
            self.file.seek(SeekFrom::Current(extra_len as i64))?;
            None
        };

        #[cfg(not(feature = "encryption"))]
        {
            if is_encrypted {
                return Err(SZipError::InvalidFormat(
                    "Encrypted entry found but encryption feature not enabled".to_string(),
                ));
            }
            // Skip extra field
            self.file.seek(SeekFrom::Current(extra_len as i64))?;
        }

        // Calculate actual data size (subtract salt, password verify, and auth code for encrypted entries)
        #[cfg(feature = "encryption")]
        let data_size = if let Some((strength, _, _)) = encryption_info {
            // Subtract salt (already read), password verify (already read), and auth code (10 bytes at end)
            entry
                .compressed_size
                .saturating_sub((strength.salt_size() + 2 + 10) as u64)
        } else {
            entry.compressed_size
        };

        #[cfg(not(feature = "encryption"))]
        let data_size = entry.compressed_size;

        // Guard against OOM from corrupt/malicious compressed_size values.
        // Entries larger than 2 GiB must use read_entry_streaming() instead.
        if data_size > MAX_ENTRY_ALLOC {
            return Err(SZipError::InvalidFormat(format!(
                "Entry '{}' is too large to read into memory ({} bytes). \
                 Use read_entry_streaming() for entries larger than 2 GiB.",
                entry.name, data_size
            )));
        }

        // Now read the compressed data
        let mut compressed_data = vec![0u8; data_size as usize];
        self.file.read_exact(&mut compressed_data)?;

        // Read auth code if encrypted
        #[cfg(feature = "encryption")]
        let auth_code = if encryption_info.is_some() {
            let mut ac = vec![0u8; 10];
            self.file.read_exact(&mut ac)?;
            Some(ac)
        } else {
            None
        };

        // Decrypt if encrypted (Step 1: Decrypt compressed data)
        #[cfg(feature = "encryption")]
        let decryptor_opt = if let Some((strength, salt, pw_verify)) = encryption_info {
            let password = self.password.as_ref().ok_or_else(|| {
                SZipError::InvalidFormat("Encrypted entry but no password set".to_string())
            })?;

            // Create decryptor (password verification happens inside new())
            let mut decryptor = AesDecryptor::new(password, strength, &salt, &pw_verify)?;

            // Decrypt compressed data in-place
            decryptor.decrypt(&mut compressed_data)?;

            Some(decryptor)
        } else {
            None
        };

        // Decompress if needed (Step 2: Decompress decrypted data)
        let data = if entry.compression_method == 8 {
            // DEFLATE compression
            let mut decoder = DeflateDecoder::new(&compressed_data[..]);
            let mut decompressed = Vec::new();
            decoder.read_to_end(&mut decompressed)?;
            decompressed
        } else if entry.compression_method == 0 {
            // No compression (stored)
            compressed_data
        } else if entry.compression_method == 93 {
            // Zstd compression
            #[cfg(feature = "zstd-support")]
            {
                zstd::decode_all(&compressed_data[..])?
            }
            #[cfg(not(feature = "zstd-support"))]
            {
                return Err(SZipError::UnsupportedCompression(entry.compression_method));
            }
        } else {
            return Err(SZipError::UnsupportedCompression(entry.compression_method));
        };

        // Verify HMAC authentication (Step 3: Update HMAC with plaintext and verify)
        #[cfg(feature = "encryption")]
        if let Some(mut decryptor) = decryptor_opt {
            // Update HMAC with decompressed plaintext data
            decryptor.update_hmac(&data);

            // Verify authentication code
            if let Some(ac) = auth_code {
                decryptor.verify_auth_code(&ac)?;
            }
        }

        // Verify CRC-32 integrity — catches bit-rot and truncated downloads.
        // Skip for encrypted entries: the HMAC above already provides stronger
        // authentication guarantees.
        #[cfg(not(feature = "encryption"))]
        {
            let actual_crc = crc32fast::hash(&data);
            if entry.crc32 != 0 && actual_crc != entry.crc32 {
                return Err(SZipError::InvalidFormat(format!(
                    "CRC-32 mismatch for '{}': expected {:#010x}, got {:#010x}. \
                     The entry may be corrupt or the download may be incomplete.",
                    entry.name, entry.crc32, actual_crc
                )));
            }
        }
        #[cfg(feature = "encryption")]
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
    pub fn read_entry_by_name(&mut self, name: &str) -> Result<Vec<u8>> {
        let entry = self
            .find_entry(name)
            .ok_or_else(|| SZipError::EntryNotFound(name.to_string()))?
            .clone();

        self.read_entry(&entry)
    }

    /// Get a streaming reader for an entry by name (for large files)
    /// Returns a reader that decompresses data on-the-fly without loading everything into memory
    pub fn read_entry_streaming_by_name(&mut self, name: &str) -> Result<Box<dyn Read + '_>> {
        let entry = self
            .find_entry(name)
            .ok_or_else(|| SZipError::EntryNotFound(name.to_string()))?
            .clone();

        self.read_entry_streaming(&entry)
    }

    /// Get a streaming reader for an entry (for large files).
    ///
    /// Returns a `Read` impl that decompresses (and, if encrypted, decrypts)
    /// data on-the-fly without loading the entire entry into memory.
    ///
    /// # Encrypted entries
    ///
    /// When the `encryption` feature is enabled and the entry is encrypted,
    /// this method returns a streaming reader that decrypts on-the-fly.
    /// **Callers must read all bytes and then call `finish()` on the returned
    /// reader to verify the HMAC-SHA1 authentication tag.**  Because the
    /// decompressor sits above the decryptor in the pipeline, the HMAC is
    /// computed over the *compressed ciphertext* bytes rather than the
    /// decompressed plaintext — this is a known limitation of the streaming
    /// path.  For full WinZip AE-2 compliance (HMAC over plaintext) use
    /// `read_entry()` instead.
    ///
    /// # Errors
    /// Returns `SZipError::EncryptionError` if the entry is encrypted but
    /// `set_password()` was not called or the password is wrong.
    pub fn read_entry_streaming(&mut self, entry: &ZipEntry) -> Result<Box<dyn Read + '_>> {
        // Seek to local file header
        self.file.seek(SeekFrom::Start(entry.offset))?;

        // Read and verify local file header
        let signature = self.read_u32_le()?;
        if signature != LOCAL_FILE_HEADER_SIGNATURE {
            return Err(SZipError::InvalidFormat(
                "Invalid local file header signature".to_string(),
            ));
        }

        // Skip version, flags, compression method
        self.file.seek(SeekFrom::Current(6))?;

        // Skip modification time and date, CRC-32
        self.file.seek(SeekFrom::Current(8))?;

        // Read compressed and uncompressed sizes (use values from central directory)
        self.file.seek(SeekFrom::Current(8))?;

        // Read filename length and extra field length
        let filename_len = self.read_u16_le()? as i64;
        let extra_len = self.read_u16_le()? as usize;

        // Skip filename
        self.file.seek(SeekFrom::Current(filename_len))?;

        // For encrypted entries: parse AES extra field and decrypt on-the-fly.
        #[cfg(feature = "encryption")]
        if entry.is_encrypted {
            let encryption_info = self.parse_aes_extra_field(extra_len)?;

            if let Some((strength, salt, pw_verify)) = encryption_info {
                let password = self.password.as_ref().ok_or_else(|| {
                    SZipError::EncryptionError(
                        "Encrypted entry but no password set. Call set_password() first."
                            .to_string(),
                    )
                })?;

                // Actual ciphertext size: compressed_size minus (salt + pw_verify + auth_code)
                let overhead = (strength.salt_size() + 2 + 10) as u64;
                let cipher_size = entry.compressed_size.saturating_sub(overhead);

                // Read the 10-byte auth code positioned after ciphertext.
                // We must read it now (by seeking past the ciphertext) then
                // seek back, because limited_reader borrows self.file mutably.
                // Instead we store auth_code and pass it to DecryptingReader.
                //
                // Offset of auth_code = current_pos + cipher_size
                let current_pos = self.file.stream_position()?;
                self.file.seek(SeekFrom::Start(current_pos + cipher_size))?;
                let mut auth_code = vec![0u8; 10];
                self.file.read_exact(&mut auth_code)?;

                // Seek back to start of ciphertext
                self.file.seek(SeekFrom::Start(current_pos))?;

                let limited_reader = (&mut self.file).take(cipher_size);

                use crate::decrypt_reader::sync::DecryptingReader;
                let decrypt_reader = DecryptingReader::new(
                    limited_reader,
                    password,
                    strength,
                    &salt,
                    &pw_verify,
                    auth_code,
                )?;

                // Wrap with decompressor
                return if entry.compression_method == 8 {
                    Ok(Box::new(DeflateDecoder::new(decrypt_reader)))
                } else if entry.compression_method == 0 {
                    Ok(Box::new(decrypt_reader))
                } else {
                    Err(SZipError::UnsupportedCompression(entry.compression_method))
                };
            } else {
                // Extra field didn't contain AES info — skip it
                self.file.seek(SeekFrom::Current(extra_len as i64))?;
            }
        }

        // Non-encrypted path: skip extra field
        #[cfg(feature = "encryption")]
        if !entry.is_encrypted {
            self.file.seek(SeekFrom::Current(extra_len as i64))?;
        }

        #[cfg(not(feature = "encryption"))]
        self.file.seek(SeekFrom::Current(extra_len as i64))?;

        // Create a reader limited to compressed data size
        let limited_reader = (&mut self.file).take(entry.compressed_size);

        // Wrap with decompressor if needed
        if entry.compression_method == 8 {
            // DEFLATE compression
            Ok(Box::new(DeflateDecoder::new(limited_reader)))
        } else if entry.compression_method == 0 {
            // No compression (stored)
            Ok(Box::new(limited_reader))
        } else if entry.compression_method == 93 {
            // Zstd compression
            #[cfg(feature = "zstd-support")]
            {
                Ok(Box::new(zstd::Decoder::new(limited_reader)?))
            }
            #[cfg(not(feature = "zstd-support"))]
            {
                Err(SZipError::UnsupportedCompression(entry.compression_method))
            }
        } else {
            Err(SZipError::UnsupportedCompression(entry.compression_method))
        }
    }

    /// Get a streaming reader for an entry by name
    pub fn read_entry_by_name_streaming(&mut self, name: &str) -> Result<Box<dyn Read + '_>> {
        let entry = self
            .find_entry(name)
            .ok_or_else(|| SZipError::EntryNotFound(name.to_string()))?
            .clone();

        self.read_entry_streaming(&entry)
    }

    /// Read the central directory from the ZIP file
    fn read_central_directory(file: &mut BufReader<File>) -> Result<Vec<ZipEntry>> {
        // Find end of central directory record
        let eocd_offset = Self::find_eocd(file)?;

        // Seek to EOCD
        file.seek(SeekFrom::Start(eocd_offset))?;

        // Read EOCD
        let signature = Self::read_u32_le_static(file)?;
        if signature != END_OF_CENTRAL_DIRECTORY_SIGNATURE {
            return Err(SZipError::InvalidFormat(format!(
                "Invalid end of central directory signature: 0x{:08x}",
                signature
            )));
        }

        // Skip disk number fields (4 bytes)
        file.seek(SeekFrom::Current(4))?;

        // Read number of entries on this disk (2 bytes)
        let _entries_on_disk = Self::read_u16_le_static(file)?;

        // Read total number of entries (2 bytes)

        // These values may be placeholder 0xFFFF/0xFFFFFFFF when ZIP64 is used
        let total_entries_16 = Self::read_u16_le_static(file)?;

        // Read central directory size (4 bytes)
        let cd_size_32 = Self::read_u32_le_static(file)?;

        // Read central directory offset (4 bytes)
        let cd_offset_32 = Self::read_u32_le_static(file)? as u64;

        // Promote to u64 and handle ZIP64 if markers present
        let mut total_entries = total_entries_16 as usize;
        let mut cd_offset = cd_offset_32;
        let _cd_size = cd_size_32 as u64;

        if total_entries_16 == 0xFFFF || cd_size_32 == 0xFFFFFFFF || cd_offset_32 == 0xFFFFFFFF {
            // Need to find ZIP64 EOCD locator and read ZIP64 EOCD record
            let (zip64_total_entries, zip64_cd_size, zip64_cd_offset) =
                Self::read_zip64_eocd(file, eocd_offset)?;
            total_entries = zip64_total_entries as usize;
            cd_offset = zip64_cd_offset;
            // _cd_size can be used if needed (zip64_cd_size)
            let _ = zip64_cd_size;
        }

        // Seek to central directory
        file.seek(SeekFrom::Start(cd_offset))?;

        // Read all central directory entries
        let mut entries = Vec::with_capacity(total_entries);
        for _ in 0..total_entries {
            let signature = Self::read_u32_le_static(file)?;
            if signature != CENTRAL_DIRECTORY_SIGNATURE {
                break;
            }

            // Skip version made by, version needed
            file.seek(SeekFrom::Current(4))?;

            // Read flags (needed for encryption check)
            #[cfg_attr(not(feature = "encryption"), allow(unused_variables))]
            let flags = Self::read_u16_le_static(file)?;

            let compression_method = Self::read_u16_le_static(file)?;

            // Read modification time, date, and CRC-32
            file.seek(SeekFrom::Current(4))?; // mod time + date
            let crc32 = Self::read_u32_le_static(file)?;

            // Read sizes as 32-bit placeholders (may be 0xFFFFFFFF meaning ZIP64)
            let compressed_size_32 = Self::read_u32_le_static(file)? as u64;
            let uncompressed_size_32 = Self::read_u32_le_static(file)? as u64;
            let filename_len = Self::read_u16_le_static(file)? as usize;
            let extra_len = Self::read_u16_le_static(file)? as usize;
            let comment_len = Self::read_u16_le_static(file)? as usize;

            // Skip disk number, internal attributes, external attributes
            file.seek(SeekFrom::Current(8))?;

            let offset_32 = Self::read_u32_le_static(file)? as u64;

            // Read filename
            let mut filename_buf = vec![0u8; filename_len];
            file.read_exact(&mut filename_buf)?;
            let name = String::from_utf8_lossy(&filename_buf).to_string();

            // Read extra field so we can parse ZIP64 extra if present
            let mut extra_buf = vec![0u8; extra_len];
            if extra_len > 0 {
                file.read_exact(&mut extra_buf)?;
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
                file.seek(SeekFrom::Current(comment_len as i64))?;
            }

            entries.push(ZipEntry {
                name,
                compressed_size,
                uncompressed_size,
                compression_method,
                offset,
                crc32,
                is_encrypted: (flags & 0x01) != 0,
            });
        }

        Ok(entries)
    }

    /// When EOCD indicates ZIP64 usage, find and read ZIP64 EOCD locator and record
    fn read_zip64_eocd(file: &mut BufReader<File>, eocd_offset: u64) -> Result<(u64, u64, u64)> {
        // Search backwards from EOCD for ZIP64 EOCD locator signature (50 4b 06 07)
        let search_start = eocd_offset.saturating_sub(65557);
        file.seek(SeekFrom::Start(search_start))?;
        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer)?;

        let zip64_eocd_offset = find_zip64_eocd_offset(&buffer)
            .ok_or_else(|| SZipError::InvalidFormat("ZIP64 EOCD locator not found".to_string()))?;

        // Seek to ZIP64 EOCD record
        file.seek(SeekFrom::Start(zip64_eocd_offset))?;

        let sig = Self::read_u32_le_static(file)?;
        if sig != ZIP64_END_OF_CENTRAL_DIRECTORY_SIGNATURE {
            return Err(SZipError::InvalidFormat(format!(
                "Invalid ZIP64 EOCD signature: 0x{:08x}",
                sig
            )));
        }

        // size of ZIP64 EOCD record (8 bytes)
        let _size = {
            let mut buf = [0u8; 8];
            file.read_exact(&mut buf)?;
            u64::from_le_bytes(buf)
        };

        // skip version made by (2), version needed (2), disk number (4), disk where central dir starts (4)
        file.seek(SeekFrom::Current(12))?;

        // total number of entries on this disk (8)
        let total_entries = {
            let mut buf = [0u8; 8];
            file.read_exact(&mut buf)?;
            u64::from_le_bytes(buf)
        };

        // total number of entries (8) - some implementations write both; ignore the second value
        {
            let mut buf = [0u8; 8];
            file.read_exact(&mut buf)?;
        }

        // central directory size (8)
        let cd_size = {
            let mut buf = [0u8; 8];
            file.read_exact(&mut buf)?;
            u64::from_le_bytes(buf)
        };

        // central directory offset (8)
        let cd_offset = {
            let mut buf = [0u8; 8];
            file.read_exact(&mut buf)?;
            u64::from_le_bytes(buf)
        };

        Ok((total_entries, cd_size, cd_offset))
    }

    /// Find the end of central directory record by scanning from the end of the file
    fn find_eocd(file: &mut BufReader<File>) -> Result<u64> {
        let file_size = file.seek(SeekFrom::End(0))?;

        // EOCD is at least 22 bytes, search last 65KB (max comment size + EOCD)
        let search_start = file_size.saturating_sub(65557);
        file.seek(SeekFrom::Start(search_start))?;

        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer)?;

        find_eocd_in_buffer(&buffer, search_start).ok_or_else(|| {
            SZipError::InvalidFormat("End of central directory not found".to_string())
        })
    }

    fn read_u16_le(&mut self) -> Result<u16> {
        let mut buf = [0u8; 2];
        self.file.read_exact(&mut buf)?;
        Ok(u16::from_le_bytes(buf))
    }

    fn read_u32_le(&mut self) -> Result<u32> {
        let mut buf = [0u8; 4];
        self.file.read_exact(&mut buf)?;
        Ok(u32::from_le_bytes(buf))
    }

    fn read_u16_le_static(file: &mut BufReader<File>) -> Result<u16> {
        let mut buf = [0u8; 2];
        file.read_exact(&mut buf)?;
        Ok(u16::from_le_bytes(buf))
    }

    fn read_u32_le_static(file: &mut BufReader<File>) -> Result<u32> {
        let mut buf = [0u8; 4];
        file.read_exact(&mut buf)?;
        Ok(u32::from_le_bytes(buf))
    }

    /// Parse AES encryption info from extra field
    #[cfg(feature = "encryption")]
    #[allow(clippy::type_complexity)]
    fn parse_aes_extra_field(
        &mut self,
        extra_len: usize,
    ) -> Result<Option<(AesStrength, Vec<u8>, [u8; 2])>> {
        if extra_len == 0 {
            return Ok(None);
        }

        let mut extra_buf = vec![0u8; extra_len];
        self.file.read_exact(&mut extra_buf)?;

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
                    "Unsupported AES strength: {}",
                    strength_code
                )))
            }
        };

        // Read salt and password verification from actual file data (not extra field)
        // Salt comes after the extra field, before compressed data
        let salt_size = strength.salt_size();

        let mut salt = vec![0u8; salt_size];
        self.file.read_exact(&mut salt)?;

        let mut pw_verify = [0u8; 2];
        self.file.read_exact(&mut pw_verify)?;

        Ok(Some((strength, salt, pw_verify)))
    }
}
