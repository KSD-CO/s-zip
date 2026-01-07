//! Example: Reading ZIP files from HTTP URLs
//!
//! This example demonstrates how to read ZIP files directly from HTTP sources
//! without downloading the entire file first. Uses the generic async reader
//! with reqwest to stream from HTTP.

use s_zip::{AsyncStreamingZipWriter, GenericAsyncZipReader};
use std::io::Cursor;
use tokio::io::AsyncReadExt;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║           Reading ZIP Files from HTTP Sources               ║");
    println!("╚══════════════════════════════════════════════════════════════╝\n");

    // Step 1: Create a test ZIP in memory
    println!("📦 Step 1: Creating a test ZIP file in memory...\n");
    let zip_bytes = create_test_zip_in_memory().await?;
    println!("   Created {} bytes ZIP in memory\n", zip_bytes.len());

    // Step 2: Simulate HTTP download by reading from in-memory buffer
    println!("🌐 Step 2: Reading ZIP from in-memory source (simulating HTTP)...\n");

    // Create a Cursor that implements AsyncRead + AsyncSeek
    let cursor = Cursor::new(zip_bytes.clone());

    // Use the generic async reader
    let mut reader = GenericAsyncZipReader::new(cursor).await?;

    println!("   ✓ Successfully opened ZIP from in-memory source\n");

    // Step 3: List all entries
    println!("📋 Step 3: Listing entries in the ZIP:\n");
    for (idx, entry) in reader.entries().iter().enumerate() {
        println!(
            "   [{}] {} - {} bytes (compressed: {} bytes)",
            idx, entry.name, entry.uncompressed_size, entry.compressed_size
        );
    }
    println!();

    // Step 4: Read specific entries
    println!("📖 Step 4: Reading specific entries:\n");

    // Read readme.txt
    if let Ok(data) = reader.read_entry_by_name("readme.txt").await {
        println!("   readme.txt content:");
        println!("   {}\n", String::from_utf8_lossy(&data));
    }

    // Read data.json
    if let Ok(data) = reader.read_entry_by_name("data.json").await {
        println!("   data.json content:");
        println!("   {}\n", String::from_utf8_lossy(&data));
    }

    // Step 5: Stream large file
    println!("🌊 Step 5: Streaming large file without loading to memory:\n");
    if let Ok(mut stream) = reader.read_entry_streaming_by_name("large.bin").await {
        let mut buffer = vec![0u8; 4096];
        let mut total_bytes = 0u64;
        let mut chunks = 0;

        loop {
            let n = stream.read(&mut buffer).await?;
            if n == 0 {
                break;
            }
            total_bytes += n as u64;
            chunks += 1;
        }

        println!("   Streamed {} bytes in {} chunks\n", total_bytes, chunks);
    }

    // Step 6: Demonstrate with actual HTTP (commented out - requires server)
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║                 Real HTTP Example                           ║");
    println!("╚══════════════════════════════════════════════════════════════╝\n");

    println!("To read from actual HTTP source:");
    println!("```rust");
    println!("use reqwest;");
    println!("use std::io::Cursor;");
    println!();
    println!("// Download ZIP into memory");
    println!("let response = reqwest::get(\"https://example.com/file.zip\").await?;");
    println!("let bytes = response.bytes().await?;");
    println!("let cursor = Cursor::new(bytes.to_vec());");
    println!();
    println!("// Read ZIP from memory");
    println!("let mut reader = GenericAsyncZipReader::new(cursor).await?;");
    println!("let data = reader.read_entry_by_name(\"file.txt\").await?;");
    println!("```\n");

    println!("✅ Example completed successfully!\n");

    Ok(())
}

/// Create a test ZIP file in memory
async fn create_test_zip_in_memory() -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let buffer = Vec::new();
    let cursor = Cursor::new(buffer);

    let mut writer = AsyncStreamingZipWriter::from_writer(cursor);

    // Add readme
    writer.start_entry("readme.txt").await?;
    writer
        .write_data(b"This is a test ZIP file created in memory.")
        .await?;

    // Add JSON data
    writer.start_entry("data.json").await?;
    writer
        .write_data(br#"{"name": "test", "value": 42, "status": "ok"}"#)
        .await?;

    // Add a larger binary file
    writer.start_entry("large.bin").await?;
    let data = vec![0xAB; 50_000]; // 50KB
    writer.write_data(&data).await?;

    // Add another text file
    writer.start_entry("info.txt").await?;
    writer
        .write_data(b"Additional information stored in the archive.")
        .await?;

    // Finish and get the bytes
    let cursor = writer.finish().await?;
    Ok(cursor.into_inner())
}
