# s-zip

[![Crates.io](https://img.shields.io/crates/v/s-zip.svg)](https://crates.io/crates/s-zip)
[![Documentation](https://docs.rs/s-zip/badge.svg)](https://docs.rs/s-zip)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

High-performance streaming ZIP library for Rust backends. Process multi-gigabyte archives with constant ~5MB memory usage.

## Features

- 🚀 **Streaming I/O** - Constant memory regardless of archive size
- 🔐 **AES-256 Encryption** - WinZip-compatible password protection (sync + async)
- ⚡ **Async/Await** - Full Tokio support with encryption
- 🌩️ **Cloud Storage** - Direct streaming to/from S3, GCS, MinIO
- 💪 **Parallel Compression** - 2-4x speedup on multi-core CPUs
- 📦 **ZIP64** - Files >4GB supported
- 🗜️ **Multiple Codecs** - DEFLATE, Zstd (3x faster compression)

## Quick Start

```toml
[dependencies]
s-zip = "0.11"

# With all features
s-zip = { version = "0.11", features = ["async", "encryption", "async-zstd", "cloud-all"] }
```

### Basic Usage

```rust
use s_zip::{StreamingZipWriter, StreamingZipReader};

// Write
let mut writer = StreamingZipWriter::new("output.zip")?;
writer.start_entry("file.txt")?;
writer.write_data(b"Hello, World!")?;
writer.finish()?;

// Read
let mut reader = StreamingZipReader::open("output.zip")?;
let data = reader.read_entry_by_name("file.txt")?;
```

### Async with Encryption (NEW in v0.11.0!)

**Solves [Issue #1](https://github.com/KSD-CO/s-zip/issues/1)** - Full async encryption support:

```rust
use s_zip::AsyncStreamingZipWriter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut writer = AsyncStreamingZipWriter::new("encrypted.zip").await?;
    writer.set_password("my_password");
    
    writer.start_entry("secret.txt").await?;
    writer.write_data(b"Confidential data").await?;
    writer.finish().await?;
    Ok(())
}
```

### Cloud Storage (S3/GCS)

```rust
use s_zip::{AsyncStreamingZipWriter, cloud::S3ZipWriter};
use aws_sdk_s3::Client;

let config = aws_config::load_from_env().await;
let s3_client = Client::new(&config);

let writer = S3ZipWriter::builder()
    .client(s3_client)
    .bucket("my-bucket")
    .key("archive.zip")
    .build()
    .await?;

let mut zip = AsyncStreamingZipWriter::from_writer(writer);
zip.start_entry("data.json").await?;
zip.write_data(br#"{"status": "ok"}"#).await?;
zip.finish().await?;
```

## What's New in v0.11.0

🔐 **Async Encryption Support** - AsyncStreamingZipWriter now supports AES-256 encryption!
- Full encryption/decryption roundtrip (fixes critical bug from v0.10.1)
- Password-protected async ZIP creation with Tokio
- WinZip AE-2 format compliance fixed
- Compatible with 7-Zip, WinZip, WinRAR

**Breaking Changes**: None - fully backward compatible!

**Migration from v0.10.x**:
```toml
s-zip = { version = "0.11", features = ["async", "encryption"] }
```

See [CHANGELOG.md](CHANGELOG.md) for full details.

## Performance

**Single-threaded** (1MB file):
- DEFLATE: 610 MiB/s
- Zstd: 2.0 GiB/s (3.3x faster, 11x smaller)

**Parallel compression** (4 cores, 400MB total):
- Sequential: 618 MB/s
- 4 threads: 1491 MB/s (2.4x speedup)

**Memory**: ~2-5 MB constant, even processing 2GB archives.

See [BENCHMARK_RESULTS.md](BENCHMARK_RESULTS.md) for detailed benchmarks.

## Optional Features

| Feature | Description |
|---------|-------------|
| `encryption` | AES-256 encryption (sync + async) |
| `async` | Tokio async/await support |
| `async-zstd` | Async Zstd compression |
| `zstd-support` | Sync Zstd compression |
| `cloud-s3` | AWS S3 / MinIO streaming |
| `cloud-gcs` | Google Cloud Storage streaming |
| `cloud-all` | All cloud providers |

## Examples

**Encryption**:
```rust
// Sync
let mut writer = StreamingZipWriter::new("secure.zip")?;
writer.set_password("password123");
writer.start_entry("secret.txt")?;
writer.write_data(b"Confidential")?;
writer.finish()?;

// Async
let mut writer = AsyncStreamingZipWriter::new("secure.zip").await?;
writer.set_password("password123");
writer.start_entry("secret.txt").await?;
writer.write_data(b"Confidential").await?;
writer.finish().await?;
```

**Zstd Compression**:
```rust
let mut writer = StreamingZipWriter::with_zstd("output.zip", 3)?;
writer.start_entry("data.bin")?;
writer.write_data(&large_data)?;
writer.finish()?;
```

**Parallel Compression**:
```rust
use s_zip::{AsyncStreamingZipWriter, ParallelConfig, ParallelEntry};

let entries = vec![
    ParallelEntry::new("file1.txt", "path/to/file1.txt"),
    ParallelEntry::new("file2.txt", "path/to/file2.txt"),
];

let config = ParallelConfig::balanced(); // 4 threads
let mut writer = AsyncStreamingZipWriter::new("output.zip").await?;
writer.write_entries_parallel(entries, config).await?;
writer.finish().await?;
```

**In-Memory ZIP**:
```rust
let buffer = Vec::new();
let cursor = std::io::Cursor::new(buffer);
let mut writer = StreamingZipWriter::from_writer(cursor)?;

writer.start_entry("data.txt")?;
writer.write_data(b"In-memory content")?;

let cursor = writer.finish()?;
let zip_bytes = cursor.into_inner();
```

More examples in [examples/](examples/) directory.

## Use Cases

- **Web APIs** - Generate ZIPs on-demand (Axum, Actix, Rocket)
- **Cloud Pipelines** - Stream directly to S3/GCS without local disk
- **Data Exports** - Large dataset exports with encryption
- **ETL Jobs** - Batch processing with bounded memory
- **Microservices** - Streaming responses over HTTP

## Documentation

- **API Docs**: https://docs.rs/s-zip
- **Performance**: [BENCHMARK_RESULTS.md](BENCHMARK_RESULTS.md)
- **Examples**: [examples/](examples/)
- **Changelog**: [CHANGELOG.md](CHANGELOG.md)

## Non-Goals

- Not a CLI tool (use `zip`/`unzip` for that)
- Not optimized for small files (<1KB)
- Not focused on desktop/GUI usage

## License

MIT License - see [LICENSE](LICENSE)

## Contributing

Contributions welcome! Please feel free to submit a Pull Request.

## Author

Ton That Vu - [@KSD-CO](https://github.com/KSD-CO)
