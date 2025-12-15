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
        let total_entries = Self::read_u16_le_static(file)? as usize;

        // Read central directory size (4 bytes)
        let _cd_size = Self::read_u32_le_static(file)?;

        // Read central directory offset (4 bytes)
        let cd_offset = Self::read_u32_le_static(file)? as u64;

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

            let compressed_size = Self::read_u32_le_static(file)? as u64;
            let uncompressed_size = Self::read_u32_le_static(file)? as u64;
            let filename_len = Self::read_u16_le_static(file)? as usize;
            let extra_len = Self::read_u16_le_static(file)? as usize;
            let comment_len = Self::read_u16_le_static(file)? as usize;

            // Skip disk number, internal attributes, external attributes
            file.seek(SeekFrom::Current(8))?;

            let offset = Self::read_u32_le_static(file)? as u64;

            // Read filename
            let mut filename_buf = vec![0u8; filename_len];
            file.read_exact(&mut filename_buf)?;
            let name = String::from_utf8_lossy(&filename_buf).to_string();

            // Skip extra field and comment
            file.seek(SeekFrom::Current((extra_len + comment_len) as i64))?;

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
