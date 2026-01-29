/// Example demonstrating optimized ZIP reading with adaptive buffer sizing
///
/// This example shows:
/// 1. How to use open_with_buffer_size() for better read performance
/// 2. Buffer size recommendations based on ZIP file size
/// 3. Comparison between default and optimized reading
use s_zip::{Result, StreamingZipReader};
use std::time::Instant;

fn main() -> Result<()> {
    // Create a sample ZIP file for testing
    println!("Creating test ZIP file...");
    create_test_zip("test_read.zip")?;

    // Test 1: Default buffer (512KB)
    println!("\n📖 Test 1: Reading with default buffer (512KB)");
    let start = Instant::now();
    let mut reader = StreamingZipReader::open("test_read.zip")?;
    let elapsed = start.elapsed();
    println!("   Opened in {:?}", elapsed);
    println!("   Found {} entries", reader.entries().len());
    test_read_entries(&mut reader)?;

    // Test 2: Small buffer for small ZIPs (64KB)
    println!("\n📖 Test 2: Reading with small buffer (64KB)");
    let start = Instant::now();
    let mut reader = StreamingZipReader::open_with_buffer_size("test_read.zip", Some(64 * 1024))?;
    let elapsed = start.elapsed();
    println!("   Opened in {:?}", elapsed);
    println!("   Found {} entries", reader.entries().len());
    test_read_entries(&mut reader)?;

    // Test 3: Large buffer for large ZIPs (2MB)
    println!("\n📖 Test 3: Reading with large buffer (2MB)");
    let start = Instant::now();
    let mut reader =
        StreamingZipReader::open_with_buffer_size("test_read.zip", Some(2 * 1024 * 1024))?;
    let elapsed = start.elapsed();
    println!("   Opened in {:?}", elapsed);
    println!("   Found {} entries", reader.entries().len());
    test_read_entries(&mut reader)?;

    // Clean up
    std::fs::remove_file("test_read.zip")?;
    println!("\n✅ All tests passed!");

    Ok(())
}

fn create_test_zip(path: &str) -> Result<()> {
    use s_zip::StreamingZipWriter;

    let mut writer = StreamingZipWriter::new(path)?;

    // Create 10 test files with different sizes
    for i in 0..10 {
        let name = format!("test_file_{}.txt", i);
        let size = (i + 1) * 1024 * 10; // 10KB, 20KB, ..., 100KB

        writer.start_entry(&name)?;
        let data = vec![b'A' + (i % 26) as u8; size];
        writer.write_data(&data)?;
    }

    writer.finish()?;
    Ok(())
}

fn test_read_entries(reader: &mut StreamingZipReader) -> Result<()> {
    let start = Instant::now();
    let mut total_size = 0u64;

    // Clone entries to avoid borrowing issues
    let entries = reader.entries().to_vec();

    for entry in &entries {
        let data = reader.read_entry(entry)?;
        total_size += data.len() as u64;
    }

    let elapsed = start.elapsed();
    let throughput = (total_size as f64 / 1024.0 / 1024.0) / elapsed.as_secs_f64();

    println!(
        "   Read {} bytes in {:?} ({:.2} MiB/s)",
        total_size, elapsed, throughput
    );

    Ok(())
}
