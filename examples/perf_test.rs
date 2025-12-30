//! Quick performance test - Sync vs Async
//!
//! Run with:
//! cargo run --release --example perf_test --features async

use s_zip::{AsyncStreamingZipWriter, Result, StreamingZipWriter};
use std::time::Instant;

fn test_sync(size_mb: usize) -> Result<std::time::Duration> {
    let start = Instant::now();

    let mut writer = StreamingZipWriter::new("/tmp/sync_perf_test.zip")?;

    let chunk = vec![b'X'; 1024 * 1024]; // 1MB
    for i in 0..size_mb {
        writer.start_entry(&format!("file_{}.bin", i))?;
        writer.write_data(&chunk)?;
    }

    writer.finish()?;

    Ok(start.elapsed())
}

async fn test_async(size_mb: usize) -> Result<std::time::Duration> {
    let start = Instant::now();

    let mut writer = AsyncStreamingZipWriter::new("/tmp/async_perf_test.zip").await?;

    let chunk = vec![b'X'; 1024 * 1024]; // 1MB
    for i in 0..size_mb {
        writer.start_entry(&format!("file_{}.bin", i)).await?;
        writer.write_data(&chunk).await?;
    }

    writer.finish().await?;

    Ok(start.elapsed())
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== Quick Performance Test: Sync vs Async ===\n");

    let sizes = vec![5, 10, 20];

    for size in sizes {
        println!("Testing with {}MB data:", size);

        // Test sync
        let sync_time = test_sync(size)?;
        let sync_ms = sync_time.as_millis();
        let sync_throughput = (size as f64 * 1024.0) / sync_time.as_secs_f64();

        // Test async
        let async_time = test_async(size).await?;
        let async_ms = async_time.as_millis();
        let async_throughput = (size as f64 * 1024.0) / async_time.as_secs_f64();

        // Calculate overhead
        let overhead_pct = ((async_ms as f64 - sync_ms as f64) / sync_ms as f64) * 100.0;

        println!("  Sync:  {:4}ms ({:6.1} KB/s)", sync_ms, sync_throughput);
        println!("  Async: {:4}ms ({:6.1} KB/s)", async_ms, async_throughput);
        println!("  Overhead: {:+.1}%", overhead_pct);
        println!();
    }

    // Cleanup
    let _ = std::fs::remove_file("/tmp/sync_perf_test.zip");
    let _ = std::fs::remove_file("/tmp/async_perf_test.zip");

    println!("✅ Performance test complete!");

    Ok(())
}
