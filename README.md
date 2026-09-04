# s-zip

[![Crates.io](https://img.shields.io/crates/v/s-zip.svg)](https://crates.io/crates/s-zip)
[![Documentation](https://docs.rs/s-zip/badge.svg)](https://docs.rs/s-zip)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![CI](https://github.com/KSD-CO/s-zip/actions/workflows/rust.yml/badge.svg)](https://github.com/KSD-CO/s-zip/actions/workflows/rust.yml)
[![Security Audit](https://img.shields.io/badge/security-audited%20in%20CI-brightgreen.svg)](SECURITY.md)

```text
███████╗      ███████╗██╗██████╗ 
██╔════╝      ╚══███╔╝██║██╔══██╗
███████╗█████╗  ███╔╝ ██║██████╔╝
╚════██║╚════╝ ███╔╝  ██║██╔═══╝ 
███████║      ███████╗██║██║     
╚══════╝      ╚══════╝╚═╝╚═╝     
```

High-performance streaming ZIP library for Rust backends. Process multi-gigabyte archives with constant ~5MB memory usage.

## Features

- 🚀 **Streaming I/O** - Constant memory regardless of archive size
- 🔐 **AES-256 Encryption** - WinZip-compatible password protection (sync + async)
- ⚡ **Async/Await** - Full Tokio support with encryption
- 🌩️ **Cloud Storage** - Direct streaming to/from S3, GCS, MinIO
- 💪 **Parallel Compression** - 2-4x speedup on multi-core CPUs
- 📦 **ZIP64** - Files >4GB supported
- 🗜️ **Multiple Codecs** - DEFLATE, Zstd (3x faster compression)
- 🔌 **Seekless Streaming** - Stream ZIPs to HTTP responses, pipes, any `AsyncWrite` (no `Seek` needed)

## Why s-zip?

Most Rust ZIP crates (e.g. [`zip`](https://crates.io/crates/zip)) are designed around
in-memory or seekable-file access. `s-zip` is built specifically for **backend/service**
workloads: constant memory on multi-GB archives, first-class `async`/Tokio support,
AES-256 encryption end-to-end (including streaming decrypt), and direct streaming to
cloud object storage without touching local disk.

| | `s-zip` | [`zip`](https://crates.io/crates/zip) | [`async_zip`](https://crates.io/crates/async_zip) |
|---|---|---|---|
| Streaming write (no `Seek` sink) | ✅ (`SeeklessZipWriter`) | ❌ | ✅ |
| Streaming read (constant memory) | ✅ | Partial (whole entry buffered) | ✅ |
| AES-256/192/128 encryption (sync + async) | ✅ | ❌ (no built-in AES) | ❌ |
| Async/Tokio native | ✅ | ❌ | ✅ |
| Direct S3 / GCS streaming | ✅ | ❌ | ❌ |
| Parallel multi-file compression | ✅ | ❌ | ❌ |
| ZIP64 (>4GB) | ✅ | ✅ | ✅ |
| Zstd codec | ✅ | ✅ | ❌ |

*Feature comparison only — see [BENCHMARK_RESULTS.md](BENCHMARK_RESULTS.md) for s-zip's own throughput/memory numbers. We haven't run head-to-head benchmarks against other crates; take the table above as "does it support X", not a speed claim.*

## Quick Start

```toml
[dependencies]
s-zip = "0.13"

# With all features
s-zip = { version = "0.13", features = ["async", "encryption", "async-zstd", "cloud-all"] }
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

### Async with Encryption

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

## What's New in v0.13.0

**Security hardening + convenience methods** — directory/archive helpers and CI security scanning:

- **`extract_all()`** on `StreamingZipReader` and `AsyncStreamingZipReader` — extract an entire archive
  to a destination directory in one call, with zip-slip protection via `safe_path()` and full
  CRC/HMAC verification per entry.

- **`add_directory()`** on `StreamingZipWriter` and `AsyncStreamingZipWriter<W>` — recursively add a
  whole directory tree to an archive in one call, preserving relative paths.

- **`SECURITY.md`** — vulnerability disclosure policy, scope, and known limitations.

- **CI security scanning** — `cargo audit` and `cargo deny` now run on every push/PR (`rust.yml`),
  enforced via `deny.toml` (license allow-list, advisory checks, banned sources).

**Previously (v0.12.0)**: seekless streaming, AES-128/192, parallel extraction, stats API, and
convenience methods:

- **`SeeklessZipWriter`** — stream ZIPs to HTTP response bodies, pipes, or any `AsyncWrite` without `Seek`.
  Pre-compresses each entry; peak RAM ≈ compressed entry size (negligible for compressible data).

- **`read_entries_parallel()`** on `AsyncStreamingZipReader` — extract multiple named entries concurrently
  with a bounded semaphore. Missing names are silently skipped.

- **AES-128 / AES-192** — `AesStrength::Aes128` and `AesStrength::Aes192` now fully functional
  alongside the existing `Aes256`.

- **`finish_with_stats()`** — returns `ZipStats` (entry count, compressed/uncompressed bytes,
  compression ratio, encrypted flag) as a non-breaking additive method on both sync and async writers.

- **`add_entry()`** one-liner — `writer.add_entry("name", data)?` on all three writer types.

- **`entry_count()` / `bytes_written()`** accessors on all writer types for progress reporting.

- **`estimated_peak_memory_mb()` fix** — updated from stale `4 MB/task` to accurate `1 MB/task`
  after the v0.11.3 `CrcReader` streaming pipeline. Measured: **4.6 MB peak** for 8 threads × 5 MB files.

**Breaking Changes**: None.

**Migration from v0.11.x**:
```toml
s-zip = { version = "0.12", features = ["async", "encryption"] }
```



## Performance

**Single-threaded** (1MB compressible data, DEFLATE level 6):
- Write: ~1.5 ms → ~680 MB/s
- Async write: ~2.2 ms (~1.5× overhead vs sync at 1MB, converges at larger sizes)

**Parallel compression** (20 files × 5MB = 100MB):
- 2 threads: 731 MB/s, **peak RAM 3 MB**
- 4 threads: 2484 MB/s, **peak RAM 2 MB**
- 8 threads: 1434 MB/s, **peak RAM 3 MB**
- Process total peak RSS: **16 MB** (bounded regardless of file size)

**Memory**: ~16 MB process peak even compressing 100MB in parallel (8 threads).

See [CHANGELOG.md](CHANGELOG.md) for full details.

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

**Encryption** (including streaming decrypt):
```rust
// Write encrypted
let mut writer = StreamingZipWriter::new("secure.zip")?;
writer.set_password("password123");
writer.start_entry("secret.txt")?;
writer.write_data(b"Confidential")?;
writer.finish()?;

// Read encrypted — full (HMAC verified before any bytes returned)
let mut reader = StreamingZipReader::open("secure.zip")?;
reader.set_password("password123");
let data = reader.read_entry_by_name("secret.txt")?;

// Read encrypted — streaming (decrypt on-the-fly, call finish() to verify HMAC)
let entry = reader.find_entry("secret.txt").unwrap().clone();
let mut stream = reader.read_entry_streaming(&entry)?;
std::io::copy(&mut stream, &mut output)?;
// stream is dropped here; HMAC is verified in DecryptingReader::finish()
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

**Seekless Streaming** (HTTP responses, pipes — no `Seek` required):
```rust
use s_zip::SeeklessZipWriter;

// Stream ZIP directly to an Axum/Actix response body or any AsyncWrite
let mut body = Vec::new(); // replace with response body writer
let mut writer = SeeklessZipWriter::new(&mut body);
writer.add_entry("report.csv", csv_bytes).await?;
writer.add_entry("data.json", json_bytes).await?;
writer.finish().await?;
// body now contains a valid ZIP archive
```

> **Memory note**: `SeeklessZipWriter` pre-compresses each entry into a `Vec<u8>` before
> writing the local header (sizes must be known upfront without `Seek`). Peak RAM per
> `add_entry()` call is proportional to the **compressed** entry size — negligible for
> compressible data, up to ~1× entry size for incompressible data. Entries are flushed
> to the sink immediately; the writer does not buffer the entire archive.

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

## Security

`s-zip` parses untrusted ZIP input and implements AES-256/192/128 + HMAC-SHA1
encryption (WinZip-compatible). CI runs `cargo audit` and `cargo deny` on every push
to catch known advisories and license/dependency issues.

Found a vulnerability? **Do not open a public issue** — see [SECURITY.md](SECURITY.md)
for private reporting instructions, scope, and known limitations.

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
