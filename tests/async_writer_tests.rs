//! Tests for async ZIP writer
//!
//! Run with: cargo test --features async

#[cfg(feature = "async")]
mod async_tests {
    use s_zip::{AsyncStreamingZipWriter, Result, StreamingZipReader};
    use std::io::Cursor;
    use tempfile::NamedTempFile;
    use tokio::io::AsyncReadExt;

    #[tokio::test]
    async fn test_async_writer_basic() -> Result<()> {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path().to_path_buf();

        // Create async ZIP
        {
            let mut writer = AsyncStreamingZipWriter::new(&path).await?;
            writer.start_entry("test.txt").await?;
            writer.write_data(b"Hello, async!").await?;
            writer.finish().await?;
        }

        // Verify with sync reader
        let mut reader = StreamingZipReader::open(&path)?;
        assert_eq!(reader.entries().len(), 1);
        assert_eq!(reader.entries()[0].name, "test.txt");

        let data = reader.read_entry_by_name("test.txt")?;
        assert_eq!(data, b"Hello, async!");

        Ok(())
    }

    #[tokio::test]
    async fn test_async_writer_multiple_entries() -> Result<()> {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path().to_path_buf();

        // Create async ZIP with multiple entries
        {
            let mut writer = AsyncStreamingZipWriter::new(&path).await?;

            writer.start_entry("file1.txt").await?;
            writer.write_data(b"First file").await?;

            writer.start_entry("file2.txt").await?;
            writer.write_data(b"Second file").await?;

            writer.start_entry("file3.txt").await?;
            writer.write_data(b"Third file").await?;

            writer.finish().await?;
        }

        // Verify
        let mut reader = StreamingZipReader::open(&path)?;
        assert_eq!(reader.entries().len(), 3);

        let data1 = reader.read_entry_by_name("file1.txt")?;
        assert_eq!(data1, b"First file");

        let data2 = reader.read_entry_by_name("file2.txt")?;
        assert_eq!(data2, b"Second file");

        let data3 = reader.read_entry_by_name("file3.txt")?;
        assert_eq!(data3, b"Third file");

        Ok(())
    }

    #[tokio::test]
    async fn test_async_writer_large_data() -> Result<()> {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path().to_path_buf();

        // Create large data (2MB to trigger buffer flushing)
        let large_data = vec![b'X'; 2 * 1024 * 1024];

        // Create async ZIP
        {
            let mut writer = AsyncStreamingZipWriter::new(&path).await?;
            writer.start_entry("large.bin").await?;
            writer.write_data(&large_data).await?;
            writer.finish().await?;
        }

        // Verify
        let mut reader = StreamingZipReader::open(&path)?;
        let data = reader.read_entry_by_name("large.bin")?;
        assert_eq!(data.len(), large_data.len());
        assert_eq!(data, large_data);

        Ok(())
    }

    #[tokio::test]
    async fn test_async_writer_in_memory() -> Result<()> {
        // Create ZIP in memory
        let buffer = Vec::new();
        let cursor = Cursor::new(buffer);

        let mut writer = AsyncStreamingZipWriter::from_writer(cursor);
        writer.start_entry("memory.txt").await?;
        writer.write_data(b"In-memory async ZIP").await?;

        let cursor = writer.finish().await?;
        let zip_bytes = cursor.into_inner();

        // Verify we got some data
        assert!(!zip_bytes.is_empty());

        // Write to temp file and verify with sync reader
        let temp_file = NamedTempFile::new().unwrap();
        std::fs::write(temp_file.path(), &zip_bytes).unwrap();

        let mut reader = StreamingZipReader::open(temp_file.path())?;
        let data = reader.read_entry_by_name("memory.txt")?;
        assert_eq!(data, b"In-memory async ZIP");

        Ok(())
    }

    #[tokio::test]
    async fn test_async_writer_multiple_writes() -> Result<()> {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path().to_path_buf();

        // Create async ZIP with multiple writes to same entry
        {
            let mut writer = AsyncStreamingZipWriter::new(&path).await?;
            writer.start_entry("chunks.txt").await?;
            writer.write_data(b"Chunk 1\n").await?;
            writer.write_data(b"Chunk 2\n").await?;
            writer.write_data(b"Chunk 3\n").await?;
            writer.finish().await?;
        }

        // Verify
        let mut reader = StreamingZipReader::open(&path)?;
        let data = reader.read_entry_by_name("chunks.txt")?;
        assert_eq!(data, b"Chunk 1\nChunk 2\nChunk 3\n");

        Ok(())
    }

    #[tokio::test]
    async fn test_async_writer_streaming_from_file() -> Result<()> {
        // Create a temp source file
        let source_file = NamedTempFile::new().unwrap();
        let source_data = b"This is source data that will be streamed";
        std::fs::write(source_file.path(), source_data).unwrap();

        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path().to_path_buf();

        // Stream from file to ZIP
        {
            let mut writer = AsyncStreamingZipWriter::new(&path).await?;
            writer.start_entry("streamed.txt").await?;

            let mut file = tokio::fs::File::open(source_file.path()).await.unwrap();
            let mut buffer = vec![0u8; 8192];
            loop {
                let n = file.read(&mut buffer).await.unwrap();
                if n == 0 {
                    break;
                }
                writer.write_data(&buffer[..n]).await?;
            }

            writer.finish().await?;
        }

        // Verify
        let mut reader = StreamingZipReader::open(&path)?;
        let data = reader.read_entry_by_name("streamed.txt")?;
        assert_eq!(data, source_data);

        Ok(())
    }

    #[tokio::test]
    async fn test_async_writer_custom_compression_level() -> Result<()> {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path().to_path_buf();

        // Create with custom compression level
        {
            let mut writer = AsyncStreamingZipWriter::with_compression(&path, 9).await?;
            writer.start_entry("compressed.txt").await?;
            let data = "Compress this text with maximum compression!".repeat(100);
            writer.write_data(data.as_bytes()).await?;
            writer.finish().await?;
        }

        // Verify
        let mut reader = StreamingZipReader::open(&path)?;
        let data = reader.read_entry_by_name("compressed.txt")?;
        let expected = "Compress this text with maximum compression!".repeat(100);
        assert_eq!(data, expected.as_bytes());

        Ok(())
    }

    #[tokio::test]
    async fn test_async_writer_empty_file() -> Result<()> {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path().to_path_buf();

        // Create ZIP with empty file
        {
            let mut writer = AsyncStreamingZipWriter::new(&path).await?;
            writer.start_entry("empty.txt").await?;
            // Don't write any data
            writer.finish().await?;
        }

        // Verify
        let mut reader = StreamingZipReader::open(&path)?;
        let data = reader.read_entry_by_name("empty.txt")?;
        assert_eq!(data.len(), 0);

        Ok(())
    }
}
