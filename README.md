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
- 🌩️ **Cloud Storage** - Direct streaming to/from S3, GCS, Azure Blob, MinIO
- 💪 **Parallel Compression** - 2-4x speedup on multi-core CPUs
- 📦 **ZIP64** - Files >4GB supported
- 🗜️ **Multiple Codecs** - DEFLATE, Zstd (3x faster compression)
- 🔌 **Seekless Streaming** - Stream ZIPs to HTTP responses, pipes, any `AsyncWrite` (no `Seek` needed)
- 📥 **Seekless Reading** - Extract straight from HTTP uploads, pipes or stdin (`ZipStreamReader`)
- 🛡️ **ZIP Bomb Protection** - Size, ratio and entry-count limits (`ExtractLimits`)
- 🗂️ **Legacy Archives** - Read ZipCrypto, bzip2, LZMA and XZ entries
- 🧰 **CLI** - `s-zip pack / unpack / list / test` (`cli` feature)

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
| Direct S3 / GCS / Azure streaming | ✅ | ❌ | ❌ |
| Read from non-seekable source | ✅ (`ZipStreamReader`) | ✅ (`read::read_zipfile_from_stream`) | ✅ |
| ZIP bomb limits | ✅ (`ExtractLimits`) | ❌ | ❌ |
| Parallel multi-file compression | ✅ | ❌ | ❌ |
| ZIP64 (>4GB) | ✅ | ✅ | ✅ |
| Zstd codec | ✅ | ✅ | ❌ |

*Feature comparison only — see [BENCHMARK_RESULTS.md](BENCHMARK_RESULTS.md) for s-zip's own throughput/memory numbers. We haven't run head-to-head benchmarks against other crates; take the table above as "does it support X", not a speed claim.*

## Quick Start

```toml
[dependencies]
s-zip = "0.15"

# With all features
s-zip = { version = "0.15", features = ["async", "encryption", "async-zstd", "cloud-all"] }
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

### Untrusted Uploads (no Seek, ZIP bomb limits)

```rust
use s_zip::{ExtractLimits, ZipStreamReader};

// `body` is any AsyncRead: an HTTP request body, a pipe, stdin...
let mut zip = ZipStreamReader::new(body);
zip.set_limits(ExtractLimits::strict());
while let Some(entry) = zip.next_entry().await? {
    let data = zip.read_entry_to_end().await?; // CRC-32 verified
    println!("{}: {} bytes", entry.name, data.len());
}
```

`StreamingZipReader` and `GenericAsyncZipReader` accept the same limits via `set_limits()`.

### Cloud Storage (S3/GCS/Azure)

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

```rust
use s_zip::{AsyncStreamingZipWriter, cloud::GCSZipWriter};
use google_cloud_storage::client::Storage;

let gcs_client = Storage::builder().build().await?;

let writer = GCSZipWriter::builder()
    .client(gcs_client)
    .bucket("my-bucket")
    .object("archive.zip")
    .build()
    .await?;

let mut zip = AsyncStreamingZipWriter::from_writer(writer);
zip.start_entry("data.json").await?;
zip.write_data(br#"{"status": "ok"}"#).await?;
zip.finish().await?;
```

```rust
use s_zip::{AsyncStreamingZipWriter, cloud::AzureZipWriter};
use azure_storage_blob::BlobClient;

let blob = BlobClient::new(blob_url, Some(credential), None)?;
let mut zip = AsyncStreamingZipWriter::from_writer(AzureZipWriter::new(blob.block_blob_client()));
zip.add_entry("data.json", br#"{"status": "ok"}"#).await?;
zip.finish().await?;
```

Readers for all three (`S3ZipReader`, `GCSZipReader`, `AzureZipReader`) plug into
`GenericAsyncZipReader` and fetch only the byte ranges they need.

### Command Line

```sh
cargo install s-zip --features cli
s-zip pack backup.zip ./data --password "$PW" --comment "nightly"
s-zip list backup.zip
s-zip test backup.zip
s-zip unpack backup.zip -d ./restore     # ZIP bomb limits on by default
```

## What's New in v0.15.0

**Safer reading, more sources and formats, and several interoperability fixes:**

- **`ExtractLimits`** — ZIP bomb protection (per-entry/total size, compression ratio, entry count),
  enforced on actual decompressed bytes. In-memory reads are now capped at 2 GiB by default.
- **`ZipStreamReader`** — read archives from non-seekable sources without a temp file.
- **Azure Blob Storage** (`cloud-azure`) and **`GCSZipReader`** — every cloud backend now has a
  writer and a reader. Cloud writers apply backpressure (bounded memory) and S3 parts really
  upload in parallel now.
- **Legacy formats** — ZipCrypto (`encryption`) and bzip2 / LZMA / XZ (`legacy-codecs`), read-only.
- **Comments** — archive and entry comments on all writers and readers.
- **`s-zip` CLI** (`cli` feature).

**Faster** — opening a 20 000-entry archive went from 592 ms to 1.4 ms (async: 2.07 s to 2.7 ms);
many-entry reads are 30–74 % faster and many-entry writes 61–78 % faster than v0.14.0.

**Interoperability fixes** (verified with Info-ZIP `unzip`/`zipinfo`):
- Non-ASCII names now carry the UTF-8 flag (no more garbled names in Windows Explorer).
- `StreamingZipWriter` entries with `unix_mode` were corrupt; fixed.
- `AsyncStreamingZipWriter` archives are extractable with `unzip` again (wrong version-needed).
- Modification times and permissions now reach the central directory.

**Breaking Changes**: `ZipEntry` and `EntryOptions` gained a `comment` field; `SZipError` is
`#[non_exhaustive]` with a new `LimitExceeded` variant.

**Migration from v0.14.x**:
```rust
let opts = EntryOptions { mtime, unix_mode, ..Default::default() };
```

**Previously (v0.14.0)**: official GCS SDK, leaner AWS TLS stack (clean `cargo audit`), upgraded
crypto crates.

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
| `cloud-azure` | Azure Blob Storage streaming |
| `cloud-all` | All cloud providers |
| `legacy-codecs` | Read bzip2 / LZMA / XZ entries |
| `cli` | `s-zip` command-line tool |

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
- **Cloud Pipelines** - Stream directly to S3/GCS/Azure without local disk
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
encryption (WinZip-compatible). For archives from untrusted sources, set
`ExtractLimits::strict()` to guard against ZIP bombs. ZipCrypto is supported for
reading legacy archives only — it is cryptographically broken and never written. CI runs `cargo audit` and `cargo deny` on every push
to catch known advisories and license/dependency issues.

Found a vulnerability? **Do not open a public issue** — see [SECURITY.md](SECURITY.md)
for private reporting instructions, scope, and known limitations.

## Non-Goals

- Not a full replacement for `zip`/`unzip` (the bundled CLI covers the common cases)
- Not optimized for small files (<1KB)
- Not focused on desktop/GUI usage

## License

MIT License - see [LICENSE](LICENSE)

## Contributing

Contributions welcome! Please feel free to submit a Pull Request.

## Author

Ton That Vu - [@KSD-CO](https://github.com/KSD-CO)
