//! Network I/O simulation - Where Async REALLY shines!
//!
//! This simulates slow I/O (network, cloud storage) where async has huge advantage.
//! We add artificial delays to simulate network latency.
//!
//! Run with:
//! cargo run --release --example network_simulation --features async

use s_zip::AsyncStreamingZipWriter;
use std::io::Cursor;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};
use tokio::io::{AsyncSeek, AsyncWrite};

/// Simulated "slow network" writer - adds 5ms delay per write
struct SlowNetworkWriter {
    inner: Cursor<Vec<u8>>,
}

impl SlowNetworkWriter {
    fn new() -> Self {
        Self {
            inner: Cursor::new(Vec::new()),
        }
    }
}

impl AsyncWrite for SlowNetworkWriter {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        // Simulate network delay
        std::thread::sleep(Duration::from_millis(5));
        Pin::new(&mut self.inner).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

impl AsyncSeek for SlowNetworkWriter {
    fn start_seek(mut self: Pin<&mut Self>, position: std::io::SeekFrom) -> std::io::Result<()> {
        Pin::new(&mut self.inner).start_seek(position)
    }

    fn poll_complete(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<u64>> {
        Pin::new(&mut self.inner).poll_complete(cx)
    }
}

impl Unpin for SlowNetworkWriter {}

async fn create_zip_with_network_delay(_id: usize) -> Duration {
    let start = Instant::now();

    let writer = SlowNetworkWriter::new();
    let mut zip_writer = AsyncStreamingZipWriter::from_writer(writer);

    let data = vec![b'X'; 50 * 1024]; // 50KB

    zip_writer.start_entry("data.bin").await.unwrap();
    zip_writer.write_data(&data).await.unwrap();
    zip_writer.finish().await.unwrap();

    start.elapsed()
}

#[tokio::main]
async fn main() {
    println!("=== Network I/O Simulation ===\n");
    println!("Simulating slow network with 5ms latency per write\n");

    let num_zips = 5;

    // ==========================================
    // SEQUENTIAL (like sync would have to do)
    // ==========================================
    println!("📦 Creating {} ZIPs SEQUENTIALLY...", num_zips);
    let seq_start = Instant::now();

    for i in 0..num_zips {
        create_zip_with_network_delay(i).await;
    }

    let seq_total = seq_start.elapsed();
    println!("   ⏱️  Total time: {:?}\n", seq_total);

    // ==========================================
    // CONCURRENT (async advantage!)
    // ==========================================
    println!("🚀 Creating {} ZIPs CONCURRENTLY...", num_zips);
    let concurrent_start = Instant::now();

    let tasks: Vec<_> = (0..num_zips)
        .map(|i| tokio::spawn(create_zip_with_network_delay(i)))
        .collect();

    for task in tasks {
        task.await.unwrap();
    }

    let concurrent_total = concurrent_start.elapsed();
    println!("   ⏱️  Total time: {:?}\n", concurrent_total);

    // ==========================================
    // COMPARISON
    // ==========================================
    let speedup = seq_total.as_secs_f64() / concurrent_total.as_secs_f64();
    let time_saved = seq_total.saturating_sub(concurrent_total);

    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("📊 RESULTS:");
    println!("   Sequential:  {:?}", seq_total);
    println!("   Concurrent:  {:?}", concurrent_total);
    println!("   Speedup:     {:.2}x faster", speedup);
    println!("   Time saved:  {:?}", time_saved);
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    println!("\n✨ With slow I/O (network, cloud), async is MUCH faster!");
    println!("   Sequential waits for each upload to finish.");
    println!(
        "   Concurrent uploads all {} files simultaneously.",
        num_zips
    );
    println!("\n💡 This is typical for:");
    println!("   - HTTP file uploads");
    println!("   - S3/GCS cloud storage");
    println!("   - WebSocket streams");
    println!("   - Remote file systems");
}
