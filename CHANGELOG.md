# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
