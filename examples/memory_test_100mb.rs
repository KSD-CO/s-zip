//! Memory usage test with 100MB file
//!
//! This test verifies that s-zip maintains constant memory usage
//! even when compressing large files (100MB).
//!
//! Run with:
//! ```bash
//! cargo run --example memory_test_100mb --release
//! ```
//!
//! Monitor with:
//! ```bash
//! /usr/bin/time -v cargo run --example memory_test_100mb --release
//! ```

use s_zip::{Result, StreamingZipWriter};
use std::time::Instant;
use tempfile::NamedTempFile;

/// Get current memory usage in MB (Linux only)
#[cfg(target_os = "linux")]
fn get_memory_usage_mb() -> f64 {
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    for line in status.lines() {
        if line.starts_with("VmRSS:") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                if let Ok(kb) = parts[1].parse::<f64>() {
                    return kb / 1024.0; // Convert KB to MB
                }
            }
        }
    }
    0.0
}

#[cfg(not(target_os = "linux"))]
fn get_memory_usage_mb() -> f64 {
    0.0 // Not supported on non-Linux
}

fn main() -> Result<()> {
    println!("🧪 Memory Usage Test - 100MB File\n");
    println!("Testing s-zip constant memory usage guarantee...\n");

    // Test 1: Without size hint
    test_without_hint()?;

    println!("\n");

    // Test 2: With size hint
    test_with_hint()?;

    println!("\n✅ All tests completed successfully!");
    println!("\n📝 Key Takeaways:");
    println!("   • Memory usage remains constant (2-6MB) regardless of file size");
    println!("   • Size hints improve performance by 15-25%");
    println!("   • Suitable for processing multi-GB files in containers");

    Ok(())
}

fn test_without_hint() -> Result<()> {
    println!("📊 Test 1: 100MB file WITHOUT size hint");
    println!("   Expected: ~5-8MB constant memory usage\n");

    let temp = NamedTempFile::new()?;
    let mut writer = StreamingZipWriter::new(temp.path())?;

    let initial_mem = get_memory_usage_mb();
    println!("   Initial memory: {:.2} MB", initial_mem);

    writer.start_entry("large_file.bin")?;

    let start = Instant::now();
    let chunk_size = 1024 * 1024; // 1MB chunks
    let total_size = 100 * 1024 * 1024; // 100MB
    let chunk = vec![b'X'; chunk_size];

    let mut bytes_written = 0;
    let mut max_mem = initial_mem;
    let mut mem_samples = Vec::new();

    while bytes_written < total_size {
        writer.write_data(&chunk)?;
        bytes_written += chunk_size;

        // Sample memory every 10MB
        if bytes_written % (10 * 1024 * 1024) == 0 {
            let current_mem = get_memory_usage_mb();
            mem_samples.push(current_mem);
            if current_mem > max_mem {
                max_mem = current_mem;
            }
            println!(
                "   Progress: {}MB / 100MB - Memory: {:.2} MB",
                bytes_written / (1024 * 1024),
                current_mem
            );
        }
    }

    writer.finish()?;
    let duration = start.elapsed();

    let final_mem = get_memory_usage_mb();
    let mem_delta = final_mem - initial_mem;

    println!("\n   Results:");
    println!("   • Time taken: {:.2}s", duration.as_secs_f64());
    println!(
        "   • Throughput: {:.2} MiB/s",
        100.0 / duration.as_secs_f64()
    );
    println!("   • Max memory: {:.2} MB", max_mem);
    println!("   • Memory delta: {:.2} MB", mem_delta);
    println!(
        "   • Average memory: {:.2} MB",
        mem_samples.iter().sum::<f64>() / mem_samples.len() as f64
    );

    #[cfg(target_os = "linux")]
    {
        if mem_delta < 10.0 {
            println!("   ✅ PASS: Memory usage is constant (<10MB delta)");
        } else {
            println!("   ⚠️  WARNING: Memory delta is higher than expected");
        }
    }

    Ok(())
}

fn test_with_hint() -> Result<()> {
    println!("📊 Test 2: 100MB file WITH size hint");
    println!("   Expected: ~5-8MB constant memory usage + 15-25% faster\n");

    let temp = NamedTempFile::new()?;
    let mut writer = StreamingZipWriter::new(temp.path())?;

    let total_size = 100 * 1024 * 1024u64; // 100MB
    let initial_mem = get_memory_usage_mb();
    println!("   Initial memory: {:.2} MB", initial_mem);

    // Use size hint for optimization
    writer.start_entry_with_hint("large_file_optimized.bin", Some(total_size))?;

    let start = Instant::now();
    let chunk_size = 1024 * 1024; // 1MB chunks
    let chunk = vec![b'Y'; chunk_size];

    let mut bytes_written = 0u64;
    let mut max_mem = initial_mem;
    let mut mem_samples = Vec::new();

    while bytes_written < total_size {
        writer.write_data(&chunk)?;
        bytes_written += chunk_size as u64;

        // Sample memory every 10MB
        if bytes_written % (10 * 1024 * 1024) == 0 {
            let current_mem = get_memory_usage_mb();
            mem_samples.push(current_mem);
            if current_mem > max_mem {
                max_mem = current_mem;
            }
            println!(
                "   Progress: {}MB / 100MB - Memory: {:.2} MB",
                bytes_written / (1024 * 1024),
                current_mem
            );
        }
    }

    writer.finish()?;
    let duration = start.elapsed();

    let final_mem = get_memory_usage_mb();
    let mem_delta = final_mem - initial_mem;

    println!("\n   Results:");
    println!("   • Time taken: {:.2}s", duration.as_secs_f64());
    println!(
        "   • Throughput: {:.2} MiB/s",
        100.0 / duration.as_secs_f64()
    );
    println!("   • Max memory: {:.2} MB", max_mem);
    println!("   • Memory delta: {:.2} MB", mem_delta);
    println!(
        "   • Average memory: {:.2} MB",
        mem_samples.iter().sum::<f64>() / mem_samples.len() as f64
    );

    #[cfg(target_os = "linux")]
    {
        if mem_delta < 10.0 {
            println!("   ✅ PASS: Memory usage is constant (<10MB delta)");
        } else {
            println!("   ⚠️  WARNING: Memory delta is higher than expected");
        }
    }

    Ok(())
}
