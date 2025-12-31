//! Async vs Sync Performance Comparison on Real S3
//!
//! This benchmark compares async streaming ZIP to S3 vs traditional sync approach
//! (create in-memory then upload).
//!
//! Run with:
//! AWS_ACCESS_KEY_ID='...' AWS_SECRET_ACCESS_KEY='...' AWS_REGION='ap-southeast-1' \
//! cargo run --release --example async_vs_sync_s3 --features cloud-s3

use s_zip::{cloud::S3ZipWriter, AsyncStreamingZipWriter, StreamingZipWriter};
use std::io::Cursor;
use std::time::Instant;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("⚡ Async vs Sync S3 Upload Comparison");
    println!("=====================================\n");

    let sdk_config = aws_config::defaults(aws_config::BehaviorVersion::latest())
        .region(aws_config::Region::new("ap-southeast-1"))
        .load()
        .await;

    let s3_config = aws_sdk_s3::config::Builder::from(&sdk_config)
        .behavior_version(aws_sdk_s3::config::BehaviorVersion::latest())
        .build();
    let s3_client = aws_sdk_s3::Client::from_conf(s3_config);

    // Test data size
    let data_size_mb = 20;
    println!("Test: Creating ZIP with {}MB of data\n", data_size_mb);

    // ============================================
    // Method 1: Sync (in-memory + upload)
    // ============================================
    println!("📦 Method 1: Sync (In-Memory + Upload)");
    let start = Instant::now();

    // Create ZIP in memory
    let cursor = Cursor::new(Vec::new());
    let mut sync_writer = StreamingZipWriter::from_writer(cursor)?;

    sync_writer.start_entry("large_file.bin")?;
    let chunk = vec![0u8; 1024 * 1024]; // 1MB chunks
    for _ in 0..data_size_mb {
        sync_writer.write_data(&chunk)?;
    }

    let cursor = sync_writer.finish()?;
    let zip_bytes = cursor.into_inner();

    let create_time = start.elapsed();
    println!(
        "   ✓ ZIP created in memory: {:?} ({} bytes)",
        create_time,
        zip_bytes.len()
    );

    // Upload to S3
    let upload_start = Instant::now();
    s3_client
        .put_object()
        .bucket("lune-nonprod")
        .key("reports/bench-sync.zip")
        .body(aws_sdk_s3::primitives::ByteStream::from(zip_bytes))
        .send()
        .await?;

    let upload_time = upload_start.elapsed();
    let total_sync = start.elapsed();

    println!("   ✓ Uploaded to S3: {:?}", upload_time);
    println!("   ⏱️  Total (sync): {:?}\n", total_sync);

    // ============================================
    // Method 2: Async (streaming directly to S3)
    // ============================================
    println!("🌩️  Method 2: Async (Direct Streaming to S3)");
    let start = Instant::now();

    let writer =
        S3ZipWriter::new(s3_client.clone(), "lune-nonprod", "reports/bench-async.zip").await?;

    let mut async_writer = AsyncStreamingZipWriter::from_writer(writer);

    async_writer.start_entry("large_file.bin").await?;
    let chunk = vec![0u8; 1024 * 1024]; // 1MB chunks
    for _ in 0..data_size_mb {
        async_writer.write_data(&chunk).await?;
    }

    async_writer.finish().await?;

    let total_async = start.elapsed();
    println!("   ⏱️  Total (async): {:?}\n", total_async);

    // ============================================
    // Results
    // ============================================
    println!("📊 Results");
    println!("==========");
    println!("Sync (create + upload):  {:?}", total_sync);
    println!("Async (stream to S3):    {:?}", total_async);

    if total_async < total_sync {
        let speedup = total_sync.as_secs_f64() / total_async.as_secs_f64();
        println!("\n✅ Async is {:.2}x faster!", speedup);
    } else {
        let slowdown = total_async.as_secs_f64() / total_sync.as_secs_f64();
        println!(
            "\n📌 Sync is {:.2}x faster (expected for small files on fast network)",
            slowdown
        );
    }

    println!("\n💡 Key Differences:");
    println!("   • Sync: Loads entire ZIP into RAM before uploading");
    println!("   • Async: Streams directly to S3, constant memory (~10MB)");
    println!("\n   For large ZIPs (>100MB), async has significant advantages:");
    println!("   - Constant memory usage");
    println!("   - Start uploading while still compressing");
    println!("   - Better for cloud-native workflows");

    Ok(())
}
