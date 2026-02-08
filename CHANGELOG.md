# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.10.1] - 2026-02-08

### Fixed 🐛

- **Compiler Warnings** - Eliminated all compiler warnings for cleaner builds
  - Fixed unused variable warnings with proper `#[cfg_attr]` annotations
  - Fixed unused `mut` warnings in encryption code paths
  - Added `#[allow(dead_code)]` for fields kept for future API extensions
- **Code Quality**
  - Removed debug example files (encryption_debug, test_pbkdf2, etc.)
  - Improved code organization and clarity
  - Better conditional compilation for optional features

### Added ✅

- **Unit Tests** - Increased test coverage with 5 new tests
  - `test_basic_write_read_roundtrip` - In-memory ZIP creation and verification
  - `test_compression_method_to_zip_method` - Enum value mappings
  - `test_empty_entry_name` - Edge case handling
  - `test_multiple_small_entries` - Multi-file archives
  - `test_error_display` - Error formatting
  - `test_aes_strength` - AES-256 parameter verification (when encryption enabled)
- **Performance Test** - Added `examples/perf_compare.rs` for quick performance verification
  - Small files: 14,317 files/sec
  - Medium files: 194 MB/sec
  - Compression ratio: 382x on highly compressible data
- **Encryption Example** - Added `examples/encryption_roundtrip.rs` for full encrypt/decrypt demo

### Performance ⚡

- **Zero Performance Regression** - All optimizations intact
  - Streaming architecture preserved (~2-5 MB constant memory)
  - Compression speed unchanged (194 MB/sec for 1MB files)
  - Excellent compression ratios (382x on compressible data)

### Documentation 📚

- Updated CHANGELOG with detailed v0.10.1 changes
- Added performance comparison test results

### Dependencies

- Added `getrandom = "0.2"` to encryption feature (for cryptographic salt generation)

## [0.10.0] - 2026-01-29

### Added 🚀

- **Parallel Compression** (2-4x performance improvement for multi-core systems)
  - `write_entries_parallel(entries, config)` - Compress multiple files simultaneously
  - Semaphore-based concurrency control for bounded memory usage
  - Three preset configurations:
    - `ParallelConfig::conservative()` - 2 threads, ~8MB peak memory
    - `ParallelConfig::balanced()` - 4 threads, ~16MB peak memory
    - `ParallelConfig::aggressive()` - 8 threads, ~32MB peak memory
  - Custom configuration with `with_max_concurrent(n)` and `with_compression_level(level)`
  - Memory safety guarantee: Peak memory = `max_concurrent × ~4MB`
  - Zero breaking changes - backward compatible with existing code
- **New API Types**
  - `ParallelConfig` - Configuration for parallel compression
  - `ParallelEntry` - File entry for parallel processing
  - `estimated_peak_memory_mb()` - Get estimated memory usage
- **New Examples**
  - `examples/parallel_compression.rs` - Full demo with benchmarks
  - `examples/memory_test_parallel.rs` - Memory profiling with different configs

### Performance ⚡

**Parallel Compression Speedup (vs sequential):**
- 2 threads (conservative): 1.5-1.9x faster, ~8MB memory
- 4 threads (balanced): 2.3-2.8x faster, ~16MB memory
- 8 threads (aggressive): 2.2-3.5x faster, ~32MB memory

**Real-world Benchmark Results:**

*Test 1: 100MB files (4 files × 100MB = 400MB total)*
```
Configuration | Time   | Throughput | Speedup | Peak Memory Delta
-------------|--------|------------|---------|------------------
Sequential   | 0.65s  | 618 MB/s   | 1.00x   | +0.7 MB
2 threads    | 0.35s  | 1159 MB/s  | 1.88x   | +0.6 MB
4 threads    | 0.27s  | 1491 MB/s  | 2.41x   | +0.9 MB
8 threads    | 0.27s  | 1496 MB/s  | 2.42x   | +0.2 MB
```

*Test 2: 500MB files (4 files × 500MB = 2GB total)*
```
Configuration | Time   | Throughput | Speedup | Peak Memory Delta
-------------|--------|------------|---------|------------------
Sequential   | 3.30s  | 606 MB/s   | 1.00x   | +2.5 MB
2 threads    | 1.78s  | 1124 MB/s  | 1.86x   | +1.2 MB
4 threads    | 1.45s  | 1383 MB/s  | 2.28x   | +1.6 MB
8 threads    | 1.48s  | 1354 MB/s  | 2.24x   | +0.0 MB
```

**Memory Safety Verified - EXTREME TEST:**
- ✅ Processing 2GB data with <6MB peak memory increase
- ✅ Memory usage INDEPENDENT of file size (0.3% ratio)
- ✅ Memory bounded by concurrency only, not data size
- ✅ No memory spikes observed across all configurations
- ✅ Streaming architecture maintains constant memory

### Implementation Details 🔧

- Uses tokio semaphore to limit concurrent compression tasks
- Files streamed from disk on-demand (not pre-loaded)
- Compressed data written immediately (no accumulation)
- Order preserved - entries written in original sequence
- Only DEFLATE supported (Zstd in future version)

### Dependencies 📦

- Added `tokio::sync` features for semaphore support
- Added `tokio::task` features for parallel task spawning
- No new external dependencies

## [0.9.0] - 2026-01-29

### Added ⚡

- **Adaptive Buffer Management** (15-25% performance improvement for writers)
  - Smart buffer allocation based on file size hints
  - `start_entry_with_hint(name, size_hint)` - New optional method
  - Automatic optimization: tiny files (8KB) → large files (512KB)
  - Adaptive flush thresholds: 256KB → 8MB based on file size
  - Zero breaking changes - backward compatible with existing code
- **Reader Buffer Optimization** (Configurable read performance)
  - `open_with_buffer_size(path, buffer_size)` for sync reader
  - `open_with_buffer_size(path, buffer_size)` for async reader
  - `new_with_buffer_size(reader, buffer_size)` for generic async reader
  - Default buffers: 512KB (sync), 1MB (async)
  - Recommended: 64KB-2MB based on archive size
- **Concurrent S3 Multipart Upload** (3-5x faster cloud operations)
  - Parallel part uploads with configurable concurrency (default: 4)
  - `max_concurrent_uploads(n)` - Configure 1-20 concurrent uploads
  - Automatic retry with exponential backoff (100ms → 400ms)
  - Resilient to transient network failures
  - Example: `examples/optimized_usage.rs`
- **New API Methods**
  - Writers: `start_entry_with_hint(name, size_hint)`
  - Sync Reader: `StreamingZipReader::open_with_buffer_size(path, buffer_size)`
  - Async Reader: `AsyncStreamingZipReader::open_with_buffer_size(path, buffer_size)`
  - Generic Async: `GenericAsyncZipReader::new_with_buffer_size(reader, buffer_size)`
  - S3: `S3ZipWriterBuilder::max_concurrent_uploads(n)`

### Performance 🚀

**Compression Performance (with size hints):**
- Small files (<100KB): Minimal overhead, optimal memory
- Large files (1-10MB): +15-25% throughput
- Very large files (>10MB): +20-25% throughput
- Memory usage: Still constant 2-6MB (unchanged)

**S3 Upload Performance:**
- 4 concurrent uploads (default): ~3x faster
- 8 concurrent uploads (aggressive): ~5x faster
- Automatic retry reduces failures by 90%+

**Buffer Size Optimization:**
| File Size      | Initial Cap | Flush Threshold | Performance |
|----------------|-------------|-----------------|-------------|
| <10KB (tiny)   | 8KB         | 256KB           | Minimal RAM |
| <100KB (small) | 32KB        | 512KB           | Optimal     |
| <1MB (medium)  | 128KB       | 2MB             | +10-15%     |
| 1-10MB (large) | 256KB       | 4MB             | +15-20%     |
| >10MB (huge)   | 512KB       | 8MB             | +20-25%     |

### Changed 🔧

- Internal buffer implementation enhanced with adaptive sizing
- S3 upload worker now uses concurrent task pool
- Improved error messages for S3 upload failures

### Documentation 📚

- New example: `examples/optimized_usage.rs` - Showcases writer optimizations
- New example: `examples/reader_optimization.rs` - Demonstrates reader buffer tuning
- New example: `examples/memory_test_100mb.rs` - Proves constant memory with 100MB files
- New example: `examples/memory_test_1gb.rs` - Extreme test with 1GB files
- Updated `OPTIMIZATION_PROPOSAL.md` - Detailed implementation notes
- Updated `IMPLEMENTATION_SUMMARY.md` - Complete implementation documentation
- Performance comparison tables in CHANGELOG
- API documentation for all new methods

### Tests ✅

- All existing tests pass
- Zero breaking changes
- Backward compatible - existing code works without modification

### Migration Notes

**Optional - Enable optimizations in existing code:**

```rust
// Old code (still works, gets default optimizations):
writer.start_entry("file.txt").await?;

// New code (optimal for large files):
let file_size = std::fs::metadata("file.txt")?.len();
writer.start_entry_with_hint("file.txt", Some(file_size)).await?;

// S3 optimization (optional):
let writer = S3ZipWriter::builder()
    .bucket("my-bucket")
    .key("archive.zip")
    .max_concurrent_uploads(8)  // 5x faster uploads!
    .build()
    .await?;
```

**No changes required** - your existing code automatically benefits from improved defaults!

## [0.6.0] - 2026-01-07

### Added 📖

- **Generic Async ZIP Reader** (`GenericAsyncZipReader<R>`)
  - Read ZIP files from any `AsyncRead + AsyncSeek` source
  - Supports local files, S3, HTTP, in-memory, and custom readers
  - Unified architecture replacing duplicate code
- **S3ZipReader** - Direct S3 ZIP streaming reads
  - Uses S3 byte-range GET requests for efficient random access
  - Read specific files without downloading entire ZIP
  - Constant memory usage (~5-10MB) regardless of ZIP size
  - Example: `examples/async_http_reader.rs`, `examples/async_reader_advanced.rs`
- **New Examples**
  - `async_reader_advanced.rs` - Advanced async reading features
  - `async_http_reader.rs` - Reading from HTTP/in-memory sources
  - Updated `async_vs_sync_s3.rs` - Now includes download/read performance testing

### Changed 🔧

- **Unified Async Reader Architecture**
  - `AsyncStreamingZipReader` is now a type alias: `GenericAsyncZipReader<File>`
  - Merged `async_reader_generic.rs` into `async_reader.rs` (eliminated 536 lines of duplicate code)
  - All reader methods now generic over any `AsyncRead + AsyncSeek` source
- **Performance Improvements**
  - Better code organization and maintainability
  - Single source of truth for async reading logic

### Documentation 📚

- Updated README with new async reader features
- Added S3 reading examples and performance notes
- Updated migration guide for v0.5.x → v0.6.0
- Added performance comparison: sync download vs async streaming

### Tests ✅

- All existing tests pass (29/29)
- Zero breaking changes - full backward compatibility
- All examples work correctly (11/11)

## [0.5.1] - 2026-01-02

### Fixed
- **Compilation error** with specific feature combinations (e.g., `cloud-s3` without `zstd-support`)
  - Fixed pattern matching for `CompressionMethod::Zstd` in async writer
  - Added proper `#[cfg]` guards for conditional Zstd support
- **Unused import warnings** in benchmark files when building without all features

### Improved
- **CI/CD robustness**: Added feature combination testing to `make ci`
  - Now tests `async`, `cloud-s3`, and `cloud-gcs` features individually
  - Prevents feature-specific compilation errors from slipping through
- **Example configuration**: Added `required-features` to cloud storage examples in `Cargo.toml`
  - `cloud_s3.rs`, `async_vs_sync_s3.rs`, `verify_s3_upload.rs` now require `cloud-s3` feature

### Documentation
- Removed outdated version markers ("NEW in v0.3.0", "NEW in v0.4.0") from README
- Updated README to reflect current v0.5.x status

## [0.5.0] - 2026-01-02

### Added 🌩️

- **AWS S3 cloud storage streaming** (NEW!)
  - `S3ZipWriter`: Stream ZIP files directly to S3 without local disk
  - S3 multipart upload support (5MB minimum part size, configurable)
  - Constant memory usage (~5-10MB regardless of ZIP size)
  - Background task pattern with async channels for non-blocking uploads
  - Builder API for custom configuration (`S3ZipWriter::builder()`)

- **Google Cloud Storage streaming** (NEW!)
  - `GCSZipWriter`: Stream ZIP files directly to GCS without local disk
  - Resumable upload support (8MB chunks, 256KB aligned)
  - Constant memory usage (~8-12MB regardless of ZIP size)
  - Configurable chunk size for performance tuning
  - Builder API for custom configuration (`GCSZipWriter::builder()`)

- **New feature flags**
  - `cloud-s3`: Enables AWS S3 streaming adapter
  - `cloud-gcs`: Enables Google Cloud Storage adapter
  - `cloud-all`: Enables all cloud storage providers

- **Cloud storage examples**
  - `cloud_s3.rs`: AWS S3 streaming upload example
  - `async_vs_sync_s3.rs`: Performance comparison (sync vs async streaming)
  - `verify_s3_upload.rs`: Verify uploaded files on S3

- **Performance metrics** for cloud streaming (tested with real S3, 20MB data):
  - Sync (in-memory + upload): 368ms, ~20MB memory
  - Async (direct streaming): 340ms, ~10MB memory
  - **1.08x faster, 50% less memory** ✅

### Changed

- Enhanced README with comprehensive cloud storage documentation
  - AWS S3 streaming examples
  - Google Cloud Storage streaming examples
  - Performance comparison tables
  - When to use cloud streaming guide
  - Advanced S3 configuration examples

- Updated migration guide for v0.5.0
  - Zero breaking changes
  - Backward compatible with v0.4.x and v0.3.x
  - Cloud features are opt-in

### Dependencies

**New optional dependencies:**
- `aws-config ^1.5` (AWS SDK configuration)
- `aws-sdk-s3 ^1.80` (AWS S3 client)
- `google-cloud-storage ^0.22` (GCS client)
- `google-cloud-auth ^0.17` (GCS authentication)

All dependencies include `behavior-version-latest` feature to ensure compatibility.

### Performance

**Cloud Streaming Benefits:**
- ✅ No local disk usage - streams directly to cloud
- ✅ Constant memory usage regardless of file size
- ✅ Better for serverless/Lambda (memory-constrained environments)
- ✅ Faster for large files (>100MB) - pipelining compression + upload

**When to use:**
- Serverless functions (AWS Lambda, Cloud Functions)
- Containers with limited memory
- Large archives (>100MB)
- Cloud-native architectures
- ETL pipelines and data exports

### Backward Compatibility ✅

- **Zero breaking changes!**
- All existing sync and async code works unchanged
- Cloud storage support is opt-in via feature flags
- Full API compatibility with v0.4.x

## [0.4.0] - 2024-12-30

### Added ⚡

- **Async/await support** with Tokio runtime
  - `AsyncStreamingZipWriter` for non-blocking ZIP creation
  - Compatible with Axum, Actix, Rocket and other async frameworks
  - Concurrent ZIP creation (4-7x faster for parallel operations)
  - Network stream support (HTTP, WebSocket, cloud storage)

- **New feature flags**
  - `async`: Enables async/await support
  - `async-zstd`: Enables both async and Zstd compression

- **Performance improvements**
  - In-memory operations 7% faster with async
  - Network/cloud operations 5x faster with concurrent async
  - Minimal overhead (~6%) for local disk I/O

- **New examples**
  - `async_basic.rs`: Basic async ZIP creation
  - `async_streaming.rs`: Stream files to ZIP
  - `async_in_memory.rs`: Cloud upload simulation
  - `concurrent_demo.rs`: Concurrent ZIP creation
  - `network_simulation.rs`: Network I/O performance demo
  - `perf_test.rs`: Quick performance comparison

- **Comprehensive documentation**
  - [PERFORMANCE.md](PERFORMANCE.md): Async vs Sync benchmarks
  - Migration guide in README
  - API comparison examples

- **Benchmarks**
  - `async_bench.rs`: Criterion benchmarks for async performance
  - Memory usage profiling with `/usr/bin/time`
  - Throughput comparisons across different scenarios

### Changed

- Updated package description to mention async support
- Enhanced README with async examples and migration guide
- Performance section now includes async metrics

### Backward Compatibility ✅

- **Zero breaking changes!**
- All existing sync code works unchanged
- Async support is opt-in via feature flags
- Full API compatibility with v0.3.x

### Performance Metrics

| Operation | Sync | Async | Advantage |
|-----------|------|-------|-----------|
| Local disk (5MB) | 6.7ms | 7.1ms | ~6% overhead (acceptable) |
| In-memory (100KB) | 146µs | 136µs | Async 7% faster |
| Network (5×50KB) | 1053ms | 211ms | **Async 5x faster** |
| 10 concurrent ops | 70ms | 10-15ms | **Async 4-7x faster** |

### Dependencies

**New optional dependencies:**
- `tokio ^1.35` (async runtime)
- `async-compression ^0.4` (async compression)
- `futures-util ^0.3` (async utilities)
- `pin-project-lite ^0.2` (pin projection)

## [0.3.0] - 2025-12-17

### Added
- **🎉 Arbitrary writer support**: `StreamingZipWriter` is now generic over `W: Write + Seek`
  - New API: `StreamingZipWriter::from_writer(writer)` - Create writer from any `Write + Seek` type
  - New API: `StreamingZipWriter::from_writer_with_compression(writer, level)` - With custom compression level
  - New API: `StreamingZipWriter::from_writer_with_method(writer, method, level)` - With compression method
  - Supports writing to: File, Vec<u8>, Cursor, network streams (TCP), custom storage backends (S3, databases, etc.)
- Example: `examples/arbitrary_writer.rs` demonstrating arbitrary writer usage with memory warnings
- Comprehensive documentation about memory usage for different writer types

### Changed
- **🚀 Major performance improvements**: 10-40% faster compression across all workloads
  - 10MB Zstd level 3: **+38% throughput** (now 3.99 GiB/s, previously 2.99 GiB/s)
  - 10MB DEFLATE level 6: **+22-26% throughput** (now 817 MiB/s)
  - 10MB DEFLATE level 9: **+27-35% throughput** (now 804 MiB/s)
  - 100 entries Zstd: **+18-40% throughput** (now 291 MiB/s)
  - 1MB Random DEFLATE: **+29-40% throughput** (now 41 MiB/s)
- Internal architecture restructured to eliminate `File::try_clone()` dependency
- Compression now uses smart buffering with 1MB threshold for optimal memory management
- All existing file-based APIs remain unchanged and fully backward compatible

### Documentation
- **⚠️ Added critical memory usage warnings** for Vec<u8>/Cursor writers:
  - Vec<u8>/Cursor stores **entire ZIP in RAM** - only use for small archives (<100MB)
  - File writers maintain constant ~2-5 MB memory usage (recommended for large files)
  - Network streams maintain constant ~2-5 MB memory usage
- Updated README with detailed memory usage comparison table
- Enhanced example with clear warnings and best practices
- Added performance benchmark results showing improvements

### Note
⚠️ **IMPORTANT Memory Usage Warning**:

When using `Vec<u8>` or `Cursor<Vec<u8>>` as the writer, the **entire compressed ZIP file will be stored in memory**. While the compressor still uses only ~2-5MB for its internal buffer, the final output accumulates in the Vec.

**Memory Usage by Writer Type:**
- ✅ **File** (`StreamingZipWriter::new(path)`): ~2-5 MB constant ← **Recommended for large files**
- ✅ **Network streams** (TCP, pipes): ~2-5 MB constant
- ⚠️ **Vec<u8>/Cursor**: ENTIRE ZIP IN RAM ← **Only for small archives (<100MB)**

**Recommended approach:**
- Use `StreamingZipWriter::new(path)` for large files (>100MB)
- Use network streams for real-time transmission
- Reserve `Vec<u8>/Cursor` for small temporary ZIPs only

## [0.2.0] - 2025-12-16

### Added
- **Performance benchmarks** using criterion.rs framework
- Comprehensive benchmark suite comparing DEFLATE vs Zstd compression
- `BENCHMARK_RESULTS.md` with detailed performance analysis
- File size analysis tool (`benches/file_size_analysis.rs`)
- Compression and decompression speed benchmarks
- Memory usage documentation
- CPU usage analysis

### Performance
- Documented that **Zstd level 3 is 3.3x faster than DEFLATE level 6**
- Documented that **Zstd achieves 11-27x better compression on repetitive data**
- Confirmed constant memory usage (~2-5 MB) regardless of file size
- Verified efficient handling of incompressible data

### Documentation
- Enhanced README with performance comparison table
- Added benchmark running instructions
- Updated feature highlights with performance metrics
- Added recommendation section for compression method selection

## [0.1.2] - 2025-12-15

### Added
- **Optional Zstd compression support** via `zstd-support` feature flag
- `CompressionMethod` enum for flexible compression selection
- `with_method()` API for generic compression method specification
- `with_zstd()` convenience method for Zstd compression
- Trait-based compression architecture (`CompressorWrite` trait)
- Automatic Zstd decompression in reader (when feature enabled)
- Zstd compression example (`examples/zstd_compression.rs`)
- Integration tests for Zstd roundtrip compatibility

### Changed
- Refactored writer to support multiple compression methods
- Enhanced `StreamingZipWriter` with flexible compression API

### Documentation
- Added Zstd usage examples to README
- Documented compression method comparison table
- Added feature flag documentation

## [0.1.1] - 2025-12-15

### Added
- **Full ZIP64 support** for files >4GB
- ZIP64 End of Central Directory parsing in reader
- ZIP64 extra field handling (0x0001) for large file metadata
- Automatic ZIP64 format detection and writing
- ZIP64 locator structure support
- Unit tests for ZIP64 functionality
- System `unzip` compatibility tests

### Fixed
- Various clippy warnings (collapsible-if, unused assignments, etc.)
- Code formatting consistency issues

### Documentation
- Updated README to reflect ZIP64 support
- Added ZIP64 technical notes

## [0.1.0] - 2024-12-15

### Added
- Initial release
- Streaming ZIP reader with minimal memory footprint
- Streaming ZIP writer with on-the-fly compression
- Support for DEFLATE and STORE compression methods
- Simple, intuitive API
- Examples and documentation
