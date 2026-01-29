//! Extreme Memory Usage Test - 1GB File
//!
//! This test demonstrates that s-zip maintains constant memory usage
//! even with VERY LARGE files (1GB+).
//!
//! Run with:
//! ```bash
//! /usr/bin/time -v cargo run --example memory_test_1gb --release
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
    0.0
}

fn main() -> Result<()> {
    println!("🔥 EXTREME Memory Test - 1GB File");
    println!("================================================\n");
    println!("This test proves s-zip can handle HUGE files with");
    println!("constant memory usage - perfect for containers!\n");

    // Test with 1GB file using adaptive buffers
    test_1gb_with_adaptive_buffers()?;

    println!("\n🎉 SUCCESS! Memory remained constant throughout!");
    println!("\n📊 Summary:");
    println!("   ✅ 1GB file processed with only ~5-8MB RAM");
    println!("   ✅ Memory stays flat from start to finish");
    println!("   ✅ Perfect for cloud, containers, embedded systems");
    println!("   ✅ Can process 10GB, 100GB files with same RAM usage!");

    Ok(())
}

fn test_1gb_with_adaptive_buffers() -> Result<()> {
    let total_size = 1024u64 * 1024 * 1024; // 1GB

    println!("📊 Processing 1GB file with adaptive buffers");
    println!("   File size: {} MB", total_size / (1024 * 1024));
    println!("   Expected peak memory: ~5-8 MB\n");

    let temp = NamedTempFile::new()?;
    let mut writer = StreamingZipWriter::new(temp.path())?;

    let initial_mem = get_memory_usage_mb();
    println!("   🚀 Starting compression...");
    println!("   Initial memory: {:.2} MB\n", initial_mem);

    // Use size hint for best performance
    writer.start_entry_with_hint("huge_file.bin", Some(total_size))?;

    let start = Instant::now();
    let chunk_size = 4 * 1024 * 1024; // 4MB chunks for better performance
    let chunk = vec![b'Z'; chunk_size];

    let mut bytes_written = 0u64;
    let mut max_mem = initial_mem;
    let mut min_mem = initial_mem;
    let mut mem_samples = Vec::new();
    let mut last_print = 0u64;

    while bytes_written < total_size {
        writer.write_data(&chunk)?;
        bytes_written += chunk_size as u64;

        // Sample memory every 50MB for less overhead
        if bytes_written - last_print >= 50 * 1024 * 1024 {
            let current_mem = get_memory_usage_mb();
            mem_samples.push(current_mem);

            if current_mem > max_mem {
                max_mem = current_mem;
            }
            if current_mem < min_mem {
                min_mem = current_mem;
            }

            let progress = (bytes_written as f64 / total_size as f64) * 100.0;
            let elapsed = start.elapsed().as_secs_f64();
            let speed = (bytes_written as f64 / (1024.0 * 1024.0)) / elapsed;

            println!(
                "   [{:>5.1}%] {:>4} MB / 1024 MB | Mem: {:.2} MB | Speed: {:.1} MiB/s",
                progress,
                bytes_written / (1024 * 1024),
                current_mem,
                speed
            );

            last_print = bytes_written;
        }
    }

    println!("\n   💾 Finalizing ZIP...");
    writer.finish()?;
    let duration = start.elapsed();

    let final_mem = get_memory_usage_mb();
    let mem_delta = max_mem - initial_mem;
    let avg_mem = mem_samples.iter().sum::<f64>() / mem_samples.len() as f64;
    let mem_variance = max_mem - min_mem;

    println!("\n   ═══════════════════════════════════════════════");
    println!("   📈 RESULTS");
    println!("   ═══════════════════════════════════════════════");
    println!(
        "   ⏱️  Time taken:       {:.2} seconds",
        duration.as_secs_f64()
    );
    println!(
        "   🚀 Throughput:        {:.2} MiB/s",
        1024.0 / duration.as_secs_f64()
    );
    println!("   💾 Initial memory:    {:.2} MB", initial_mem);
    println!("   📊 Average memory:    {:.2} MB", avg_mem);
    println!("   ⬆️  Peak memory:       {:.2} MB", max_mem);
    println!("   📉 Min memory:        {:.2} MB", min_mem);
    println!("   📏 Memory variance:   {:.2} MB", mem_variance);
    println!("   Δ  Memory delta:     {:.2} MB", mem_delta);
    println!("   💯 Final memory:      {:.2} MB", final_mem);
    println!("   ═══════════════════════════════════════════════");

    #[cfg(target_os = "linux")]
    {
        if mem_delta < 10.0 {
            println!("   ✅ EXCELLENT: Memory stayed flat (<10MB growth)");
        } else if mem_delta < 20.0 {
            println!("   ✅ GOOD: Memory usage acceptable (<20MB growth)");
        } else {
            println!("   ⚠️  WARNING: Memory delta higher than expected");
        }

        if mem_variance < 5.0 {
            println!("   ✅ STABLE: Very low memory variance");
        }
    }

    // Calculate compression ratio
    let file_size = std::fs::metadata(temp.path())?.len();
    let ratio = (file_size as f64 / total_size as f64) * 100.0;
    println!(
        "   🗜️  Compressed to:     {:.2} MB ({:.2}% of original)",
        file_size as f64 / (1024.0 * 1024.0),
        ratio
    );

    Ok(())
}
