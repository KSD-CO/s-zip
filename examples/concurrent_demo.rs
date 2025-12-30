//! Concurrent ZIP creation demo - Where Async SHINES!
//!
//! This demonstrates the REAL advantage of async: creating multiple ZIP files
//! concurrently, which is impossible with sync code.
//!
//! Run with:
//! cargo run --release --example concurrent_demo --features async

use s_zip::{AsyncStreamingZipWriter, Result, StreamingZipWriter};
use std::time::Instant;
use tokio::task::JoinSet;

fn create_zip_sync(id: usize, size_kb: usize) -> Result<std::time::Duration> {
    let start = Instant::now();
    let path = format!("/tmp/sync_zip_{}.zip", id);

    let mut writer = StreamingZipWriter::new(&path)?;
    let data = vec![b'X'; size_kb * 1024];

    writer.start_entry("data.bin")?;
    writer.write_data(&data)?;
    writer.finish()?;

    Ok(start.elapsed())
}

async fn create_zip_async(id: usize, size_kb: usize) -> Result<std::time::Duration> {
    let start = Instant::now();
    let path = format!("/tmp/async_zip_{}.zip", id);

    let mut writer = AsyncStreamingZipWriter::new(&path).await?;
    let data = vec![b'X'; size_kb * 1024];

    writer.start_entry("data.bin").await?;
    writer.write_data(&data).await?;
    writer.finish().await?;

    Ok(start.elapsed())
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== Concurrent ZIP Creation Demo ===\n");
    println!("This shows where async REALLY matters!\n");

    let num_files = 10;
    let size_per_file = 500; // KB

    // ==========================================
    // SYNC: Sequential (one at a time)
    // ==========================================
    println!("📦 Creating {} ZIPs SEQUENTIALLY with sync...", num_files);
    let sync_start = Instant::now();

    for i in 0..num_files {
        create_zip_sync(i, size_per_file)?;
    }

    let sync_total = sync_start.elapsed();
    println!("   ⏱️  Total time: {:?}\n", sync_total);

    // ==========================================
    // ASYNC: Concurrent (all at once!)
    // ==========================================
    println!("🚀 Creating {} ZIPs CONCURRENTLY with async...", num_files);
    let async_start = Instant::now();

    let mut tasks = JoinSet::new();
    for i in 0..num_files {
        tasks.spawn(create_zip_async(i, size_per_file));
    }

    // Wait for all tasks to complete
    while let Some(result) = tasks.join_next().await {
        result.unwrap()?;
    }

    let async_total = async_start.elapsed();
    println!("   ⏱️  Total time: {:?}\n", async_total);

    // ==========================================
    // COMPARISON
    // ==========================================
    let speedup = sync_total.as_secs_f64() / async_total.as_secs_f64();
    let improvement = ((sync_total.as_millis() - async_total.as_millis()) as f64
        / sync_total.as_millis() as f64)
        * 100.0;

    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("📊 RESULTS:");
    println!("   Sync (sequential):  {:?}", sync_total);
    println!("   Async (concurrent): {:?}", async_total);
    println!("   Speedup:            {:.2}x faster", speedup);
    println!("   Time saved:         {:.1}%", improvement);
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    println!("\n✨ THIS is why async matters!");
    println!("   Sync has to wait for each file to finish.");
    println!("   Async can work on all files simultaneously.\n");

    // Cleanup
    for i in 0..num_files {
        let _ = std::fs::remove_file(format!("/tmp/sync_zip_{}.zip", i));
        let _ = std::fs::remove_file(format!("/tmp/async_zip_{}.zip", i));
    }

    Ok(())
}
