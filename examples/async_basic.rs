//! Basic async ZIP writer example
//!
//! This example demonstrates creating a ZIP file using async/await with Tokio runtime.
//!
//! Run with:
//! ```
//! cargo run --example async_basic --features async
//! ```

use s_zip::{AsyncStreamingZipWriter, Result};

#[tokio::main]
async fn main() -> Result<()> {
    println!("Creating async ZIP file: async_example.zip");

    // Create async writer
    let mut writer = AsyncStreamingZipWriter::new("async_example.zip").await?;

    // Add first entry
    writer.start_entry("hello.txt").await?;
    writer.write_data(b"Hello, async world!").await?;
    println!("  Added: hello.txt");

    // Add second entry
    writer.start_entry("data.txt").await?;
    writer
        .write_data(b"This is data written with async/await.\n")
        .await?;
    writer.write_data(b"Multiple writes work fine!\n").await?;
    println!("  Added: data.txt");

    // Add third entry with larger data
    writer.start_entry("large.txt").await?;
    let large_data = "This is a larger text that will be compressed.\n".repeat(100);
    writer.write_data(large_data.as_bytes()).await?;
    println!(
        "  Added: large.txt ({} bytes uncompressed)",
        large_data.len()
    );

    // Finish ZIP
    writer.finish().await?;

    println!("\n✅ Successfully created async_example.zip");
    println!("   Verify with: unzip -l async_example.zip");

    Ok(())
}
