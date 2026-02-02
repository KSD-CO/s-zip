# s-zip v0.10.0 Release Notes

## 🚀 Parallel Compression - Multi-Core Performance with Bounded Memory

Version 0.10.0 introduces **parallel compression**, enabling 2-2.4x faster compression on multi-core systems while maintaining strict memory constraints.

## New Features

### Parallel Compression API

Compress multiple files simultaneously with automatic concurrency control:

```rust
use s_zip::{AsyncStreamingZipWriter, ParallelConfig, ParallelEntry};

// Prepare files
let entries = vec![
    ParallelEntry::new("file1.txt", "path/to/file1.txt"),
    ParallelEntry::new("file2.txt", "path/to/file2.txt"),
    ParallelEntry::new("file3.txt", "path/to/file3.txt"),
];

// Choose configuration
let config = ParallelConfig::balanced(); // 4 threads, ~16MB

// Compress in parallel
let mut writer = AsyncStreamingZipWriter::new("output.zip").await?;
writer.write_entries_parallel(entries, config).await?;
writer.finish().await?;
```

### Configuration Presets

Three ready-to-use configurations for different systems:

```rust
// Low-memory systems (< 1GB)
let config = ParallelConfig::conservative(); // 2 threads, ~8MB peak

// Normal systems (2-8GB)
let config = ParallelConfig::balanced();     // 4 threads, ~16MB peak

// High-performance systems (16GB+)
let config = ParallelConfig::aggressive();   // 8 threads, ~32MB peak

// Custom configuration
let config = ParallelConfig::default()
    .with_max_concurrent(6)
    .with_compression_level(9);
```

## Performance Benchmarks

### Test 1: 100MB Files (400MB total)

| Configuration | Time  | Throughput | Speedup | Memory Delta |
|--------------|-------|------------|---------|--------------|
| Sequential   | 0.65s | 618 MB/s   | 1.00x   | +0.7 MB      |
| 2 threads    | 0.35s | 1159 MB/s  | 1.88x   | +0.6 MB      |
| 4 threads    | 0.27s | 1491 MB/s  | 2.41x   | +0.9 MB      |
| 8 threads    | 0.27s | 1496 MB/s  | 2.42x   | +0.2 MB      |

### Test 2: 500MB Files (2GB total)

| Configuration | Time  | Throughput | Speedup | Memory Delta |
|--------------|-------|------------|---------|--------------|
| Sequential   | 3.30s | 606 MB/s   | 1.00x   | +2.5 MB      |
| 2 threads    | 1.78s | 1124 MB/s  | 1.86x   | +1.2 MB      |
| 4 threads    | 1.45s | 1383 MB/s  | 2.28x   | +1.6 MB      |
| 8 threads    | 1.48s | 1354 MB/s  | 2.24x   | +0.0 MB      |

## Memory Safety Guarantees

### Key Achievements

✅ **Processing 2GB data with <6MB memory increase**
- Memory usage is independent of file size
- Only 0.3% of data size used in memory
- No memory spikes across all configurations

✅ **Predictable Memory Formula**
```
Peak Memory = max_concurrent × ~4MB
```

✅ **Streaming Architecture**
- Files read from disk on-demand (not pre-loaded)
- Compressed data written immediately (no accumulation)
- Semaphore bounds concurrent tasks
- Order preserved in output

### Memory Test Results

Test with 500MB files (2GB total data):
- Baseline: 23 MB
- Sequential: +2.5 MB (25.5 MB total)
- 4 threads: +1.6 MB (24.6 MB total)
- 8 threads: +0.0 MB (23 MB total)

**Ratio: 0.3% memory to data size** - Processing 2000 MB with 6 MB!

## Implementation Details

### Technical Architecture

1. **Semaphore-Based Concurrency**
   - Tokio semaphore limits concurrent compression tasks
   - Prevents memory spikes and CPU overload
   - Configurable from 1-16 threads

2. **Streaming I/O**
   - Files read from disk only when thread available
   - No pre-loading of all files into memory
   - Compressed output written immediately

3. **Order Preservation**
   - Results collected with original index
   - Entries written in specified order
   - Maintains deterministic output

4. **Error Handling**
   - Per-file error tracking
   - Graceful failure handling
   - Clean resource cleanup

### Supported Compression

Currently supports DEFLATE compression only. Zstd support planned for future release.

## Breaking Changes

**None!** This release is 100% backward compatible with v0.9.0.

All existing code continues to work unchanged. Parallel compression is an optional new API.

## Migration from v0.9.0

Simply update your `Cargo.toml`:

```toml
[dependencies]
s-zip = { version = "0.10", features = ["async"] }
```

Existing code works as-is. Use new parallel API only where needed for better performance.

## Examples

Run the included examples to see parallel compression in action:

```bash
# Full demo with all configurations
cargo run --example parallel_compression --features async --release

# Memory test with 100MB files (400MB total)
cargo run --example parallel_memory_test --features async --release

# Extreme test with 500MB files (2GB total)
cargo run --example parallel_500mb_test --features async --release
```

## Dependencies

Added tokio features for parallel support:
- `tokio::sync` - For Semaphore
- `tokio::task` - For spawning parallel tasks

No new external dependencies required.

## Future Work

Planned improvements for future releases:
- Zstd parallel compression support
- Progress reporting callbacks
- Adaptive concurrency based on system load
- Per-file compression method selection

## Comparison with v0.9.0

| Feature | v0.9.0 | v0.10.0 |
|---------|--------|---------|
| Sequential compression | ✅ | ✅ |
| Parallel compression | ❌ | ✅ 2-2.4x faster |
| Memory constraints | ✅ <5MB | ✅ <6MB (even with 2GB data) |
| Adaptive buffers | ✅ | ✅ |
| Reader optimization | ✅ | ✅ |
| S3 concurrent uploads | ✅ | ✅ |
| Multi-core CPU usage | ❌ | ✅ |

## Acknowledgments

Special thanks to the Rust async ecosystem:
- Tokio team for excellent async runtime
- async-compression for compression codecs
- Community feedback and testing

## Links

- **Crates.io**: https://crates.io/crates/s-zip
- **Documentation**: https://docs.rs/s-zip
- **Repository**: https://github.com/KSD-CO/s-zip
- **Benchmarks**: See BENCHMARK_RESULTS.md
- **License**: MIT

---

**Release Date**: January 29, 2026
**Rust Version**: 1.83+
**Stability**: Production Ready
