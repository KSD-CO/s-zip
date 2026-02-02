# s-zip

[![Crates.io](https://img.shields.io/crates/v/s-zip.svg)](https://crates.io/crates/s-zip)
[![Documentation](https://docs.rs/s-zip/badge.svg)](https://docs.rs/s-zip)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

```text
███████╗      ███████╗██╗██████╗ 
██╔════╝      ╚══███╔╝██║██╔══██╗
███████╗█████╗  ███╔╝ ██║██████╔╝
╚════██║╚════╝ ███╔╝  ██║██╔═══╝ 
███████║      ███████╗██║██║     
╚══════╝      ╚══════╝╚═╝╚═╝     
```
`s-zip` is a streaming ZIP reader and writer designed for backend systems that need
to process large archives with minimal memory usage.

The focus is not on end-user tooling, but on providing a reliable ZIP building block
for servers, batch jobs, and data pipelines.

## Why s-zip?

Most ZIP libraries assume small files or in-memory buffers.
`s-zip` is built around streaming from day one.

- Constant memory usage
- Suitable for very large files
- Works well in containers and memory-constrained environments
- Designed for backend and data-processing workloads

## Key Features

- **Streaming ZIP writer** (no full buffering)
- **Parallel compression** 🚀 NEW! 2-2.4x faster with multi-core CPUs and bounded memory
- **AES-256 encryption** 🔐 Password-protect files with WinZip-compatible encryption
- **Async/await support** ⚡ Compatible with Tokio runtime
- **Async ZIP reader** 📖 Stream ZIPs from any source (S3, HTTP, files)
- **Cloud storage adapters** 🌩️ Stream directly to/from AWS S3, Google Cloud Storage, MinIO, and S3-compatible services
- **Arbitrary writer support** (File, Vec<u8>, network streams, etc.)
- **Streaming ZIP reader** with minimal memory footprint
- **ZIP64 support** for files >4GB
- **Multiple compression methods**: DEFLATE, Zstd (optional)
- **Predictable memory usage**: ~2-5 MB constant, even with parallel compression
- **High performance**: Parallel compression 2x faster, Zstd 3x faster than DEFLATE
- **Concurrent operations**: Create multiple ZIPs simultaneously with async
- **Rust safety guarantees**
- **Backend-friendly API**

## Non-goals

- Not a CLI replacement for zip/unzip
- Not focused on desktop or interactive usage
- Not optimized for small files convenience

## Typical Use Cases

- **Web applications** (Axum, Actix, Rocket) - Generate ZIPs on-demand
- **Cloud storage** - Stream ZIPs directly to AWS S3, Google Cloud Storage without local disk usage
- **Data exports** - Generate large ZIP exports for reports, datasets, backups
- **Data pipelines** - ETL jobs, batch processing, log aggregation
- **Infrastructure tools** - ZIP as intermediate format for deployments, artifacts
- **Real-time streaming** - WebSocket, SSE, HTTP chunked responses

## Performance Highlights

Based on comprehensive benchmarks (see [BENCHMARK_RESULTS.md](BENCHMARK_RESULTS.md)):

### Single-threaded Compression Performance

| Metric | DEFLATE level 6 | **Zstd level 3** | Improvement |
|--------|-----------------|------------------|-------------|
| **Speed** (1MB) | 610 MiB/s | **2.0 GiB/s** | **3.3x faster** ⚡ |
| **File Size** (1MB compressible) | 3.16 KB | **281 bytes** | **11x smaller** 🗜️ |
| **File Size** (10MB compressible) | 29.97 KB | **1.12 KB** | **27x smaller** 🗜️ |
| **Memory Usage** | 2-5 MB constant | 2-5 MB constant | Same ✓ |
| **CPU Usage** | Moderate | Low-Moderate | Better ✓ |

### Parallel Compression Performance (NEW in v0.10.0!)

Multi-core compression with bounded memory:

*Test: 100MB files (4 × 100MB = 400MB total)*
```
Config      | Time  | Throughput | Speedup | Memory
----------- |-------|------------|---------|--------
Sequential  | 0.65s | 618 MB/s   | 1.00x   | +0.7 MB
2 threads   | 0.35s | 1159 MB/s  | 1.88x   | +0.6 MB
4 threads   | 0.27s | 1491 MB/s  | 2.41x   | +0.9 MB
```

*Test: 500MB files (4 × 500MB = 2GB total)*
```
Config      | Time  | Throughput | Speedup | Memory
----------- |-------|------------|---------|--------
Sequential  | 3.30s | 606 MB/s   | 1.00x   | +2.5 MB
4 threads   | 1.45s | 1383 MB/s  | 2.28x   | +1.6 MB
```

**🎯 Memory Safety Achievement:**
- ✅ Processing 2GB data with <6MB memory increase
- ✅ Memory usage independent of file size (0.3% ratio)
- ✅ Memory bounded by concurrency, not data size

**Key Benefits:**
- ✅ No temp files - Direct streaming saves disk I/O
- ✅ ZIP64 support for files >4GB
- ✅ Parallel compression: 2-2.4x faster on multi-core CPUs
- ✅ Zstd compression: faster + smaller than DEFLATE
- ✅ Constant memory usage regardless of archive size

## Quick Start

Add this to your `Cargo.toml`:

```toml
[dependencies]
s-zip = "0.10"

# With AES-256 encryption support
s-zip = { version = "0.10", features = ["encryption"] }

# With async support (Tokio runtime)
s-zip = { version = "0.10", features = ["async"] }

# With AWS S3 cloud storage support
s-zip = { version = "0.10", features = ["cloud-s3"] }

# With Google Cloud Storage support
s-zip = { version = "0.10", features = ["cloud-gcs"] }

# With all cloud storage providers
s-zip = { version = "0.10", features = ["cloud-all"] }

# With async + Zstd compression + encryption
s-zip = { version = "0.10", features = ["async", "async-zstd", "encryption"] }
```

### Optional Features

| Feature | Description | Dependencies |
|---------|-------------|--------------|
| **`encryption`** | AES-256 encryption support (NEW!) | aes, ctr, hmac, sha1, pbkdf2 |
| **`async`** | Enables async/await support with Tokio runtime | tokio, async-compression |
| **`async-zstd`** | Async + Zstd compression support | async, zstd-support |
| **`zstd-support`** | Zstd compression for sync API | zstd |
| **`cloud-s3`** | AWS S3 + MinIO + S3-compatible services | async, aws-sdk-s3 |
| **`cloud-gcs`** | Google Cloud Storage adapter | async, google-cloud-storage |
| **`cloud-all`** | All cloud storage providers | cloud-s3, cloud-gcs |

**Note**: `async-zstd` includes both `async` and `zstd-support` features. Cloud features require `async`.

### Reading a ZIP file

```rust
use s_zip::StreamingZipReader;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Open with default 512KB buffer
    let mut reader = StreamingZipReader::open("archive.zip")?;
    
    // Or optimize buffer for large archives (NEW in v0.9.0!)
    let mut reader = StreamingZipReader::open_with_buffer_size(
        "large_archive.zip", 
        Some(2 * 1024 * 1024)  // 2MB buffer for better performance
    )?;

    // List all entries
    for entry in reader.entries() {
        println!("{}: {} bytes", entry.name, entry.uncompressed_size);
    }

    // Read a specific file
    let data = reader.read_entry_by_name("file.txt")?;
    println!("Content: {}", String::from_utf8_lossy(&data));

    // Or use streaming for large files
    let mut stream = reader.read_entry_streaming_by_name("large_file.bin")?;
    std::io::copy(&mut stream, &mut std::io::stdout())?;

    Ok(())
}
```

### Writing a ZIP file

```rust
use s_zip::StreamingZipWriter;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut writer = StreamingZipWriter::new("output.zip")?;

    // Add first file
    writer.start_entry("file1.txt")?;
    writer.write_data(b"Hello, World!")?;

    // Add second file with size hint for better performance (NEW in v0.9.0!)
    let file_size = std::fs::metadata("large_file.bin")?.len();
    writer.start_entry_with_hint("large_file.bin", Some(file_size))?;
    let data = std::fs::read("large_file.bin")?;
    writer.write_data(&data)?;

    // Finish and write central directory
    writer.finish()?;

    Ok(())
}
```

### Custom compression level

```rust
use s_zip::StreamingZipWriter;

let mut writer = StreamingZipWriter::with_compression("output.zip", 9)?; // Max compression
// ... add files ...
writer.finish()?;
```

### Using Zstd compression (requires `zstd-support` feature)

```rust
use s_zip::{StreamingZipWriter, CompressionMethod};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create writer with Zstd compression (level 3, range 1-21)
    let mut writer = StreamingZipWriter::with_zstd("output.zip", 3)?;
    
    // Or use the generic method API
    let mut writer = StreamingZipWriter::with_method(
        "output.zip",
        CompressionMethod::Zstd,
        3  // compression level
    )?;

    writer.start_entry("compressed.bin")?;
    writer.write_data(b"Data compressed with Zstd")?;
    writer.finish()?;

    // Reader automatically detects and decompresses Zstd entries
    let mut reader = StreamingZipReader::open("output.zip")?;
    let data = reader.read_entry_by_name("compressed.bin")?;
    
    Ok(())
}
```

**Note**: Zstd compression provides better compression ratios than DEFLATE but may have slower decompression on some systems. The reader will automatically detect and decompress Zstd-compressed entries when the `zstd-support` feature is enabled.

## Password Protection / AES-256 Encryption

`s-zip` supports **WinZip-compatible AES-256 encryption** to password-protect sensitive files in your ZIP archives. This feature is perfect for securing confidential data, credentials, or any sensitive information.

### Encryption Features

- 🔐 **AES-256-CTR encryption** - Industry-standard strongest encryption
- 🔑 **PBKDF2-HMAC-SHA1** key derivation (1000 iterations)
- ✅ **HMAC-SHA1 authentication** - Detects tampering and incorrect passwords
- 🌐 **WinZip AE-2 format** - Compatible with 7-Zip, WinZip, WinRAR, etc.
- 📁 **Per-file passwords** - Different passwords for different files in same archive
- 🚀 **Streaming encryption** - Encrypt on-the-fly with constant memory usage

### Basic Encryption Example

```rust
use s_zip::StreamingZipWriter;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut writer = StreamingZipWriter::new("encrypted.zip")?;

    // Set password for encryption (requires 'encryption' feature)
    writer.set_password("my_secure_password_123");

    // All subsequent files will be encrypted
    writer.start_entry("confidential.txt")?;
    writer.write_data(b"Top secret information")?;

    writer.start_entry("passwords.txt")?;
    writer.write_data(b"Database credentials")?;

    // Clear password to add unencrypted files
    writer.clear_password();
    writer.start_entry("readme.txt")?;
    writer.write_data(b"Public information")?;

    writer.finish()?;
    Ok(())
}
```

### Multiple Passwords in One Archive

You can use different passwords for different files in the same ZIP:

```rust
let mut writer = StreamingZipWriter::new("mixed.zip")?;

// Financial files with one password
writer.set_password("finance_2024");
writer.start_entry("salary_report.txt")?;
writer.write_data(b"Employee salaries...")?;

// Legal files with different password
writer.set_password("legal_secure");
writer.start_entry("contracts/agreement.pdf")?;
writer.write_data(b"Contract data...")?;

// Public files without password
writer.clear_password();
writer.start_entry("public_info.txt")?;
writer.write_data(b"Public data...")?;

writer.finish()?;
```

### Security Specifications

- **Encryption**: AES-256-CTR (Counter mode)
- **Key Derivation**: PBKDF2-HMAC-SHA1 with 1000 iterations
- **Salt**: 16 bytes (randomly generated per file)
- **Authentication**: HMAC-SHA1 (10-byte authentication code)
- **Format**: WinZip AE-2 (no CRC for better security)
- **Compatibility**: Works with 7-Zip, WinZip, WinRAR, Info-ZIP (with AES support)

### Security Best Practices

1. **Use strong passwords**: Minimum 12 characters with mixed case, numbers, symbols
2. **Different passwords for different security levels**: Don't reuse passwords across files
3. **Store passwords securely**: Use environment variables or secret management systems
4. **Verify integrity**: The HMAC authentication ensures files haven't been tampered with

### Performance Impact

Encryption adds overhead but maintains constant memory usage:

| File Size | Overhead | Throughput | Notes |
|-----------|----------|------------|-------|
| 1 KB | ~80x slower | 8-10 MiB/s | Dominated by key derivation (~950µs) |
| 100 KB | ~23x slower | 20-23 MiB/s | Stable encryption overhead |
| 1 MB+ | ~24-31x slower | 17-23 MiB/s | Network/disk I/O becomes bottleneck |

**Memory usage**: ✅ **No impact** - maintains constant 2-5 MB streaming architecture

**Best for**: Backend services, large files, cloud storage (where network is the bottleneck)

**Considerations**: Real-time applications with <100ms latency requirements

📊 See [ENCRYPTION_PERFORMANCE.md](ENCRYPTION_PERFORMANCE.md) for detailed benchmarks

### Decryption Support

Currently, **decryption is not yet implemented** in the reader. This is planned for future releases. For now, you can extract encrypted ZIPs using:
- 7-Zip: `7z x encrypted.zip`
- WinZip, WinRAR, or other tools that support WinZip AE-2 format

## Async/Await Support

`s-zip` supports async/await with Tokio runtime, enabling non-blocking I/O for web servers and cloud applications.

### When to Use Async?

**✅ Use Async for:**
- Web frameworks (Axum, Actix, Rocket)
- Cloud storage uploads (S3, GCS, Azure)
- Network streams (HTTP, WebSocket)
- Concurrent operations (multiple ZIPs simultaneously)
- Real-time applications

**✅ Use Sync for:**
- CLI tools and scripts
- Batch processing (single-threaded)
- Maximum throughput (CPU-bound tasks)

### Async Writer Example

```rust
use s_zip::AsyncStreamingZipWriter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut writer = AsyncStreamingZipWriter::new("output.zip").await?;

    writer.start_entry("hello.txt").await?;
    writer.write_data(b"Hello, async world!").await?;

    writer.start_entry("data.txt").await?;
    writer.write_data(b"Streaming with async/await").await?;

    writer.finish().await?;
    Ok(())
}
```

### Async with In-Memory (Cloud Upload)

Perfect for HTTP responses or cloud storage:

```rust
use s_zip::AsyncStreamingZipWriter;
use std::io::Cursor;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create ZIP in memory
    let buffer = Vec::new();
    let cursor = Cursor::new(buffer);

    let mut writer = AsyncStreamingZipWriter::from_writer(cursor);

    writer.start_entry("data.json").await?;
    writer.write_data(br#"{"status": "ok"}"#).await?;

    // Get ZIP bytes for upload
    let cursor = writer.finish().await?;
    let zip_bytes = cursor.into_inner();

    // Upload to S3, send as HTTP response, etc.
    println!("Created {} bytes", zip_bytes.len());

    Ok(())
}
```

### Streaming from Async Sources

Stream files directly without blocking:

```rust
use s_zip::AsyncStreamingZipWriter;
use tokio::fs::File;
use tokio::io::AsyncReadExt;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut writer = AsyncStreamingZipWriter::new("archive.zip").await?;

    // Stream large file without loading into memory
    writer.start_entry("large_file.bin").await?;

    let mut file = File::open("source.bin").await?;
    let mut buffer = vec![0u8; 8192];

    loop {
        let n = file.read(&mut buffer).await?;
        if n == 0 { break; }
        writer.write_data(&buffer[..n]).await?;
    }

    writer.finish().await?;
    Ok(())
}
```

### Async Reader

Read ZIP files asynchronously with minimal memory usage. Supports reading from local files, S3, HTTP, or any `AsyncRead + AsyncSeek` source.

```rust
use s_zip::AsyncStreamingZipReader;
use tokio::io::AsyncReadExt;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Open ZIP from local file with default 1MB buffer
    let mut reader = AsyncStreamingZipReader::open("archive.zip").await?;
    
    // Or optimize buffer for large archives (NEW in v0.9.0!)
    let mut reader = AsyncStreamingZipReader::open_with_buffer_size(
        "large_archive.zip",
        Some(2 * 1024 * 1024)  // 2MB buffer
    ).await?;

    // List all entries
    for entry in reader.entries() {
        println!("{}: {} bytes", entry.name, entry.uncompressed_size);
    }

    // Read a specific file into memory
    let data = reader.read_entry_by_name("file.txt").await?;
    println!("Content: {}", String::from_utf8_lossy(&data));

    // Stream large files without loading into memory
    let mut stream = reader.read_entry_streaming_by_name("large_file.bin").await?;
    let mut buffer = vec![0u8; 8192];
    
    loop {
        let n = stream.read(&mut buffer).await?;
        if n == 0 { break; }
        // Process chunk...
    }

    Ok(())
}
```

### Reading from S3 (NEW in v0.6.0!)

Read ZIP files directly from S3 without downloading to disk:

```rust
use s_zip::{GenericAsyncZipReader, cloud::S3ZipReader};
use aws_sdk_s3::Client;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Configure AWS SDK
    let config = aws_config::load_from_env().await;
    let s3_client = Client::new(&config);

    // Create S3 reader - streams directly from S3 using byte-range requests
    let s3_reader = S3ZipReader::new(
        s3_client,
        "my-bucket",
        "archives/data.zip"
    ).await?;

    // Wrap with GenericAsyncZipReader
    let mut reader = GenericAsyncZipReader::new(s3_reader).await?;

    // List entries
    for entry in reader.entries() {
        println!("📄 {}: {} bytes", entry.name, entry.uncompressed_size);
    }

    // Read specific file from S3 ZIP
    let data = reader.read_entry_by_name("report.csv").await?;
    println!("Downloaded {} bytes from S3 ZIP", data.len());

    Ok(())
}
```

**Key Benefits:**
- ✅ **No local disk** - Reads directly from S3 using byte-range GET requests
- ✅ **Constant memory** - ~5-10MB regardless of ZIP size
- ✅ **Random access** - Jump to any file without downloading entire ZIP
- ✅ **Generic API** - Works with any `AsyncRead + AsyncSeek` source (HTTP, in-memory, custom)

**Performance Note:** For small files (<50MB), downloading the entire ZIP first is faster due to network latency. For large archives or when reading only a few files, streaming from S3 provides significant memory savings.

### Reading from HTTP/Custom Sources

The generic async reader works with any `AsyncRead + AsyncSeek` source:

```rust
use s_zip::GenericAsyncZipReader;
use std::io::Cursor;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Example: In-memory ZIP (could be from HTTP response)
    let zip_bytes = download_zip_from_http().await?;
    let cursor = Cursor::new(zip_bytes);

    // Read ZIP from in-memory source
    let mut reader = GenericAsyncZipReader::new(cursor).await?;

    for entry in reader.entries() {
        println!("📦 {}", entry.name);
    }

    Ok(())
}
```

### Concurrent ZIP Creation

Create multiple ZIPs simultaneously (5x faster than sequential):

```rust
use s_zip::AsyncStreamingZipWriter;
use tokio::task::JoinSet;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut tasks = JoinSet::new();

    // Create 10 ZIPs concurrently
    for i in 0..10 {
        tasks.spawn(async move {
            let path = format!("output_{}.zip", i);
            let mut writer = AsyncStreamingZipWriter::new(&path).await?;
            writer.start_entry("data.txt").await?;
            writer.write_data(b"Concurrent creation!").await?;
            writer.finish().await?;
            Ok::<_, s_zip::SZipError>(())
        });
    }

    // Wait for all to complete
    while let Some(result) = tasks.join_next().await {
        result.unwrap()?;
    }

    println!("Created 10 ZIPs concurrently!");
    Ok(())
}
```

### Performance: Async vs Sync

| Scenario | Sync | Async | Advantage |
|----------|------|-------|-----------|
| **Local disk (5MB)** | 6.7ms | 7.1ms | ≈ Same (~6% overhead) |
| **In-memory (100KB)** | 146µs | 136µs | **Async 7% faster** |
| **Network upload (5×50KB)** | 1053ms | 211ms | **Async 5x faster** 🚀 |
| **10 concurrent operations** | 70ms | 10-15ms | **Async 4-7x faster** 🚀 |

**See [PERFORMANCE.md](PERFORMANCE.md) for detailed benchmarks.**

## Cloud Storage Streaming

Stream ZIP files directly to/from AWS S3 or Google Cloud Storage without writing to local disk. Perfect for serverless, containers, and cloud-native applications.

### AWS S3 Streaming (Write)

```rust
use s_zip::{AsyncStreamingZipWriter, cloud::S3ZipWriter};
use aws_sdk_s3::Client;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Configure AWS SDK
    let config = aws_config::load_from_env().await;
    let s3_client = Client::new(&config);

    // Create S3 writer with concurrent uploads (NEW in v0.9.0!)
    let writer = S3ZipWriter::builder()
        .client(s3_client)
        .bucket("my-bucket")
        .key("exports/archive.zip")
        .max_concurrent_uploads(8)  // 5x faster with concurrent uploads!
        .build()
        .await?;

    let mut zip = AsyncStreamingZipWriter::from_writer(writer);

    // Add files with size hints for optimal performance
    let file_size = std::fs::metadata("report.csv")?.len();
    zip.start_entry_with_hint("report.csv", Some(file_size)).await?;
    zip.write_data(b"id,name,value\n1,Alice,100\n").await?;

    zip.start_entry("data.json").await?;
    zip.write_data(br#"{"status": "success"}"#).await?;

    // Finish - completes S3 multipart upload
    zip.finish().await?;

    println!("✅ ZIP streamed to s3://my-bucket/exports/archive.zip");
    Ok(())
}
```

**Key Benefits:**
- ✅ **No local disk usage** - Streams directly to S3
- ✅ **Constant memory** - ~5-10MB regardless of ZIP size
- ✅ **S3 multipart upload** - Handles files >5GB automatically
- ✅ **Concurrent uploads** - 3-5x faster with parallel part uploads (NEW in v0.9.0!)
- ✅ **Auto-retry** - Resilient to network failures with exponential backoff
- ✅ **Configurable part size** - Default 5MB, customize up to 5GB

### AWS S3 Streaming (Read - NEW in v0.6.0!)

Read ZIP files directly from S3 without downloading:

```rust
use s_zip::{GenericAsyncZipReader, cloud::S3ZipReader};
use aws_sdk_s3::Client;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = aws_config::load_from_env().await;
    let s3_client = Client::new(&config);

    // Read directly from S3 using byte-range requests
    let s3_reader = S3ZipReader::new(s3_client, "bucket", "archive.zip").await?;
    let mut reader = GenericAsyncZipReader::new(s3_reader).await?;

    // Extract specific files without downloading entire ZIP
    let data = reader.read_entry_by_name("report.csv").await?;
    println!("Read {} bytes from S3", data.len());

    Ok(())
}
```

**Key Benefits:**
- ✅ **No local download** - Uses S3 byte-range GET requests
- ✅ **Constant memory** - ~5-10MB for any ZIP size
- ✅ **Random access** - Read any file without downloading entire archive
- ✅ **Cost effective** - Only transfer bytes you need

### Google Cloud Storage Streaming

```rust
use s_zip::{AsyncStreamingZipWriter, cloud::GCSZipWriter};
use google_cloud_storage::client::Client;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Configure GCS client
    let gcs_client = Client::default().await?;

    // Create GCS writer - streams with resumable upload
    let writer = GCSZipWriter::new(
        gcs_client,
        "my-bucket",
        "exports/archive.zip"
    ).await?;

    let mut zip = AsyncStreamingZipWriter::from_writer(writer);

    zip.start_entry("log.txt").await?;
    zip.write_data(b"Application logs...").await?;

    zip.finish().await?;

    println!("✅ ZIP streamed to gs://my-bucket/exports/archive.zip");
    Ok(())
}
```

**Key Benefits:**
- ✅ **No local disk usage** - Streams directly to GCS
- ✅ **Constant memory** - ~8-12MB regardless of ZIP size
- ✅ **Resumable upload** - 8MB chunks (256KB aligned)
- ✅ **Configurable chunk size** - Customize for performance

### Performance: Async Streaming vs Sync Upload

Real-world comparison on AWS S3 (20MB data):

| Method | Time | Memory | Description |
|--------|------|--------|-------------|
| **Sync (in-memory + upload)** | 368ms | ~20MB | Create ZIP in RAM, then upload |
| **Async (direct streaming)** | 340ms | ~10MB | Stream directly to S3 |
| **Speedup** | **1.08x faster** | **50% less memory** | ✅ Better for large files |

**For 100MB+ files:**
- 🚀 Async streaming: Constant 10MB memory
- ⚠️ Sync approach: 100MB+ memory (entire ZIP in RAM)

**When to use cloud streaming:**
- ✅ Serverless functions (Lambda, Cloud Functions)
- ✅ Containers with limited memory
- ✅ Large archives (>100MB)
- ✅ Cloud-native architectures
- ✅ ETL pipelines, data exports

### MinIO / S3-Compatible Services (NEW in v0.7.0!)

Stream ZIPs directly to MinIO, Cloudflare R2, DigitalOcean Spaces, Backblaze B2, and other S3-compatible services:

```rust
use s_zip::{AsyncStreamingZipWriter, cloud::S3ZipWriter};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Write to MinIO
    let writer = S3ZipWriter::builder()
        .endpoint_url("http://localhost:9000")
        .region("us-east-1")
        .bucket("my-bucket")
        .key("archive.zip")
        .force_path_style(true)  // Required for MinIO
        .build()
        .await?;

    let mut zip = AsyncStreamingZipWriter::from_writer(writer);
    zip.start_entry("data.txt").await?;
    zip.write_data(b"Hello MinIO!").await?;
    zip.finish().await?;

    println!("✅ ZIP streamed to MinIO");
    Ok(())
}
```

**Read from MinIO:**

```rust
use s_zip::{GenericAsyncZipReader, cloud::S3ZipReader};

let reader = S3ZipReader::builder()
    .endpoint_url("http://localhost:9000")
    .bucket("my-bucket")
    .key("archive.zip")
    .build()
    .await?;

let mut zip = GenericAsyncZipReader::new(reader).await?;
let data = zip.read_entry_by_name("data.txt").await?;
```

**Supported S3-Compatible Services:**

| Service | Endpoint Example |
|---------|------------------|
| **MinIO** | `http://localhost:9000` |
| **Cloudflare R2** | `https://<account_id>.r2.cloudflarestorage.com` |
| **DigitalOcean Spaces** | `https://<region>.digitaloceanspaces.com` |
| **Backblaze B2** | `https://s3.<region>.backblazeb2.com` |
| **Linode Object Storage** | `https://<region>.linodeobjects.com` |

### Advanced S3 Configuration

```rust
use s_zip::cloud::S3ZipWriter;

// Custom part size for large files
let writer = S3ZipWriter::builder()
    .client(s3_client)
    .bucket("my-bucket")
    .key("large-archive.zip")
    .part_size(100 * 1024 * 1024)  // 100MB parts for huge files
    .build()
    .await?;

// Or with custom endpoint for S3-compatible services
let writer = S3ZipWriter::builder()
    .endpoint_url("https://s3.us-west-001.backblazeb2.com")
    .region("us-west-001")
    .bucket("my-bucket")
    .key("archive.zip")
    .build()
    .await?;
```

**See examples:**
- [examples/cloud_s3.rs](examples/cloud_s3.rs) - S3 streaming example
- [examples/async_vs_sync_s3.rs](examples/async_vs_sync_s3.rs) - Performance comparison

### Using Arbitrary Writers (Advanced)

`s-zip` supports writing to any type that implements `Write + Seek`, not just files. This enables:

- **In-memory ZIP creation** (Vec<u8>, Cursor)
- **Network streaming** (TCP streams with buffering)
- **Custom storage backends** (S3, databases, etc.)

```rust
use s_zip::StreamingZipWriter;
use std::io::Cursor;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Write ZIP to in-memory buffer
    let buffer = Vec::new();
    let cursor = Cursor::new(buffer);

    let mut writer = StreamingZipWriter::from_writer(cursor)?;

    writer.start_entry("data.txt")?;
    writer.write_data(b"In-memory ZIP content")?;

    // finish() returns the writer, allowing you to extract the data
    let cursor = writer.finish()?;
    let zip_bytes = cursor.into_inner();

    // Now you can save to file, send over network, etc.
    std::fs::write("output.zip", &zip_bytes)?;
    println!("Created ZIP with {} bytes", zip_bytes.len());

    Ok(())
}
```

**⚠️ IMPORTANT - Memory Usage by Writer Type:**

| Writer Type | Memory Usage | Best For |
|-------------|--------------|----------|
| **File** (`StreamingZipWriter::new(path)`) | ✅ ~2-5 MB constant | Large files, production use |
| **Network streams** (TCP, pipes) | ✅ ~2-5 MB constant | Streaming over network |
| **Vec<u8>/Cursor** (`from_writer()`) | ⚠️ **ENTIRE ZIP IN RAM** | **Small archives only (<100MB)** |

**⚠️ Critical Warning for Vec<u8>/Cursor:**
When using `Vec<u8>` or `Cursor<Vec<u8>>` as the writer, the **entire compressed ZIP file will be stored in memory**. While the compressor still uses only ~2-5MB for its internal buffer, the final output accumulates in the Vec. **Only use this for small archives** or when you have sufficient RAM.

**Recommended approach for large files:**
- Use `StreamingZipWriter::new(path)` to write to disk (constant ~2-5MB memory)
- Use network streams for real-time transmission
- Reserve `Vec<u8>/Cursor` for small temporary ZIPs (<100MB)

The implementation uses a 1MB buffer threshold to periodically flush compressed data to the writer, keeping **compression memory** low (~2-5MB) for all writer types. However, in-memory writers like `Vec<u8>` will still accumulate the full output.

See [examples/arbitrary_writer.rs](examples/arbitrary_writer.rs) for more examples.

## Supported Compression Methods

| Method | Description | Default | Feature Flag | Best For |
|--------|-------------|---------|--------------|----------|
| DEFLATE (8) | Standard ZIP compression | ✓ | Always available | Text, source code, JSON, XML, CSV, XLSX |
| Stored (0) | No compression | - | Always available | Already compressed files (JPG, PNG, MP4, PDF) |
| Zstd (93) | Modern compression algorithm | - | `zstd-support` | All text/data files, logs, databases |

### Compression Method Selection Guide

**Use DEFLATE (default)** when:
- ✅ Maximum compatibility required (all ZIP tools support it)
- ✅ Working with: text files, source code, JSON, XML, CSV, HTML, XLSX
- ✅ Standard ZIP format compliance needed

**Use Zstd** when:
- ⚡ **Best performance**: 3.3x faster compression, 11-27x better compression ratio
- ✅ Working with: server logs, database dumps, repetitive data, large text files
- ✅ Backend/internal systems (don't need old tool compatibility)
- ✅ Processing large volumes of data

**Use Stored (no compression)** when:
- ✅ Files are already compressed: JPEG, PNG, GIF, MP4, MOV, PDF, ZIP, GZ
- ✅ Need fastest possible archive creation
- ✅ CPU resources are limited

## Performance Benchmarks

`s-zip` includes comprehensive benchmarks to compare compression methods:

```bash
# Run all benchmarks with Zstd support
./run_benchmarks.sh

# Or run individual benchmark suites
cargo bench --features zstd-support --bench compression_bench
cargo bench --features zstd-support --bench read_bench
cargo bench --features async --bench async_bench
```

Benchmarks measure:
- **Compression speed**: Write throughput for different compression methods and levels
- **Decompression speed**: Read throughput for various compressed formats
- **Parallel compression**: Multi-core performance with bounded memory
- **Data patterns**: Highly compressible text, random data, and mixed workloads
- **File sizes**: From 1KB to 500MB to test scaling characteristics and memory constraints
- **Multiple entries**: Performance with 100+ files in a single archive

### Parallel Compression Benchmarks

Run memory-constrained parallel compression tests:

```bash
# Test with 100MB files (400MB total)
cargo run --example parallel_memory_test --features async --release

# Test with 500MB files (2GB total) - Extreme test
cargo run --example parallel_500mb_test --features async --release

# Full demo with all configurations
cargo run --example parallel_compression --features async --release
```

**Verified Results:**
- ✅ 2-2.4x speedup on multi-core systems
- ✅ Processing 2GB data with <6MB memory increase
- ✅ Memory usage independent of file size (0.3% ratio)
- ✅ No memory spikes across all configurations
- ✅ Linear scaling up to 4 cores

Results are saved to `target/criterion/` with HTML reports showing detailed statistics, comparisons, and performance graphs.

### Quick Comparison Results

#### File Size (1MB Compressible Data)

| Method | Compressed Size | Ratio | Speed |
|--------|-----------------|-------|-------|
| DEFLATE level 6 | 3.16 KB | 0.31% | ~610 MiB/s |
| DEFLATE level 9 | 3.16 KB | 0.31% | ~494 MiB/s |
| **Zstd level 3** | **281 bytes** | **0.03%** | **~2.0 GiB/s** ⚡ |
| Zstd level 10 | 358 bytes | 0.03% | ~370 MiB/s |

**Key Insights:**
- ✅ **Zstd level 3 is 11x smaller and 3.3x faster than DEFLATE** on repetitive data
- ✅ **For 10MB data: Zstd = 1.12 KB vs DEFLATE = 29.97 KB (27x better!)**
- ✅ **Random data: All methods ~100%** (both handle incompressible data efficiently)
- ✅ **Memory: ~2-5 MB constant** regardless of file size
- ✅ **CPU: Zstd level 3 uses less CPU than DEFLATE level 9**

**💡 Recommendation:** Use **Zstd level 3** for best performance and compression. Only use DEFLATE when compatibility with older tools is required.

**📊 Full Analysis:** See [BENCHMARK_RESULTS.md](BENCHMARK_RESULTS.md) for detailed performance data including:
- Complete speed benchmarks (1KB to 10MB)
- Memory profiling
- CPU usage analysis
- Multiple compression levels comparison
- Random vs compressible data patterns

## Migration Guide

### Upgrading from v0.9.x to v0.10.0

**Zero Breaking Changes!** The v0.10.0 release is fully backward compatible.

**What's New:**
- 🚀 **Parallel Compression** - 2-4x faster with multi-core CPUs
- 🧠 **Bounded Memory** - Semaphore-based concurrency limits (conservative: 8MB, balanced: 16MB, aggressive: 32MB)
- ⚡ **Configurable Concurrency** - 1-8 threads with automatic memory guarantees
- 📊 **Predictable Performance** - Linear scaling with cores, controlled resource usage
- ✅ All existing code works unchanged

**Migration:**

```toml
[dependencies]
# Just update the version - existing code works as-is!
s-zip = "0.10"

# Or with features
s-zip = { version = "0.10", features = ["async", "cloud-s3", "encryption"] }
```

**New APIs (Optional - for multi-core performance):**

```rust
use s_zip::{AsyncStreamingZipWriter, ParallelConfig, ParallelEntry};

// Prepare files to compress
let entries = vec![
    ParallelEntry::new("file1.txt", "path/to/file1.txt"),
    ParallelEntry::new("file2.txt", "path/to/file2.txt"),
    ParallelEntry::new("file3.txt", "path/to/file3.txt"),
];

// Choose a preset based on your system
let config = ParallelConfig::balanced(); // 4 threads, ~16MB peak memory

// Compress all files in parallel (2-4x faster!)
let mut writer = AsyncStreamingZipWriter::new("output.zip").await?;
writer.write_entries_parallel(entries, config).await?;
writer.finish().await?;

// Custom configuration for specific needs
let custom = ParallelConfig::default()
    .with_max_concurrent(6)           // 6 parallel tasks
    .with_compression_level(9);       // Maximum compression

// Memory-constrained systems
let conservative = ParallelConfig::conservative(); // 2 threads, ~8MB
```

**Performance Benchmarks:**

*Test: 100MB files (4 × 100MB = 400MB total)*
```
Configuration | Time   | Throughput | Speedup | Memory Delta
-------------|--------|------------|---------|-------------
Sequential   | 0.65s  | 618 MB/s   | 1.00x   | +0.7 MB
2 threads    | 0.35s  | 1159 MB/s  | 1.88x   | +0.6 MB
4 threads    | 0.27s  | 1491 MB/s  | 2.41x   | +0.9 MB
8 threads    | 0.27s  | 1496 MB/s  | 2.42x   | +0.2 MB
```

*Test: 500MB files (4 × 500MB = 2GB total)*
```
Configuration | Time   | Throughput | Speedup | Memory Delta
-------------|--------|------------|---------|-------------
Sequential   | 3.30s  | 606 MB/s   | 1.00x   | +2.5 MB
2 threads    | 1.78s  | 1124 MB/s  | 1.86x   | +1.2 MB
4 threads    | 1.45s  | 1383 MB/s  | 2.28x   | +1.6 MB
8 threads    | 1.48s  | 1354 MB/s  | 2.24x   | +0.0 MB
```

**Key Insights:**
- ✅ Memory usage is **independent of file size** (only 0.3% of data size)
- ✅ Processing 2GB data with <6MB memory increase
- ✅ 2-2.4x speedup on multi-core systems
- ✅ Memory bounded by concurrency, not file size
- ✅ No memory spikes across all configurations

**Memory Safety Guarantee:**
- Peak memory formula: `max_concurrent × ~4MB`
- Semaphore prevents memory spikes
- Files streamed from disk, not pre-loaded
- Compressed output written immediately

### Upgrading from v0.7.x to v0.9.0

**Zero Breaking Changes!** The v0.9.0 release is fully backward compatible.

**What's New:**
- ⚡ **Adaptive Buffer Management** - 15-25% faster compression for large files
- 📖 **Reader Buffer Optimization** - Configurable buffers for optimal read performance
- 🚀 **S3 Concurrent Uploads** - 3-5x faster cloud uploads with parallel parts
- 🔄 **Auto-retry** - Resilient to network failures with exponential backoff
- ✅ All existing code works unchanged

**Migration:**

```toml
[dependencies]
# Just update the version - existing code works as-is!
s-zip = "0.9"

# Or with features
s-zip = { version = "0.9", features = ["async", "cloud-s3", "encryption"] }
```

**New APIs (Optional - for better performance):**

```rust
// Writer: Use size hints for 15-25% faster compression
let file_size = std::fs::metadata("large.bin")?.len();
writer.start_entry_with_hint("large.bin", Some(file_size))?;

// Reader: Optimize buffer for large archives
let reader = StreamingZipReader::open_with_buffer_size(
    "archive.zip", 
    Some(2 * 1024 * 1024)  // 2MB buffer
)?;

// Async reader: Same optimization
let reader = AsyncStreamingZipReader::open_with_buffer_size(
    "archive.zip",
    Some(2 * 1024 * 1024)
).await?;

// S3: Enable concurrent uploads for 3-5x faster uploads
let writer = S3ZipWriter::builder()
    .bucket("my-bucket")
    .key("archive.zip")
    .max_concurrent_uploads(8)  // 5x faster!
    .build()
    .await?;
```

### Upgrading from v0.6.x to v0.7.0

**Zero Breaking Changes!** The v0.7.0 release is fully backward compatible.

**What's New:**
- 🔐 **AES-256 encryption support** (opt-in via `encryption` feature)
- 🔑 Password-protect files with WinZip-compatible AE-2 format
- 🚀 Streaming encryption with constant memory usage (~2-5 MB)
- 📁 Per-file passwords in same archive
- ✅ All existing code works unchanged

**Migration:**

```toml
[dependencies]
# Just update the version - existing code works as-is!
s-zip = "0.9"

# Or add encryption support
s-zip = { version = "0.9", features = ["encryption"] }
```

**New APIs (Optional):**

```rust
// Enable encryption for files
let mut writer = StreamingZipWriter::new("secure.zip")?;
writer.set_password("my_password");
writer.start_entry("confidential.txt")?;
writer.write_data(b"Secret data")?;

// Mix encrypted and unencrypted files
writer.clear_password();
writer.start_entry("public.txt")?;
writer.write_data(b"Public data")?;
writer.finish()?;
```

### Upgrading from v0.5.x to v0.6.0

**Zero Breaking Changes!** The v0.6.0 release is fully backward compatible.

**What's New:**
- ✅ Generic async ZIP reader (`GenericAsyncZipReader<R>`)
- ✅ Read ZIPs from any `AsyncRead + AsyncSeek` source (S3, HTTP, in-memory, files)
- ✅ S3ZipReader for direct S3 streaming reads
- ✅ Unified architecture - eliminated duplicate code
- ✅ All existing sync and async code works unchanged

**Migration:**

```toml
[dependencies]
# Just update the version - existing code works as-is!
s-zip = "0.9"

# Or with features
s-zip = { version = "0.9", features = ["async", "cloud-s3"] }
```

**New APIs (Optional):**

```rust
// v0.5.x - Still works!
let mut reader = AsyncStreamingZipReader::open("file.zip").await?;

// v0.6.0+ - Read from S3
let s3_reader = S3ZipReader::new(client, "bucket", "key").await?;
let mut reader = GenericAsyncZipReader::new(s3_reader).await?;

// v0.6.0+ - Read from any source
let mut reader = GenericAsyncZipReader::new(custom_reader).await?;
```

### Upgrading from v0.4.x to v0.5.0

**Zero Breaking Changes!** The v0.5.0 release is fully backward compatible.

**What's New:**
- ✅ AWS S3 streaming support (opt-in via `cloud-s3` feature)
- ✅ Google Cloud Storage support (opt-in via `cloud-gcs` feature)
- ✅ Direct cloud upload without local disk usage
- ✅ Constant memory usage for cloud uploads (~5-10MB)
- ✅ All existing sync and async code works unchanged

**Migration Options:**

**Option 1: Keep Using Existing Code (No Changes)**
```toml
[dependencies]
s-zip = "0.5"  # Existing code works as-is
```

Your existing code continues to work exactly as before!

**Option 2: Add Cloud Storage Support**
```toml
[dependencies]
# AWS S3 only
s-zip = { version = "0.5", features = ["cloud-s3"] }

# Google Cloud Storage only
s-zip = { version = "0.5", features = ["cloud-gcs"] }

# Both S3 and GCS
s-zip = { version = "0.5", features = ["cloud-all"] }
```

**API Comparison:**

```rust
// Local file (v0.4.x and later)
let mut writer = AsyncStreamingZipWriter::new("output.zip").await?;
writer.start_entry("file.txt").await?;
writer.write_data(b"data").await?;
writer.finish().await?;

// AWS S3 (v0.5.0+)
let s3_writer = S3ZipWriter::new(s3_client, "bucket", "key.zip").await?;
let mut writer = AsyncStreamingZipWriter::from_writer(s3_writer);
writer.start_entry("file.txt").await?;
writer.write_data(b"data").await?;
writer.finish().await?;
```

### Upgrading from v0.3.x to v0.4.0+

All v0.3.x code is compatible with v0.9.0. Just update the version number and optionally add new features.

## Examples

Check out the [examples/](examples/) directory for complete working examples:

**Sync Examples:**
- [basic.rs](examples/basic.rs) - Simple ZIP creation
- [arbitrary_writer.rs](examples/arbitrary_writer.rs) - In-memory ZIPs
- [zstd_compression.rs](examples/zstd_compression.rs) - Zstd compression

**Performance Examples:**
- [optimized_usage.rs](examples/optimized_usage.rs) - Size hints & S3 concurrent uploads (NEW in v0.9.0!)
- [reader_optimization.rs](examples/reader_optimization.rs) - Reader buffer tuning (NEW in v0.9.0!)

**Encryption Examples:**
- [encryption_basic.rs](examples/encryption_basic.rs) - Basic password protection
- [encryption_advanced.rs](examples/encryption_advanced.rs) - Multiple passwords per archive

**Async Examples:**
- [async_basic.rs](examples/async_basic.rs) - Basic async usage
- [async_streaming.rs](examples/async_streaming.rs) - Stream files to ZIP
- [async_in_memory.rs](examples/async_in_memory.rs) - Cloud upload simulation
- [async_reader_advanced.rs](examples/async_reader_advanced.rs) - Advanced async reading
- [async_http_reader.rs](examples/async_http_reader.rs) - Read from HTTP/in-memory
- [concurrent_demo.rs](examples/concurrent_demo.rs) - Concurrent creation
- [network_simulation.rs](examples/network_simulation.rs) - Network I/O demo

**Cloud Storage Examples:**
- [cloud_s3.rs](examples/cloud_s3.rs) - AWS S3 streaming upload
- [async_vs_sync_s3.rs](examples/async_vs_sync_s3.rs) - Performance comparison (upload + download)
- [verify_s3_upload.rs](examples/verify_s3_upload.rs) - Verify S3 uploads

Run examples:
```bash
# Sync examples
cargo run --example basic
cargo run --example zstd_compression --features zstd-support

# Encryption examples
cargo run --example encryption_basic --features encryption
cargo run --example encryption_advanced --features encryption

# Async examples
cargo run --example async_basic --features async
cargo run --example concurrent_demo --features async
cargo run --example network_simulation --features async

# Cloud storage examples (requires AWS credentials)
export AWS_ACCESS_KEY_ID="..."
export AWS_SECRET_ACCESS_KEY="..."
export AWS_REGION="us-east-1"
cargo run --example cloud_s3 --features cloud-s3
cargo run --example async_vs_sync_s3 --features cloud-s3
```

## Documentation

- **API Documentation**: https://docs.rs/s-zip
- **Performance Benchmarks**: [PERFORMANCE.md](PERFORMANCE.md)
- **Benchmark Results**: [BENCHMARK_RESULTS.md](BENCHMARK_RESULTS.md)

## License

MIT License - see [LICENSE](LICENSE) file for details.

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

## Author

Ton That Vu - [@KSD-CO](https://github.com/KSD-CO)
