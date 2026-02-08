//! Quick performance comparison test
//! Run with: cargo run --example perf_compare --release

use s_zip::{Result, StreamingZipWriter};
use std::io::Cursor;
use std::time::Instant;

fn main() -> Result<()> {
    println!("🚀 Quick Performance Test\n");

    // Test 1: Small files (1KB each)
    println!("Test 1: Writing 100 small files (1KB each)");
    let start = Instant::now();
    {
        let buffer = Vec::new();
        let cursor = Cursor::new(buffer);
        let mut writer = StreamingZipWriter::from_writer(cursor)?;

        let data = vec![b'A'; 1024]; // 1KB of data
        for i in 0..100 {
            writer.start_entry(&format!("file_{}.txt", i))?;
            writer.write_data(&data)?;
        }
        writer.finish()?;
    }
    let duration = start.elapsed();
    println!("   ✓ Time: {:?}", duration);
    println!(
        "   ✓ Throughput: {:.2} files/sec\n",
        100.0 / duration.as_secs_f64()
    );

    // Test 2: Medium file (1MB)
    println!("Test 2: Writing 1 medium file (1MB)");
    let start = Instant::now();
    {
        let buffer = Vec::new();
        let cursor = Cursor::new(buffer);
        let mut writer = StreamingZipWriter::from_writer(cursor)?;

        let data = vec![b'B'; 1024 * 1024]; // 1MB of data
        writer.start_entry("large_file.bin")?;
        writer.write_data(&data)?;
        writer.finish()?;
    }
    let duration = start.elapsed();
    let throughput_mb = 1.0 / duration.as_secs_f64();
    println!("   ✓ Time: {:?}", duration);
    println!("   ✓ Throughput: {:.2} MB/sec\n", throughput_mb);

    // Test 3: Many entries with varying sizes
    println!("Test 3: Writing 50 files with varying sizes (100B to 10KB)");
    let start = Instant::now();
    {
        let buffer = Vec::new();
        let cursor = Cursor::new(buffer);
        let mut writer = StreamingZipWriter::from_writer(cursor)?;

        for i in 0..50 {
            let size = 100 + (i * 200); // 100B to 10KB
            let data = vec![b'C'; size];
            writer.start_entry(&format!("varying_{}.dat", i))?;
            writer.write_data(&data)?;
        }
        writer.finish()?;
    }
    let duration = start.elapsed();
    println!("   ✓ Time: {:?}", duration);
    println!(
        "   ✓ Throughput: {:.2} files/sec\n",
        50.0 / duration.as_secs_f64()
    );

    // Test 4: Compression ratio test
    println!("Test 4: Compression ratio (highly compressible data)");
    let start = Instant::now();
    let zip_size = {
        let buffer = Vec::new();
        let cursor = Cursor::new(buffer);
        let mut writer = StreamingZipWriter::from_writer(cursor)?;

        let data = vec![b'X'; 100 * 1024]; // 100KB of same byte
        writer.start_entry("compressible.txt")?;
        writer.write_data(&data)?;

        let cursor = writer.finish()?;
        cursor.into_inner().len()
    };
    let duration = start.elapsed();
    let ratio = 100.0 * 1024.0 / zip_size as f64;
    println!("   ✓ Time: {:?}", duration);
    println!("   ✓ Original: 100 KB, Compressed: {} bytes", zip_size);
    println!("   ✓ Compression ratio: {:.2}x\n", ratio);

    println!("✅ All performance tests completed!");
    println!("\nNote: Run with --release flag for accurate performance numbers");

    Ok(())
}
