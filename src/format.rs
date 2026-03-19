//! Shared ZIP format constants, types, and pure parsing helpers.
//!
//! Both `reader` and `async_reader` import from here to avoid duplicating ~600
//! lines of identical ZIP parsing logic.  None of the functions in this module
//! perform I/O — they only operate on already-read byte slices so they can be
//! used in both the sync and async code paths without any adaptation.

use std::path::{Component, Path, PathBuf};

// ── Signatures ────────────────────────────────────────────────────────────────

/// ZIP local file header signature (`PK\x03\x04`)
pub const LOCAL_FILE_HEADER_SIGNATURE: u32 = 0x04034b50;

/// ZIP central directory entry signature (`PK\x01\x02`)
pub const CENTRAL_DIRECTORY_SIGNATURE: u32 = 0x02014b50;

/// ZIP end-of-central-directory signature (`PK\x05\x06`)
pub const END_OF_CENTRAL_DIRECTORY_SIGNATURE: u32 = 0x06054b50;

/// ZIP64 end-of-central-directory record signature (`PK\x06\x06`)
pub const ZIP64_END_OF_CENTRAL_DIRECTORY_SIGNATURE: u32 = 0x06064b50;

// ── Limits ────────────────────────────────────────────────────────────────────

/// Maximum single-entry allocation (2 GiB).
///
/// Prevents OOM when reading a corrupt or maliciously crafted ZIP that
/// advertises a huge `compressed_size` (e.g. `u64::MAX`) in its central
/// directory.  Entries genuinely larger than this threshold must use the
/// streaming API (`read_entry_streaming`).
pub const MAX_ENTRY_ALLOC: u64 = 2 * 1024 * 1024 * 1024; // 2 GiB

// ── Entry ─────────────────────────────────────────────────────────────────────

/// Entry in a ZIP central directory.
///
/// Shared between the sync (`reader`) and async (`async_reader`) modules.
#[derive(Debug, Clone)]
pub struct ZipEntry {
    pub name: String,
    pub compressed_size: u64,
    pub uncompressed_size: u64,
    pub compression_method: u16,
    /// Offset of the local file header from the start of the archive.
    pub offset: u64,
    /// CRC-32 checksum from the central directory.
    pub crc32: u32,
    /// `true` when general-purpose bit 0 (encryption flag) is set in the
    /// central directory.  The sync reader gates this behind
    /// `#[cfg(feature = "encryption")]` at the usage sites; the async reader
    /// always exposes it.
    pub is_encrypted: bool,
}

impl ZipEntry {
    /// Return a sanitized extraction path safe against zip-slip attacks.
    ///
    /// Strips leading `/`, `\`, `..`, and Windows drive prefixes.
    ///
    /// Always use this method when extracting entries to disk.  Never use
    /// `entry.name` directly as a filesystem path.
    ///
    /// # Example
    /// ```no_run
    /// # use s_zip::ZipEntry;
    /// // A malicious entry "../../../etc/passwd" becomes "etc/passwd"
    /// ```
    pub fn safe_path(&self) -> PathBuf {
        Path::new(&self.name)
            .components()
            .filter(|c| matches!(c, Component::Normal(_)))
            .collect()
    }
}

// ── Pure parsing helpers ──────────────────────────────────────────────────────

/// Scan `buffer` (which starts at byte `search_start` in the file) for the
/// end-of-central-directory signature and return its absolute file offset.
///
/// The search starts from the **end** of the buffer (most ZIPs have no
/// comment, so EOCD is right at the end) and works backwards.
///
/// Returns `None` if the signature is not found.
#[inline]
pub fn find_eocd_in_buffer(buffer: &[u8], search_start: u64) -> Option<u64> {
    for i in (0..buffer.len().saturating_sub(3)).rev() {
        if buffer[i] == 0x50
            && buffer[i + 1] == 0x4b
            && buffer[i + 2] == 0x05
            && buffer[i + 3] == 0x06
        {
            return Some(search_start + i as u64);
        }
    }
    None
}

/// Scan `buffer` for the ZIP64 EOCD locator signature (`PK\x06\x07`) and
/// return the absolute file offset of the ZIP64 EOCD *record* encoded inside
/// the locator.
///
/// `buffer` must include the region between the start of the file (or a
/// reasonable backward search window) and the EOCD record.
///
/// Returns `None` if the locator is not found or the buffer is too short.
#[inline]
pub fn find_zip64_eocd_offset(buffer: &[u8]) -> Option<u64> {
    for i in (0..buffer.len().saturating_sub(3)).rev() {
        if buffer[i] == 0x50
            && buffer[i + 1] == 0x4b
            && buffer[i + 2] == 0x06
            && buffer[i + 3] == 0x07
        {
            // locator layout (after sig): disk_with_zip64_eocd(4), zip64_eocd_offset(8), total_disks(4)
            if i + 16 > buffer.len() {
                return None;
            }
            let rel_off_bytes = &buffer[i + 8..i + 16];
            let offset = u64::from_le_bytes([
                rel_off_bytes[0],
                rel_off_bytes[1],
                rel_off_bytes[2],
                rel_off_bytes[3],
                rel_off_bytes[4],
                rel_off_bytes[5],
                rel_off_bytes[6],
                rel_off_bytes[7],
            ]);
            return Some(offset);
        }
    }
    None
}

/// Parse a ZIP64 extra field (tag `0x0001`) out of `extra_buf` and return
/// updated `(uncompressed_size, compressed_size, offset)`.
///
/// Only values whose 32-bit central-directory placeholder equals `0xFFFFFFFF`
/// are replaced; the others are returned unchanged.
///
/// If no ZIP64 extra field is found the input values are returned as-is.
#[inline]
pub fn parse_zip64_extra_field(
    extra_buf: &[u8],
    compressed_size_32: u64,
    uncompressed_size_32: u64,
    offset_32: u64,
) -> (u64, u64, u64) {
    let mut compressed_size = compressed_size_32;
    let mut uncompressed_size = uncompressed_size_32;
    let mut offset = offset_32;

    let mut i = 0usize;
    while i + 4 <= extra_buf.len() {
        let id = u16::from_le_bytes([extra_buf[i], extra_buf[i + 1]]);
        let data_len = u16::from_le_bytes([extra_buf[i + 2], extra_buf[i + 3]]) as usize;
        i += 4;
        if i + data_len > extra_buf.len() {
            break;
        }
        if id == 0x0001 {
            // ZIP64 extra field: values present in this order — original size,
            // compressed size, relative header offset, disk start — but only
            // when the corresponding 32-bit field holds the placeholder 0xFFFFFFFF.
            let mut cursor = 0usize;
            if uncompressed_size_32 == 0xFFFFFFFF && cursor + 8 <= data_len {
                uncompressed_size =
                    u64::from_le_bytes(extra_buf[i + cursor..i + cursor + 8].try_into().unwrap());
                cursor += 8;
            }
            if compressed_size_32 == 0xFFFFFFFF && cursor + 8 <= data_len {
                compressed_size =
                    u64::from_le_bytes(extra_buf[i + cursor..i + cursor + 8].try_into().unwrap());
                cursor += 8;
            }
            if offset_32 == 0xFFFFFFFF && cursor + 8 <= data_len {
                offset =
                    u64::from_le_bytes(extra_buf[i + cursor..i + cursor + 8].try_into().unwrap());
            }
            break;
        }
        i += data_len;
    }

    (uncompressed_size, compressed_size, offset)
}

/// Parse WinZip AES extra field (ID `0x9901`) from `extra_buf`.
///
/// Returns `Some((strength_code, data_offset))` where `strength_code` is the
/// single byte encoding the AES key length (0x01 = AES-128, 0x02 = AES-192,
/// 0x03 = AES-256) and `data_offset` is the byte position immediately after
/// the extra field (where the caller's further parsing stops — the salt and
/// password-verification bytes are read from the *file data*, not the extra
/// field itself).
///
/// Returns `None` if no AES extra field is found.
#[inline]
pub fn parse_aes_extra_field_buf(extra_buf: &[u8]) -> Option<u8> {
    let mut i = 0usize;
    while i + 4 <= extra_buf.len() {
        let id = u16::from_le_bytes([extra_buf[i], extra_buf[i + 1]]);
        let data_len = u16::from_le_bytes([extra_buf[i + 2], extra_buf[i + 3]]) as usize;
        i += 4;
        if i + data_len > extra_buf.len() {
            break;
        }
        if id == 0x9901 {
            // WinZip AES extra field: version(2)+vendor(2)+strength(1)+compression(2) = 7 bytes
            if data_len >= 7 {
                return Some(extra_buf[i + 4]);
            }
        }
        i += data_len;
    }
    None
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_eocd_in_buffer_found() {
        // Minimal valid EOCD: signature + 18 zeros
        let mut buf = vec![0u8; 40];
        buf[10] = 0x50;
        buf[11] = 0x4b;
        buf[12] = 0x05;
        buf[13] = 0x06;
        let offset = find_eocd_in_buffer(&buf, 1000).unwrap();
        assert_eq!(offset, 1010);
    }

    #[test]
    fn test_find_eocd_in_buffer_not_found() {
        let buf = vec![0u8; 40];
        assert!(find_eocd_in_buffer(&buf, 0).is_none());
    }

    #[test]
    fn test_parse_zip64_extra_no_placeholder() {
        // No 0xFFFFFFFF placeholders → values unchanged
        let extra = [];
        let (u, c, o) = parse_zip64_extra_field(&extra, 100, 200, 300);
        assert_eq!((u, c, o), (200, 100, 300));
    }

    #[test]
    fn test_parse_zip64_extra_with_zip64_field() {
        // Build a ZIP64 extra field with all three values
        let unc: u64 = 0xDEAD_BEEF_0000_0001;
        let com: u64 = 0xDEAD_BEEF_0000_0002;
        let off: u64 = 0xDEAD_BEEF_0000_0003;

        let mut extra = Vec::new();
        extra.extend_from_slice(&0x0001u16.to_le_bytes()); // tag
        extra.extend_from_slice(&24u16.to_le_bytes()); // data len = 3 * 8
        extra.extend_from_slice(&unc.to_le_bytes());
        extra.extend_from_slice(&com.to_le_bytes());
        extra.extend_from_slice(&off.to_le_bytes());

        let (u, c, o) = parse_zip64_extra_field(&extra, 0xFFFFFFFF, 0xFFFFFFFF, 0xFFFFFFFF);
        assert_eq!(u, unc);
        assert_eq!(c, com);
        assert_eq!(o, off);
    }

    #[test]
    fn test_safe_path_strips_dotdot() {
        let entry = ZipEntry {
            name: "../../etc/passwd".to_string(),
            compressed_size: 0,
            uncompressed_size: 0,
            compression_method: 0,
            offset: 0,
            crc32: 0,
            is_encrypted: false,
        };
        let p = entry.safe_path();
        assert_eq!(p, PathBuf::from("etc/passwd"));
    }
}
