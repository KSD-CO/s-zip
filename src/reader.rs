//! Streaming ZIP reader - reads ZIP files without loading entire central directory
//!
//! This is a minimal ZIP reader that can extract specific files from a ZIP archive
//! without loading the entire central directory into memory.

use crate::error::{Result, SZipError};
use flate2::read::DeflateDecoder;
use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::Path;

/// ZIP local file header signature
const LOCAL_FILE_HEADER_SIGNATURE: u32 = 0x04034b50;

/// ZIP central directory signature
const CENTRAL_DIRECTORY_SIGNATURE: u32 = 0x02014b50;

/// ZIP end of central directory signature
const END_OF_CENTRAL_DIRECTORY_SIGNATURE: u32 = 0x06054b50;

/// ZIP64 end of central directory record signature
const ZIP64_END_OF_CENTRAL_DIRECTORY_SIGNATURE: u32 = 0x06064b50;

// ZIP64 end of central directory locator signature (not used as a u32 constant)

/// Entry in the ZIP central directory
#[derive(Debug, Clone)]
pub struct ZipEntry {
    pub name: String,
    pub compressed_size: u64,
    pub uncompressed_size: u64,
    pub compression_method: u16,
    pub offset: u64,
}

/// Streaming ZIP archive reader
pub struct StreamingZipReader {
    file: BufReader<File>,
    entries: Vec<ZipEntry>,
}

impl StreamingZipReader {
    /// Open a ZIP file and read its central directory
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let mut file = BufReader::new(File::open(path)?);

        // Find and read central directory
        let entries = Self::read_central_directory(&mut file)?;

        Ok(StreamingZipReader { file, entries })
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

        // Skip version, flags, compression method
        self.file.seek(SeekFrom::Current(6))?;

        // Skip modification time and date, CRC-32
        self.file.seek(SeekFrom::Current(8))?;

        // Read compressed and uncompressed sizes (already known from central directory)
        self.file.seek(SeekFrom::Current(8))?;

        // Read filename length and extra field length
        let filename_len = self.read_u16_le()? as i64;
        let extra_len = self.read_u16_le()? as i64;

        // Skip filename and extra field
        self.file
            .seek(SeekFrom::Current(filename_len + extra_len))?;

        // Now read the compressed data
        let mut compressed_data = vec![0u8; entry.compressed_size as usize];
        self.file.read_exact(&mut compressed_data)?;

        // Decompress if needed
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

    /// Get a streaming reader for an entry (for large files)
    /// Returns a reader that decompresses data on-the-fly without loading everything into memory
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

        // Read compressed and uncompressed sizes
        self.file.seek(SeekFrom::Current(8))?;

        // Read filename length and extra field length
        let filename_len = self.read_u16_le()? as i64;
        let extra_len = self.read_u16_le()? as i64;

        // Skip filename and extra field
        self.file
            .seek(SeekFrom::Current(filename_len + extra_len))?;

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

            // Skip version made by, version needed, flags
            file.seek(SeekFrom::Current(6))?;

            let compression_method = Self::read_u16_le_static(file)?;

            // Skip modification time, date, CRC-32
            file.seek(SeekFrom::Current(8))?;

            // Read sizes as 32-bit placeholders (may be 0xFFFFFFFF meaning ZIP64)
            let compressed_size_32 = Self::read_u32_le_static(file)? as u64;
            let uncompressed_size_32 = Self::read_u32_le_static(file)? as u64;
            let filename_len = Self::read_u16_le_static(file)? as usize;
            let extra_len = Self::read_u16_le_static(file)? as usize;
            let comment_len = Self::read_u16_le_static(file)? as usize;

            // Skip disk number, internal attributes, external attributes
            file.seek(SeekFrom::Current(8))?;

            let mut offset = Self::read_u32_le_static(file)? as u64;

            // Read filename
            let mut filename_buf = vec![0u8; filename_len];
            file.read_exact(&mut filename_buf)?;
            let name = String::from_utf8_lossy(&filename_buf).to_string();

            // Read extra field so we can parse ZIP64 extra if present
            let mut extra_buf = vec![0u8; extra_len];
            if extra_len > 0 {
                file.read_exact(&mut extra_buf)?;
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
                file.seek(SeekFrom::Current(comment_len as i64))?;
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
    fn read_zip64_eocd(file: &mut BufReader<File>, eocd_offset: u64) -> Result<(u64, u64, u64)> {
        // Search backwards from EOCD for ZIP64 EOCD locator signature (50 4b 06 07)
        let search_start = eocd_offset.saturating_sub(65557);
        file.seek(SeekFrom::Start(search_start))?;
        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer)?;

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
            // ignore u64::from_le_bytes(buf)
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
}
