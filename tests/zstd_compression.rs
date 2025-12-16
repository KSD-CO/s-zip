#[cfg(feature = "zstd-support")]
#[test]
fn test_zstd_roundtrip() {
    use s_zip::{CompressionMethod, StreamingZipReader, StreamingZipWriter};
    use tempfile::tempdir;

    let dir = tempdir().unwrap();
    let zip_path = dir.path().join("zstd_test.zip");

    // Write a ZIP with Zstd compression
    {
        let mut writer = StreamingZipWriter::with_method(&zip_path, CompressionMethod::Zstd, 3)
            .expect("Failed to create writer");

        writer.start_entry("test1.txt").unwrap();
        writer.write_data(b"Hello from Zstd compression!").unwrap();

        writer.start_entry("test2.bin").unwrap();
        // Write some compressible data
        let data = vec![42u8; 10000];
        writer.write_data(&data).unwrap();

        writer.finish().unwrap();
    }

    // Read the ZIP back
    {
        let mut reader = StreamingZipReader::open(&zip_path).expect("Failed to open zip");

        // Clone entries data to avoid borrow checker issues
        let entries: Vec<_> = reader.entries().to_vec();

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name, "test1.txt");
        assert_eq!(entries[0].compression_method, 93); // Zstd method
        assert_eq!(entries[1].name, "test2.bin");
        assert_eq!(entries[1].compression_method, 93);

        // Read first entry
        let data1 = reader.read_entry_by_name("test1.txt").unwrap();
        assert_eq!(data1, b"Hello from Zstd compression!");

        // Read second entry
        let data2 = reader.read_entry_by_name("test2.bin").unwrap();
        assert_eq!(data2.len(), 10000);
        assert!(data2.iter().all(|&b| b == 42));

        // Verify compression actually happened (compressed should be much smaller)
        assert!(entries[1].compressed_size < entries[1].uncompressed_size / 2);
    }
}

#[cfg(feature = "zstd-support")]
#[test]
fn test_zstd_with_helper() {
    use s_zip::{StreamingZipReader, StreamingZipWriter};
    use tempfile::tempdir;

    let dir = tempdir().unwrap();
    let zip_path = dir.path().join("zstd_helper.zip");

    // Use the convenience method
    {
        let mut writer =
            StreamingZipWriter::with_zstd(&zip_path, 5).expect("Failed to create writer");

        writer.start_entry("data.txt").unwrap();
        writer
            .write_data(b"Testing Zstd with helper method")
            .unwrap();

        writer.finish().unwrap();
    }

    // Verify it can be read back
    {
        let mut reader = StreamingZipReader::open(&zip_path).expect("Failed to open zip");
        let data = reader.read_entry_by_name("data.txt").unwrap();
        assert_eq!(data, b"Testing Zstd with helper method");
    }
}

#[cfg(feature = "zstd-support")]
#[test]
fn test_zstd_streaming_read() {
    use s_zip::{CompressionMethod, StreamingZipReader, StreamingZipWriter};
    use std::io::Read;
    use tempfile::tempdir;

    let dir = tempdir().unwrap();
    let zip_path = dir.path().join("zstd_stream.zip");

    // Write with Zstd
    {
        let mut writer = StreamingZipWriter::with_method(&zip_path, CompressionMethod::Zstd, 3)
            .expect("Failed to create writer");

        writer.start_entry("large.bin").unwrap();
        let data = vec![0x55u8; 50000];
        writer.write_data(&data).unwrap();

        writer.finish().unwrap();
    }

    // Read with streaming API
    {
        let mut reader = StreamingZipReader::open(&zip_path).expect("Failed to open zip");
        let mut stream = reader
            .read_entry_streaming_by_name("large.bin")
            .expect("Failed to get stream");

        let mut buffer = Vec::new();
        stream.read_to_end(&mut buffer).unwrap();

        assert_eq!(buffer.len(), 50000);
        assert!(buffer.iter().all(|&b| b == 0x55));
    }
}
