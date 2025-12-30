//! Async streaming example - stream from file to ZIP
//!
//! This example demonstrates streaming large files into a ZIP archive
//! without loading everything into memory.
//!
//! Run with:
//! ```
//! cargo run --example async_streaming --features async
//! ```

use s_zip::{AsyncStreamingZipWriter, Result};
use std::path::Path;
use tokio::fs::File;
use tokio::io::AsyncReadExt;

#[tokio::main]
async fn main() -> Result<()> {
    println!("Async streaming example");

    // Create some test files first
    println!("\nCreating test files...");
    create_test_files().await.unwrap();

    println!("\nCreating ZIP archive with streaming...");
    let mut writer = AsyncStreamingZipWriter::new("async_streaming.zip").await?;

    // Stream file1.txt
    println!("  Streaming file1.txt...");
    stream_file_to_zip(&mut writer, "test_file1.txt", "file1.txt").await?;

    // Stream file2.txt
    println!("  Streaming file2.txt...");
    stream_file_to_zip(&mut writer, "test_file2.txt", "file2.txt").await?;

    // Stream file3.txt
    println!("  Streaming file3.txt...");
    stream_file_to_zip(&mut writer, "test_file3.txt", "documents/file3.txt").await?;

    writer.finish().await?;

    println!("\n✅ Successfully created async_streaming.zip");
    println!("   Verify with: unzip -l async_streaming.zip");
    println!("   Extract with: unzip async_streaming.zip");

    // Cleanup
    cleanup_test_files().await.unwrap();

    Ok(())
}

async fn stream_file_to_zip<W>(
    writer: &mut AsyncStreamingZipWriter<W>,
    source_path: &str,
    zip_entry_name: &str,
) -> Result<()>
where
    W: tokio::io::AsyncWrite + tokio::io::AsyncSeek + Unpin,
{
    writer.start_entry(zip_entry_name).await?;

    let mut file = File::open(source_path).await?;
    let mut buffer = vec![0u8; 8192]; // 8KB buffer for streaming

    let mut total_bytes = 0u64;
    loop {
        let n = file.read(&mut buffer).await?;
        if n == 0 {
            break;
        }
        writer.write_data(&buffer[..n]).await?;
        total_bytes += n as u64;
    }

    println!("    → Streamed {} bytes", total_bytes);

    Ok(())
}

async fn create_test_files() -> std::io::Result<()> {
    use tokio::io::AsyncWriteExt;

    let file1_content = "This is test file 1\n".repeat(1000);
    let mut file1 = File::create("test_file1.txt").await?;
    file1.write_all(file1_content.as_bytes()).await?;
    println!("  Created test_file1.txt ({} bytes)", file1_content.len());

    let file2_content = "This is test file 2 with different content\n".repeat(2000);
    let mut file2 = File::create("test_file2.txt").await?;
    file2.write_all(file2_content.as_bytes()).await?;
    println!("  Created test_file2.txt ({} bytes)", file2_content.len());

    let file3_content = "This is test file 3 in a subdirectory\n".repeat(500);
    let mut file3 = File::create("test_file3.txt").await?;
    file3.write_all(file3_content.as_bytes()).await?;
    println!("  Created test_file3.txt ({} bytes)", file3_content.len());

    Ok(())
}

async fn cleanup_test_files() -> std::io::Result<()> {
    for file in ["test_file1.txt", "test_file2.txt", "test_file3.txt"] {
        if Path::new(file).exists() {
            tokio::fs::remove_file(file).await?;
        }
    }
    Ok(())
}
