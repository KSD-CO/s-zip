//! Integration tests for v0.14.1 hardening:
//! - UTF-8 file name flag (general purpose bit 11)
//! - ExtractLimits (ZIP bomb protection)

use s_zip::{ExtractLimits, SZipError, StreamingZipReader, StreamingZipWriter};
use tempfile::NamedTempFile;

// ── UTF-8 file names ─────────────────────────────────────────────────────────

#[test]
fn test_utf8_flag_set_and_name_roundtrip() {
    let tmp = NamedTempFile::new().unwrap();
    let name = "thư mục/Báo cáo tháng 10.txt";
    {
        let mut writer = StreamingZipWriter::new(tmp.path()).unwrap();
        writer.add_entry(name, b"xin chao").unwrap();
        writer.finish().unwrap();
    }

    let bytes = std::fs::read(tmp.path()).unwrap();
    // Local header general purpose flag at offset 6
    let flags = u16::from_le_bytes([bytes[6], bytes[7]]);
    assert_ne!(flags & (1 << 11), 0, "UTF-8 flag must be set");

    let mut reader = StreamingZipReader::open(tmp.path()).unwrap();
    assert_eq!(reader.entries()[0].name, name);
    assert_eq!(reader.read_entry_by_name(name).unwrap(), b"xin chao");
}

// ── ExtractLimits ────────────────────────────────────────────────────────────

/// 20 MB of zeros compresses ~1000:1 with DEFLATE
fn write_bomb(path: &std::path::Path) {
    let mut writer = StreamingZipWriter::new(path).unwrap();
    writer
        .add_entry("zeros.bin", &vec![0u8; 20 * 1024 * 1024])
        .unwrap();
    writer.add_entry("small.txt", b"hello").unwrap();
    writer.finish().unwrap();
}

#[test]
fn test_default_limits_allow_normal_reads() {
    let tmp = NamedTempFile::new().unwrap();
    write_bomb(tmp.path());
    let mut reader = StreamingZipReader::open(tmp.path()).unwrap();
    assert_eq!(
        reader.read_entry_by_name("zeros.bin").unwrap().len(),
        20 * 1024 * 1024
    );
}

#[test]
fn test_ratio_limit_blocks_bomb() {
    let tmp = NamedTempFile::new().unwrap();
    write_bomb(tmp.path());
    let mut reader = StreamingZipReader::open(tmp.path()).unwrap();
    reader.set_limits(ExtractLimits {
        max_ratio: Some(100),
        ..ExtractLimits::unlimited()
    });
    let err = reader.read_entry_by_name("zeros.bin").unwrap_err();
    assert!(matches!(err, SZipError::LimitExceeded(_)), "{err:?}");
    // Small entry is still readable
    assert_eq!(reader.read_entry_by_name("small.txt").unwrap(), b"hello");
}

#[test]
fn test_streaming_read_enforces_limit() {
    let tmp = NamedTempFile::new().unwrap();
    write_bomb(tmp.path());
    let mut reader = StreamingZipReader::open(tmp.path()).unwrap();
    reader.set_limits(ExtractLimits {
        max_entry_size: Some(1024 * 1024),
        ..ExtractLimits::unlimited()
    });
    // Declared size is checked before any data is read
    let err = reader
        .read_entry_streaming_by_name("zeros.bin")
        .err()
        .unwrap();
    assert!(matches!(err, SZipError::LimitExceeded(_)));
}

#[test]
fn test_extract_all_total_and_entry_count_limits() {
    let tmp = NamedTempFile::new().unwrap();
    write_bomb(tmp.path());
    let out = tempfile::tempdir().unwrap();

    let mut reader = StreamingZipReader::open(tmp.path()).unwrap();
    reader.set_limits(ExtractLimits {
        max_total_size: Some(10 * 1024 * 1024),
        ..ExtractLimits::unlimited()
    });
    let err = reader.extract_all(out.path()).unwrap_err();
    assert!(matches!(err, SZipError::LimitExceeded(_)));

    reader.set_limits(ExtractLimits {
        max_entries: Some(1),
        ..ExtractLimits::unlimited()
    });
    let err = reader.extract_all(out.path()).unwrap_err();
    assert!(matches!(err, SZipError::LimitExceeded(_)));
}

#[cfg(feature = "async")]
#[tokio::test]
async fn test_async_ratio_limit_blocks_bomb() {
    let tmp = NamedTempFile::new().unwrap();
    write_bomb(tmp.path());
    let mut reader = s_zip::AsyncStreamingZipReader::open(tmp.path())
        .await
        .unwrap();
    // strict() allows 1000:1; tighten so the bomb trips it
    reader.set_limits(ExtractLimits {
        max_ratio: Some(100),
        ..ExtractLimits::strict()
    });
    let err = reader.read_entry_by_name("zeros.bin").await.unwrap_err();
    assert!(matches!(err, SZipError::LimitExceeded(_)), "{err:?}");

    // Declared size already exceeds the ratio budget, so streaming is refused up front
    let err = reader
        .read_entry_streaming_by_name("zeros.bin")
        .await
        .err()
        .unwrap();
    assert!(matches!(err, SZipError::LimitExceeded(_)));
}
