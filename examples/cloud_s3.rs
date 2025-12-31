//! S3 Cloud Storage Example
//!
//! This example demonstrates streaming ZIP files directly to AWS S3 without
//! loading the entire archive into memory.
//!
//! Run with:
//! ```bash
//! export AWS_ACCESS_KEY_ID="your-key"
//! export AWS_SECRET_ACCESS_KEY="your-secret"
//! export AWS_REGION="ap-southeast-1"
//! cargo run --example cloud_s3 --features cloud-s3
//! ```

use s_zip::cloud::S3ZipWriter;
use s_zip::AsyncStreamingZipWriter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🌩️  S3 ZIP Streaming Example");
    println!("=============================\n");

    // Load AWS config from environment
    let sdk_config = aws_config::defaults(aws_config::BehaviorVersion::latest())
        .region(aws_config::Region::new("ap-southeast-1"))
        .load()
        .await;

    // Create S3 client with explicit behavior version
    let s3_config = aws_sdk_s3::config::Builder::from(&sdk_config)
        .behavior_version(aws_sdk_s3::config::BehaviorVersion::latest())
        .build();
    let s3_client = aws_sdk_s3::Client::from_conf(s3_config);

    println!("📦 Creating S3 writer...");

    // Create S3 writer - streams directly to S3
    let writer =
        S3ZipWriter::new(s3_client, "lune-nonprod", "reports/test-s-zip-upload.zip").await?;

    println!("✏️  Creating ZIP with multiple entries...\n");

    // Create ZIP writer with S3 backend
    let mut zip = AsyncStreamingZipWriter::from_writer(writer);

    // Add first file
    println!("   [1/4] Adding README.txt");
    zip.start_entry("README.txt").await?;
    zip.write_data(
        b"# S3 ZIP Test\n\n\
        This ZIP was created by s-zip and streamed directly to S3!\n\n\
        Features:\n\
        - No local disk usage\n\
        - Constant memory (~10MB)\n\
        - S3 multipart upload\n\
        - Fast and efficient\n",
    )
    .await?;

    // Add data file
    println!("   [2/4] Adding data.json");
    zip.start_entry("data.json").await?;
    let json_data = r#"{
  "test": true,
  "timestamp": "2025-12-30T10:00:00Z",
  "source": "s-zip cloud adapter",
  "platform": "AWS S3",
  "region": "ap-southeast-1"
}"#;
    zip.write_data(json_data.as_bytes()).await?;

    // Add CSV file
    println!("   [3/4] Adding report.csv");
    zip.start_entry("reports/report.csv").await?;
    zip.write_data(b"id,name,value,timestamp\n").await?;
    for i in 1..=100 {
        let row = format!("{},Item{},{},{}\n", i, i, i * 100, i * 1000);
        zip.write_data(row.as_bytes()).await?;
    }

    // Add large file (to test multipart upload)
    println!("   [4/4] Adding large_file.bin (10MB)");
    zip.start_entry("large_file.bin").await?;

    // Generate 10MB of data in chunks
    let chunk = vec![0u8; 1024 * 1024]; // 1MB chunk
    for _ in 0..10 {
        zip.write_data(&chunk).await?;
    }

    println!("\n⬆️  Uploading to S3...");
    println!("   Calling zip.finish()...");

    // Finish - this completes the S3 multipart upload
    let writer = zip.finish().await?;
    println!("   finish() returned successfully");

    // Explicitly drop writer to ensure shutdown is called
    drop(writer);
    println!("   Writer dropped");

    println!("\n✅ Success!");
    println!("📍 Location: s3://lune-nonprod/reports/test-s-zip-upload.zip");
    println!("\n🔍 Verify with:");
    println!("   aws s3 ls s3://lune-nonprod/reports/test-s-zip-upload.zip");
    println!("   aws s3 cp s3://lune-nonprod/reports/test-s-zip-upload.zip . && unzip -l test-s-zip-upload.zip");

    Ok(())
}
