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

// ── Entry metadata and comments ──────────────────────────────────────────────

fn options() -> s_zip::EntryOptions {
    // 2024-05-17 13:45:30 UTC
    let mtime = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_715_953_530);
    s_zip::EntryOptions {
        mtime: Some(mtime),
        unix_mode: Some(0o755),
        comment: Some("entry note".to_string()),
    }
}

/// Check the archive with Info-ZIP: integrity, permissions, date and comments.
fn check_with_info_zip(path: &std::path::Path) {
    let Ok(test) = std::process::Command::new("unzip")
        .arg("-t")
        .arg(path)
        .output()
    else {
        eprintln!("unzip not installed; skipping");
        return;
    };
    assert!(
        test.status.success(),
        "{}",
        String::from_utf8_lossy(&test.stdout)
    );

    let info = std::process::Command::new("zipinfo")
        .arg(path)
        .output()
        .unwrap();
    let info = String::from_utf8_lossy(&info.stdout);
    assert!(info.contains("-rwxr-xr-x"), "{info}");
    assert!(info.contains("unx"), "{info}");
    assert!(info.contains("24-May-17 13:45"), "{info}");

    let comments = std::process::Command::new("unzip")
        .arg("-z")
        .arg(path)
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&comments.stdout).contains("archive note"));
}

fn check_with_reader(path: &std::path::Path) {
    let mut reader = StreamingZipReader::open(path).unwrap();
    assert_eq!(reader.comment(), "archive note");
    assert_eq!(reader.entries()[0].comment, "entry note");
    assert_eq!(
        reader.read_entry_by_name("run.sh").unwrap(),
        b"#!/bin/sh\necho hi\n"
    );
}

#[test]
fn test_sync_writer_metadata_and_comments() {
    let tmp = NamedTempFile::new().unwrap();
    let mut writer = StreamingZipWriter::new(tmp.path()).unwrap();
    writer.set_comment("archive note").unwrap();
    writer
        .start_entry_with_options("run.sh", options())
        .unwrap();
    writer.write_data(b"#!/bin/sh\necho hi\n").unwrap();
    writer.finish().unwrap();
    check_with_info_zip(tmp.path());
    check_with_reader(tmp.path());
}

#[cfg(feature = "encryption")]
#[test]
fn test_sync_writer_unix_mode_with_encryption() {
    // The AES salt must come after the whole extra field (Unix extra included)
    let tmp = NamedTempFile::new().unwrap();
    let mut writer = StreamingZipWriter::new(tmp.path()).unwrap();
    writer.set_password("pw");
    writer
        .start_entry_with_options("run.sh", options())
        .unwrap();
    writer.write_data(b"secret").unwrap();
    writer.finish().unwrap();
    let mut reader = StreamingZipReader::open(tmp.path()).unwrap();
    reader.set_password("pw");
    assert_eq!(reader.read_entry_by_name("run.sh").unwrap(), b"secret");
}

#[test]
fn test_comment_too_long_rejected() {
    let mut writer = StreamingZipWriter::from_writer(std::io::Cursor::new(Vec::new())).unwrap();
    assert!(writer.set_comment("x".repeat(70_000)).is_err());
}

#[cfg(feature = "async")]
#[tokio::test]
async fn test_async_writer_metadata_and_comments() {
    let tmp = NamedTempFile::new().unwrap();
    let mut writer = s_zip::AsyncStreamingZipWriter::new(tmp.path())
        .await
        .unwrap();
    writer.set_comment("archive note").unwrap();
    writer
        .start_entry_with_options("run.sh", options())
        .await
        .unwrap();
    writer.write_data(b"#!/bin/sh\necho hi\n").await.unwrap();
    writer.finish().await.unwrap();
    check_with_info_zip(tmp.path());
    check_with_reader(tmp.path());

    let mut reader = s_zip::AsyncStreamingZipReader::open(tmp.path())
        .await
        .unwrap();
    assert_eq!(reader.comment(), "archive note");
    assert_eq!(reader.entries()[0].comment, "entry note");
    assert_eq!(
        reader.read_entry_by_name("run.sh").await.unwrap(),
        b"#!/bin/sh\necho hi\n"
    );
}

#[cfg(feature = "async")]
#[tokio::test]
async fn test_seekless_writer_metadata_and_comments() {
    let tmp = NamedTempFile::new().unwrap();
    let file = tokio::fs::File::create(tmp.path()).await.unwrap();
    let mut writer = s_zip::SeeklessZipWriter::new(file);
    writer.set_comment("archive note").unwrap();
    writer
        .add_entry_with_options("run.sh", b"#!/bin/sh\necho hi\n", options())
        .await
        .unwrap();
    writer.finish().await.unwrap();
    check_with_info_zip(tmp.path());
    check_with_reader(tmp.path());
}
