//! Example demonstrating Zstd compression support in s-zip
//!
//! This example requires the `zstd-support` feature to be enabled:
//! ```bash
//! cargo run --example zstd_compression --features zstd-support
//! ```

#[cfg(feature = "zstd-support")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use s_zip::{CompressionMethod, StreamingZipReader, StreamingZipWriter};

    println!("=== s-zip Zstd Compression Example ===\n");

    // Create a test file with Zstd compression
    let zip_path = "test_zstd.zip";
    println!("Creating {} with Zstd compression...", zip_path);

    {
        let mut writer = StreamingZipWriter::with_method(
            zip_path,
            CompressionMethod::Zstd,
            3, // Zstd compression level (1-21, higher = better compression)
        )?;

        // Add a text file
        writer.start_entry("hello_zstd.txt")?;
        writer.write_data(b"Hello from Zstd compression!")?;

        // Add a larger file with repetitive data (compresses well)
        writer.start_entry("data_zstd.txt")?;
        let data = "This is a test line with repetitive data.\n".repeat(100);
        writer.write_data(data.as_bytes())?;

        writer.finish()?;
    }
    println!("✓ Created {}\n", zip_path);

    // Read back the Zstd-compressed ZIP
    println!("Reading {}...", zip_path);
    let mut reader = StreamingZipReader::open(zip_path)?;

    println!("Entries in ZIP:");
    for entry in reader.entries() {
        println!(
            "  - {} ({} bytes, compressed: {} bytes, method: {})",
            entry.name, entry.uncompressed_size, entry.compressed_size, entry.compression_method
        );
    }
    println!();

    // Read and display content
    println!("Reading hello_zstd.txt:");
    let data = reader.read_entry_by_name("hello_zstd.txt")?;
    println!("  Content: {}\n", String::from_utf8_lossy(&data));

    println!("Reading data_zstd.txt (first 100 chars):");
    let data = reader.read_entry_by_name("data_zstd.txt")?;
    let preview = String::from_utf8_lossy(&data);
    println!(
        "  Content preview: {}...\n",
        &preview[..100.min(preview.len())]
    );

    // Compare compression ratios
    println!("Compression ratio analysis:");
    for entry in reader.entries() {
        if entry.uncompressed_size > 0 {
            let ratio = (entry.compressed_size as f64 / entry.uncompressed_size as f64) * 100.0;
            println!("  {}: {:.1}% of original size", entry.name, ratio);
        }
    }

    // Cleanup
    std::fs::remove_file(zip_path)?;
    println!("\n✓ All done! Cleaned up test file.");

    Ok(())
}

#[cfg(not(feature = "zstd-support"))]
fn main() {
    eprintln!("This example requires the 'zstd-support' feature.");
    eprintln!("Run with: cargo run --example zstd_compression --features zstd-support");
    std::process::exit(1);
}
