# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
