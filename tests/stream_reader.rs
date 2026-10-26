//! Integration tests for ZipStreamReader (sequential reads without Seek).

#![cfg(feature = "async")]

use s_zip::{
    AsyncStreamingZipWriter, CompressionMethod, ExtractLimits, SZipError, SeeklessZipWriter,
    StreamingZipWriter, ZipStreamReader,
};
use std::io::Cursor;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn sync_archive(method: CompressionMethod) -> Vec<u8> {
    let mut writer =
        StreamingZipWriter::from_writer_with_method(Cursor::new(Vec::new()), method, 6).unwrap();
    writer.add_entry("a.txt", b"hello hello hello").unwrap();
    writer.add_entry("dir/b.bin", &vec![7u8; 100_000]).unwrap();
    writer.add_entry("empty.txt", b"").unwrap();
    writer.finish().unwrap().into_inner()
}

async fn read_all(bytes: Vec<u8>) -> s_zip::Result<Vec<(String, Vec<u8>)>> {
    let mut zip = ZipStreamReader::new(Cursor::new(bytes));
    let mut out = Vec::new();
    while let Some(entry) = zip.next_entry().await? {
        out.push((entry.name, zip.read_entry_to_end().await?));
    }
    Ok(out)
}

fn assert_expected(entries: &[(String, Vec<u8>)]) {
    assert_eq!(entries.len(), 3);
    assert_eq!(
        entries[0],
        ("a.txt".to_string(), b"hello hello hello".to_vec())
    );
    assert_eq!(entries[1], ("dir/b.bin".to_string(), vec![7u8; 100_000]));
    assert_eq!(entries[2], ("empty.txt".to_string(), Vec::new()));
}

#[tokio::test]
async fn test_reads_deflate_with_data_descriptors() {
    assert_expected(
        &read_all(sync_archive(CompressionMethod::Deflate))
            .await
            .unwrap(),
    );
}

#[cfg(feature = "async-zstd")]
#[tokio::test]
async fn test_reads_zstd_with_data_descriptors() {
    assert_expected(
        &read_all(sync_archive(CompressionMethod::Zstd))
            .await
            .unwrap(),
    );
}

#[tokio::test]
async fn test_rejects_stored_entry_without_size() {
    let err = read_all(sync_archive(CompressionMethod::Stored))
        .await
        .unwrap_err();
    assert!(matches!(err, SZipError::InvalidFormat(_)), "{err:?}");
}

#[tokio::test]
async fn test_reads_async_writer_output() {
    let mut writer = AsyncStreamingZipWriter::from_writer(Cursor::new(Vec::new()));
    writer
        .add_entry("a.txt", b"hello hello hello")
        .await
        .unwrap();
    writer
        .add_entry("dir/b.bin", &vec![7u8; 100_000])
        .await
        .unwrap();
    writer.add_entry("empty.txt", b"").await.unwrap();
    let bytes = writer.finish().await.unwrap().into_inner();
    assert_expected(&read_all(bytes).await.unwrap());
}

#[tokio::test]
async fn test_reads_seekless_output_from_pipe() {
    // Producer writes into one end of an in-memory pipe; consumer can never seek
    let (tx, rx) = tokio::io::duplex(4096);
    let producer = tokio::spawn(async move {
        let mut writer = SeeklessZipWriter::new(tx);
        writer
            .add_entry("a.txt", b"hello hello hello")
            .await
            .unwrap();
        writer
            .add_entry("dir/b.bin", &vec![7u8; 100_000])
            .await
            .unwrap();
        writer.add_entry("empty.txt", b"").await.unwrap();
        let mut tx = writer.finish().await.unwrap();
        tx.shutdown().await.unwrap();
    });

    let mut zip = ZipStreamReader::new(rx);
    let mut entries = Vec::new();
    while let Some(entry) = zip.next_entry().await.unwrap() {
        entries.push((entry.name, zip.read_entry_to_end().await.unwrap()));
    }
    producer.await.unwrap();
    assert_expected(&entries);
}

#[tokio::test]
async fn test_skipping_unread_entries() {
    let mut zip = ZipStreamReader::new(Cursor::new(sync_archive(CompressionMethod::Deflate)));
    // Partially read the first entry, skip the second entirely
    zip.next_entry().await.unwrap().unwrap();
    let mut first = [0u8; 5];
    zip.entry_reader().read_exact(&mut first).await.unwrap();
    assert_eq!(&first, b"hello");
    assert_eq!(zip.next_entry().await.unwrap().unwrap().name, "dir/b.bin");
    let last = zip.next_entry().await.unwrap().unwrap();
    assert_eq!(last.name, "empty.txt");
    assert!(zip.next_entry().await.unwrap().is_none());
}

#[tokio::test]
async fn test_detects_corrupted_data() {
    let mut bytes = sync_archive(CompressionMethod::Deflate);
    // Corrupt the CRC in the first data descriptor (follows the PK\x07\x08 signature)
    let dd = bytes
        .windows(4)
        .position(|w| w == [0x50, 0x4b, 0x07, 0x08])
        .unwrap();
    bytes[dd + 4] ^= 0xFF;
    let err = read_all(bytes).await.unwrap_err();
    assert!(
        matches!(&err, SZipError::InvalidFormat(m) if m.contains("CRC")),
        "{err:?}"
    );
}

#[tokio::test]
async fn test_limits_apply_to_unknown_size_entries() {
    let mut writer = StreamingZipWriter::from_writer(Cursor::new(Vec::new())).unwrap();
    writer
        .add_entry("zeros.bin", &vec![0u8; 20 * 1024 * 1024])
        .unwrap();
    let bytes = writer.finish().unwrap().into_inner();

    let mut zip = ZipStreamReader::new(Cursor::new(bytes));
    zip.set_limits(ExtractLimits {
        max_ratio: Some(100),
        ..ExtractLimits::unlimited()
    });
    zip.next_entry().await.unwrap().unwrap();
    let err = zip.read_entry_to_end().await.unwrap_err();
    assert!(matches!(err, SZipError::LimitExceeded(_)), "{err:?}");
}

/// Archives produced by Info-ZIP `zip` (known sizes, and data descriptors when
/// writing to a pipe). Skipped when the tool is not installed.
#[tokio::test]
async fn test_reads_info_zip_archives() {
    if std::process::Command::new("zip")
        .arg("-v")
        .output()
        .is_err()
    {
        eprintln!("zip not installed; skipping");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), b"hello hello hello").unwrap();
    std::fs::write(dir.path().join("b.bin"), vec![7u8; 100_000]).unwrap();

    // Regular file output (sizes in local headers)
    let status = std::process::Command::new("zip")
        .current_dir(dir.path())
        .args(["-q", "out.zip", "a.txt", "b.bin"])
        .status()
        .unwrap();
    assert!(status.success());
    let entries = read_all(std::fs::read(dir.path().join("out.zip")).unwrap())
        .await
        .unwrap();
    assert_eq!(entries[0].1, b"hello hello hello");
    assert_eq!(entries[1].1, vec![7u8; 100_000]);

    // Streaming output to stdout (data descriptors, unknown sizes)
    let output = std::process::Command::new("zip")
        .current_dir(dir.path())
        .args(["-q", "-", "a.txt", "b.bin"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let entries = read_all(output.stdout).await.unwrap();
    assert_eq!(entries[0].1, b"hello hello hello");
    assert_eq!(entries[1].1, vec![7u8; 100_000]);
}
