//! Verify S3 Upload Example
//!
//! This example verifies that the ZIP files were uploaded correctly to S3
//! by listing them and checking their sizes.

use aws_sdk_s3::Client;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🔍 Verifying S3 Uploads");
    println!("======================\n");

    // Load AWS config
    let sdk_config = aws_config::defaults(aws_config::BehaviorVersion::latest())
        .region(aws_config::Region::new("ap-southeast-1"))
        .load()
        .await;

    let s3_config = aws_sdk_s3::config::Builder::from(&sdk_config)
        .behavior_version(aws_sdk_s3::config::BehaviorVersion::latest())
        .build();
    let s3_client = Client::from_conf(s3_config);

    let files = vec![
        "reports/test-s-zip-upload.zip",
        "reports/bench-sync.zip",
        "reports/bench-async.zip",
    ];

    for file in files {
        match s3_client
            .head_object()
            .bucket("lune-nonprod")
            .key(file)
            .send()
            .await
        {
            Ok(response) => {
                let size = response.content_length().unwrap_or(0);
                println!("✅ {} - {} bytes", file, size);
            }
            Err(e) => {
                println!("❌ {} - Not found or error: {}", file, e);
            }
        }
    }

    println!("\n✅ Verification complete!");
    Ok(())
}
