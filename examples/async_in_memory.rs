//! Async in-memory ZIP example
//!
//! This example demonstrates creating a ZIP file entirely in memory
//! using async/await, which is useful for:
//! - Web applications (send ZIP as HTTP response)
//! - Cloud storage uploads (S3, GCS, etc.)
//! - Network streams
//! - Any scenario where you don't want to write to disk
//!
//! Run with:
//! ```
//! cargo run --example async_in_memory --features async
//! ```

use s_zip::{AsyncStreamingZipWriter, Result};
use std::io::Cursor;

#[tokio::main]
async fn main() -> Result<()> {
    println!("Async in-memory ZIP example\n");

    // Example 1: Create ZIP in memory
    println!("1. Creating ZIP in memory...");
    let zip_bytes = create_in_memory_zip().await?;
    println!("   ✅ Created {} bytes in memory", zip_bytes.len());

    // Write to file to verify
    std::fs::write("async_in_memory.zip", &zip_bytes).unwrap();
    println!("   💾 Saved to async_in_memory.zip for verification");

    // Example 2: Simulating HTTP upload
    println!("\n2. Simulating HTTP upload...");
    simulate_http_upload(zip_bytes.clone()).await;

    // Example 3: Simulating S3 upload
    println!("\n3. Simulating S3/cloud storage upload...");
    simulate_s3_upload(zip_bytes).await;

    println!("\n✅ All examples completed successfully!");
    println!("   Verify with: unzip -l async_in_memory.zip");

    Ok(())
}

async fn create_in_memory_zip() -> Result<Vec<u8>> {
    // Create a Vec<u8> buffer wrapped in Cursor for in-memory operations
    let buffer = Vec::new();
    let cursor = Cursor::new(buffer);

    // Create ZIP writer from cursor
    let mut writer = AsyncStreamingZipWriter::from_writer(cursor);

    // Add files to the in-memory ZIP
    writer.start_entry("readme.txt").await?;
    writer
        .write_data(b"This ZIP was created entirely in memory!\n")
        .await?;
    writer
        .write_data(b"No temporary files were used.\n")
        .await?;

    writer.start_entry("data/config.json").await?;
    let config = r#"{
  "app": "s-zip",
  "version": "0.3.1",
  "async": true,
  "in_memory": true
}"#;
    writer.write_data(config.as_bytes()).await?;

    writer.start_entry("data/sample.csv").await?;
    writer.write_data(b"id,name,value\n").await?;
    writer.write_data(b"1,Alice,100\n").await?;
    writer.write_data(b"2,Bob,200\n").await?;
    writer.write_data(b"3,Charlie,300\n").await?;

    // Finish and extract the bytes
    let cursor = writer.finish().await?;
    Ok(cursor.into_inner())
}

async fn simulate_http_upload(zip_bytes: Vec<u8>) {
    // In a real application, you would send this to an HTTP endpoint
    println!(
        "   📤 Uploading {} bytes to HTTP endpoint...",
        zip_bytes.len()
    );
    println!("   Example: POST /api/download with ZIP in body");

    // Simulate upload delay
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    println!("   ✅ Upload complete!");
    println!("   → Response: 200 OK");
    println!("   → Content-Type: application/zip");
    println!("   → Content-Length: {}", zip_bytes.len());
}

async fn simulate_s3_upload(zip_bytes: Vec<u8>) {
    // In a real application, you would use AWS SDK or similar
    println!("   ☁️  Uploading {} bytes to S3...", zip_bytes.len());
    println!("   Bucket: my-bucket");
    println!("   Key: exports/data-export.zip");

    // Simulate upload delay
    tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;

    println!("   ✅ Upload complete!");
    println!("   → ETag: \"abc123def456\"");
    println!("   → URL: s3://my-bucket/exports/data.zip");
}
