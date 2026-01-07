//! Tests for async streaming ZIP reader

use s_zip::{AsyncStreamingZipReader, AsyncStreamingZipWriter, Result};
use tokio::io::AsyncReadExt;

#[tokio::test]
async fn test_async_reader_basic() -> Result<()> {
    // Create a test ZIP file
    let mut writer = AsyncStreamingZipWriter::new("test_async_reader.zip").await?;

    writer.start_entry("file1.txt").await?;
    writer.write_data(b"Hello, World!").await?;

    writer.start_entry("file2.txt").await?;
    writer.write_data(b"Second file content").await?;

    writer.finish().await?;

    // Read the ZIP file
    let mut reader = AsyncStreamingZipReader::open("test_async_reader.zip").await?;

    // Check entries
    assert_eq!(reader.entries().len(), 2);
    assert_eq!(reader.entries()[0].name, "file1.txt");
    assert_eq!(reader.entries()[1].name, "file2.txt");

    // Read first file
    let data1 = reader.read_entry_by_name("file1.txt").await?;
    assert_eq!(data1, b"Hello, World!");

    // Read second file
    let data2 = reader.read_entry_by_name("file2.txt").await?;
    assert_eq!(data2, b"Second file content");

    // Cleanup
    std::fs::remove_file("test_async_reader.zip").ok();

    Ok(())
}

#[tokio::test]
async fn test_async_reader_streaming() -> Result<()> {
    // Create a test ZIP with larger content
    let mut writer = AsyncStreamingZipWriter::new("test_async_streaming.zip").await?;

    writer.start_entry("large.txt").await?;
    let content = "This is a line of text.\n".repeat(1000);
    writer.write_data(content.as_bytes()).await?;

    writer.finish().await?;

    // Read using streaming
    let mut reader = AsyncStreamingZipReader::open("test_async_streaming.zip").await?;
    let mut stream = reader.read_entry_streaming_by_name("large.txt").await?;

    let mut buffer = Vec::new();
    stream.read_to_end(&mut buffer).await?;

    assert_eq!(buffer.len(), content.len());
    assert_eq!(buffer, content.as_bytes());

    // Cleanup
    std::fs::remove_file("test_async_streaming.zip").ok();

    Ok(())
}

#[tokio::test]
async fn test_async_reader_find_entry() -> Result<()> {
    // Create a test ZIP
    let mut writer = AsyncStreamingZipWriter::new("test_async_find.zip").await?;

    writer.start_entry("exists.txt").await?;
    writer.write_data(b"This file exists").await?;

    writer.finish().await?;

    // Test finding entries
    let reader = AsyncStreamingZipReader::open("test_async_find.zip").await?;

    assert!(reader.find_entry("exists.txt").is_some());
    assert!(reader.find_entry("missing.txt").is_none());

    // Cleanup
    std::fs::remove_file("test_async_find.zip").ok();

    Ok(())
}

#[tokio::test]
async fn test_async_reader_multiple_entries() -> Result<()> {
    // Create a ZIP with multiple entries
    let mut writer = AsyncStreamingZipWriter::new("test_async_multiple.zip").await?;

    for i in 0..10 {
        writer.start_entry(&format!("file{}.txt", i)).await?;
        writer
            .write_data(format!("Content of file {}", i).as_bytes())
            .await?;
    }

    writer.finish().await?;

    // Read and verify
    let mut reader = AsyncStreamingZipReader::open("test_async_multiple.zip").await?;

    assert_eq!(reader.entries().len(), 10);

    // Read each entry and verify
    for i in 0..10 {
        let data = reader.read_entry_by_name(&format!("file{}.txt", i)).await?;
        let expected = format!("Content of file {}", i);
        assert_eq!(data, expected.as_bytes());
    }

    // Cleanup
    std::fs::remove_file("test_async_multiple.zip").ok();

    Ok(())
}

#[tokio::test]
async fn test_async_reader_empty_file() -> Result<()> {
    // Create a ZIP with an empty file
    let mut writer = AsyncStreamingZipWriter::new("test_async_empty.zip").await?;

    writer.start_entry("empty.txt").await?;
    writer.write_data(b"").await?;

    writer.finish().await?;

    // Read the empty file
    let mut reader = AsyncStreamingZipReader::open("test_async_empty.zip").await?;
    let data = reader.read_entry_by_name("empty.txt").await?;

    assert_eq!(data.len(), 0);

    // Cleanup
    std::fs::remove_file("test_async_empty.zip").ok();

    Ok(())
}

#[tokio::test]
async fn test_async_reader_large_file() -> Result<()> {
    // Create a ZIP with a large file
    let mut writer = AsyncStreamingZipWriter::new("test_async_large.zip").await?;

    writer.start_entry("large_data.bin").await?;

    // Write 1MB of data
    let chunk = vec![42u8; 8192];
    for _ in 0..128 {
        writer.write_data(&chunk).await?;
    }

    writer.finish().await?;

    // Read using streaming
    let mut reader = AsyncStreamingZipReader::open("test_async_large.zip").await?;
    let mut stream = reader
        .read_entry_streaming_by_name("large_data.bin")
        .await?;

    let mut total_bytes = 0u64;
    let mut buffer = vec![0u8; 8192];

    loop {
        let n = stream.read(&mut buffer).await?;
        if n == 0 {
            break;
        }
        total_bytes += n as u64;
        // Verify all bytes are 42
        for &byte in &buffer[..n] {
            assert_eq!(byte, 42);
        }
    }

    assert_eq!(total_bytes, 1024 * 1024); // 1MB

    // Cleanup
    std::fs::remove_file("test_async_large.zip").ok();

    Ok(())
}

#[tokio::test]
async fn test_async_reader_binary_data() -> Result<()> {
    // Create a ZIP with binary data
    let mut writer = AsyncStreamingZipWriter::new("test_async_binary.zip").await?;

    writer.start_entry("binary.dat").await?;
    let binary_data: Vec<u8> = (0..=255).collect();
    writer.write_data(&binary_data).await?;

    writer.finish().await?;

    // Read and verify binary data
    let mut reader = AsyncStreamingZipReader::open("test_async_binary.zip").await?;
    let data = reader.read_entry_by_name("binary.dat").await?;

    assert_eq!(data.len(), 256);
    for (i, &byte) in data.iter().enumerate() {
        assert_eq!(byte, i as u8);
    }

    // Cleanup
    std::fs::remove_file("test_async_binary.zip").ok();

    Ok(())
}
