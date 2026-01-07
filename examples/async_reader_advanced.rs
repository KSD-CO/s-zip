//! Advanced async ZIP reader example
//!
//! This example demonstrates:
//! - Reading large ZIP files efficiently
//! - Streaming reads to minimize memory usage
//! - Processing entries in parallel
//! - Error handling

use s_zip::{AsyncStreamingZipReader, Result};
use tokio::io::AsyncReadExt;

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== Advanced Async ZIP Reader Example ===\n");

    // Create a sample ZIP with various files
    create_test_zip().await?;

    // Open the ZIP file
    let mut reader = AsyncStreamingZipReader::open("advanced_test.zip").await?;

    println!("ZIP archive contains {} entries\n", reader.entries().len());

    // Example 1: List all entries with detailed info
    println!("1. Listing all entries:");
    for (idx, entry) in reader.entries().iter().enumerate() {
        let compression_type = match entry.compression_method {
            0 => "Stored",
            8 => "Deflate",
            93 => "Zstd",
            _ => "Unknown",
        };
        let ratio = if entry.uncompressed_size > 0 {
            100.0 * (1.0 - entry.compressed_size as f64 / entry.uncompressed_size as f64)
        } else {
            0.0
        };
        println!(
            "   [{}] {} - {} ({:.1}% compression, {})",
            idx,
            entry.name,
            format_bytes(entry.uncompressed_size),
            ratio,
            compression_type
        );
    }
    println!();

    // Example 2: Read a small file completely
    println!("2. Reading small file 'readme.txt':");
    match reader.read_entry_by_name("readme.txt").await {
        Ok(data) => {
            let content = String::from_utf8_lossy(&data);
            println!("   Content: {}\n", content);
        }
        Err(e) => println!("   Error: {}\n", e),
    }

    // Example 3: Stream a large file (memory efficient)
    println!("3. Streaming large file 'data.bin':");
    match reader.read_entry_streaming_by_name("data.bin").await {
        Ok(mut stream) => {
            let mut total_bytes = 0u64;
            let mut buffer = vec![0u8; 8192]; // 8KB buffer

            loop {
                let n = stream.read(&mut buffer).await?;
                if n == 0 {
                    break;
                }
                total_bytes += n as u64;
                // Process chunk here (e.g., hash, parse, etc.)
            }

            println!("   Streamed {} total\n", format_bytes(total_bytes));
        }
        Err(e) => println!("   Error: {}\n", e),
    }

    // Example 4: Find and read specific files
    println!("4. Finding files by pattern:");
    let txt_files: Vec<_> = reader
        .entries()
        .iter()
        .filter(|e| e.name.ends_with(".txt"))
        .collect();

    println!("   Found {} .txt files:", txt_files.len());
    for entry in txt_files {
        println!("      - {}", entry.name);
    }
    println!();

    // Example 5: Check if specific file exists
    println!("5. Checking for specific files:");
    let files_to_check = vec!["readme.txt", "missing.txt", "data.bin"];
    for filename in files_to_check {
        let exists = reader.find_entry(filename).is_some();
        println!(
            "   {} {} in archive",
            filename,
            if exists { "exists" } else { "does NOT exist" }
        );
    }
    println!();

    // Example 6: Get statistics
    println!("6. Archive statistics:");
    let total_compressed: u64 = reader.entries().iter().map(|e| e.compressed_size).sum();
    let total_uncompressed: u64 = reader.entries().iter().map(|e| e.uncompressed_size).sum();
    let overall_ratio = if total_uncompressed > 0 {
        100.0 * (1.0 - total_compressed as f64 / total_uncompressed as f64)
    } else {
        0.0
    };

    println!("   Total compressed:   {}", format_bytes(total_compressed));
    println!(
        "   Total uncompressed: {}",
        format_bytes(total_uncompressed)
    );
    println!("   Overall compression: {:.1}%\n", overall_ratio);

    println!("=== Example completed successfully! ===");

    // Cleanup
    std::fs::remove_file("advanced_test.zip").ok();

    Ok(())
}

/// Create a test ZIP file with various content
async fn create_test_zip() -> Result<()> {
    use s_zip::AsyncStreamingZipWriter;

    let mut writer = AsyncStreamingZipWriter::new("advanced_test.zip").await?;

    // Add a readme
    writer.start_entry("readme.txt").await?;
    writer
        .write_data(b"This is a test ZIP archive with multiple files.")
        .await?;

    // Add a larger text file
    writer.start_entry("document.txt").await?;
    let doc = "Lorem ipsum dolor sit amet, consectetur adipiscing elit. ".repeat(100);
    writer.write_data(doc.as_bytes()).await?;

    // Add a binary-like file (less compressible)
    writer.start_entry("data.bin").await?;
    let mut data = Vec::new();
    for i in 0..10000 {
        data.extend_from_slice(&(i as u32).to_le_bytes());
    }
    writer.write_data(&data).await?;

    // Add another text file
    writer.start_entry("notes.txt").await?;
    writer.write_data(b"Some notes and observations.").await?;

    writer.finish().await?;

    println!("Created test ZIP file: advanced_test.zip\n");

    Ok(())
}

/// Format bytes into human-readable string
fn format_bytes(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut size = bytes as f64;
    let mut unit_idx = 0;

    while size >= 1024.0 && unit_idx < UNITS.len() - 1 {
        size /= 1024.0;
        unit_idx += 1;
    }

    if unit_idx == 0 {
        format!("{} {}", bytes, UNITS[0])
    } else {
        format!("{:.2} {}", size, UNITS[unit_idx])
    }
}
