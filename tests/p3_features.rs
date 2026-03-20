//! Integration tests for P3 features:
//! - SeeklessZipWriter (no Seek required)
//! - read_entries_parallel
//! - add_entry(), entry_count(), bytes_written()
//! - finish_with_stats()
//! - AES-128 / AES-192

use s_zip::{
    AsyncStreamingZipReader, AsyncStreamingZipWriter, SeeklessZipWriter, StreamingZipWriter,
};
use std::io::Cursor;
use tempfile::NamedTempFile;

// ── SeeklessZipWriter ────────────────────────────────────────────────────────

#[tokio::test]
async fn test_seekless_basic_roundtrip() {
    use tempfile::NamedTempFile;

    // Write seekless to a tmpfile (acts as plain AsyncWrite via tokio::fs::File)
    let tmp = NamedTempFile::new().unwrap();
    let path = tmp.path().to_path_buf();

    {
        let file = tokio::fs::File::create(&path).await.unwrap();
        let mut writer = SeeklessZipWriter::new(file);
        writer
            .add_entry("hello.txt", b"Hello, world!")
            .await
            .unwrap();
        writer
            .add_entry("data.txt", b"Some data here")
            .await
            .unwrap();
        writer.finish().await.unwrap();
    }

    // Verify starts with ZIP local sig
    let bytes = tokio::fs::read(&path).await.unwrap();
    assert_eq!(&bytes[..4], b"PK\x03\x04", "must start with ZIP local sig");

    // Read back with the sync reader
    let mut reader = s_zip::StreamingZipReader::open(&path).unwrap();
    let data = reader.read_entry_by_name("hello.txt").unwrap();
    assert_eq!(data, b"Hello, world!");
    let data2 = reader.read_entry_by_name("data.txt").unwrap();
    assert_eq!(data2, b"Some data here");
}

#[tokio::test]
async fn test_seekless_empty_archive() {
    use tempfile::NamedTempFile;
    use tokio::io::AsyncWriteExt;
    let tmp = NamedTempFile::new().unwrap();
    let file = tokio::fs::File::create(tmp.path()).await.unwrap();
    let writer = SeeklessZipWriter::new(file);
    let mut f = writer.finish().await.unwrap();
    f.flush().await.unwrap();
    drop(f);
    let bytes = tokio::fs::read(tmp.path()).await.unwrap();
    assert!(bytes.len() >= 22);
    let last4 = &bytes[bytes.len() - 22..bytes.len() - 18];
    assert_eq!(last4, b"PK\x05\x06");
}

#[tokio::test]
async fn test_seekless_entry_count_bytes_written() {
    use tempfile::NamedTempFile;
    let tmp = NamedTempFile::new().unwrap();
    let file = tokio::fs::File::create(tmp.path()).await.unwrap();
    let mut writer = SeeklessZipWriter::new(file);
    assert_eq!(writer.entry_count(), 0);
    assert_eq!(writer.bytes_written(), 0);

    writer.add_entry("a.txt", b"abc").await.unwrap();
    assert_eq!(writer.entry_count(), 1);
    assert_eq!(writer.bytes_written(), 3);

    writer.add_entry("b.txt", b"defgh").await.unwrap();
    assert_eq!(writer.entry_count(), 2);
    assert_eq!(writer.bytes_written(), 8);

    writer.finish().await.unwrap();
}

// ── add_entry / entry_count / bytes_written (sync) ──────────────────────────

#[test]
fn test_sync_add_entry_convenience() {
    let cursor = Cursor::new(Vec::new());
    let mut writer = StreamingZipWriter::from_writer(cursor).unwrap();
    // entry_count counts in-progress entries too
    assert_eq!(writer.entry_count(), 0);
    writer.add_entry("file.txt", b"content").unwrap();
    assert_eq!(writer.entry_count(), 1); // in-progress entry counted
                                         // bytes_written reflects completed entries; the entry is still in-progress
                                         // after add_entry until the next start_entry or finish
    writer.add_entry("file2.txt", b"more").unwrap(); // finishes the first entry
    assert_eq!(writer.entry_count(), 2);
    assert_eq!(writer.bytes_written(), 7); // first entry completed
    let cursor = writer.finish().unwrap();
    assert!(!cursor.into_inner().is_empty());
}

// ── finish_with_stats (sync) ─────────────────────────────────────────────────

#[test]
fn test_sync_finish_with_stats() {
    let cursor = Cursor::new(Vec::new());
    let mut writer = StreamingZipWriter::from_writer(cursor).unwrap();
    writer.add_entry("a.txt", b"aaaaaaaaaa").unwrap(); // compressible
    let (_w, stats) = writer.finish_with_stats().unwrap();

    assert_eq!(stats.entry_count, 1);
    assert_eq!(stats.total_uncompressed_bytes, 10);
    // compressed may be slightly larger for tiny data — just check it's present
    assert!(stats.total_compressed_bytes > 0);
    assert!(!stats.encrypted);
}

// ── finish_with_stats (async) ────────────────────────────────────────────────

#[tokio::test]
async fn test_async_finish_with_stats() {
    let cursor = Cursor::new(Vec::new());
    let mut writer = AsyncStreamingZipWriter::from_writer_with_compression(cursor, 6);
    writer.add_entry("data.txt", b"Hello!").await.unwrap();
    let (_w, stats) = writer.finish_with_stats().await.unwrap();

    assert_eq!(stats.entry_count, 1);
    assert_eq!(stats.total_uncompressed_bytes, 6);
    assert!(stats.total_compressed_bytes > 0);
    assert!(!stats.encrypted);
}

// ── read_entries_parallel ────────────────────────────────────────────────────

#[tokio::test]
async fn test_read_entries_parallel() {
    // Create a zip file on disk
    let tmp = NamedTempFile::new().unwrap();
    let path = tmp.path().to_path_buf();

    {
        let mut writer = AsyncStreamingZipWriter::new(&path).await.unwrap();
        writer.add_entry("a.txt", b"aaa").await.unwrap();
        writer.add_entry("b.txt", b"bbb").await.unwrap();
        writer.add_entry("c.txt", b"ccc").await.unwrap();
        writer.finish().await.unwrap();
    }

    let results = AsyncStreamingZipReader::read_entries_parallel(
        &path,
        vec!["a.txt".to_string(), "c.txt".to_string()],
        Some(2),
    )
    .await
    .unwrap();

    assert_eq!(results.len(), 2);
    // Results may come in any order
    let a = results.iter().find(|(n, _)| n == "a.txt").unwrap();
    let c = results.iter().find(|(n, _)| n == "c.txt").unwrap();
    assert_eq!(a.1, b"aaa");
    assert_eq!(c.1, b"ccc");
}

#[tokio::test]
async fn test_read_entries_parallel_missing_name_skipped() {
    let tmp = NamedTempFile::new().unwrap();
    let path = tmp.path().to_path_buf();

    {
        let mut writer = AsyncStreamingZipWriter::new(&path).await.unwrap();
        writer.add_entry("real.txt", b"exists").await.unwrap();
        writer.finish().await.unwrap();
    }

    let results = AsyncStreamingZipReader::read_entries_parallel(
        &path,
        vec!["real.txt".to_string(), "missing.txt".to_string()],
        None,
    )
    .await
    .unwrap();

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].0, "real.txt");
}

// ── AES-128 / AES-192 ────────────────────────────────────────────────────────

#[cfg(feature = "encryption")]
#[test]
fn test_aes128_roundtrip() {
    use s_zip::encryption::{AesDecryptor, AesEncryptor, AesStrength};

    let password = "test_128";
    let plaintext = b"AES-128 test data";

    let mut enc = AesEncryptor::new(password, AesStrength::Aes128).unwrap();
    let salt = enc.salt().to_vec();
    let pw_verify = *enc.password_verify();

    let mut data = plaintext.to_vec();
    enc.encrypt(&mut data).unwrap();
    let auth = enc.finalize();

    assert_ne!(data, plaintext.to_vec());

    let mut dec = AesDecryptor::new(password, AesStrength::Aes128, &salt, &pw_verify).unwrap();
    dec.decrypt(&mut data).unwrap();
    dec.verify_auth_code(&auth).unwrap();
    assert_eq!(data, plaintext.to_vec());
}

#[cfg(feature = "encryption")]
#[test]
fn test_aes192_roundtrip() {
    use s_zip::encryption::{AesDecryptor, AesEncryptor, AesStrength};

    let password = "test_192";
    let plaintext = b"AES-192 test data";

    let mut enc = AesEncryptor::new(password, AesStrength::Aes192).unwrap();
    let salt = enc.salt().to_vec();
    let pw_verify = *enc.password_verify();

    let mut data = plaintext.to_vec();
    enc.encrypt(&mut data).unwrap();
    let auth = enc.finalize();

    let mut dec = AesDecryptor::new(password, AesStrength::Aes192, &salt, &pw_verify).unwrap();
    dec.decrypt(&mut data).unwrap();
    dec.verify_auth_code(&auth).unwrap();
    assert_eq!(data, plaintext.to_vec());
}
