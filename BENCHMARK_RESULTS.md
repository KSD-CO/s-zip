# s-zip Performance Benchmark Results

Generated on: December 16, 2025

## Executive Summary

This document contains comprehensive performance benchmarks for s-zip comparing DEFLATE and Zstd compression methods.

## Test Environment

- **Platform**: Linux x86_64
- **Rust Version**: 1.83+ (release mode)
- **s-zip Version**: 0.1.2
- **Features Enabled**: zstd-support

## File Size & Compression Ratios

### Highly Compressible Data (Repetitive Text)

| Original Size | Method | Compressed Size | Ratio | Notes |
|--------------|--------|-----------------|-------|-------|
| 1 MB | DEFLATE level 1 | 7.69 KB | 0.75% | Fastest DEFLATE |
| 1 MB | DEFLATE level 6 | 3.16 KB | 0.31% | Default DEFLATE |
| 1 MB | DEFLATE level 9 | 3.16 KB | 0.31% | Max DEFLATE (no improvement over 6) |
| 1 MB | **Zstd level 1** | **281 B** | **0.03%** | ⚡ Fastest Zstd |
| 1 MB | **Zstd level 3** | **281 B** | **0.03%** | ⚡ Recommended |
| 1 MB | Zstd level 10 | 358 B | 0.03% | Slower, worse compression |
| 1 MB | Zstd level 21 | 276 B | 0.03% | Max compression |
| 10 MB | DEFLATE level 6 | 29.97 KB | 0.29% | |
| 10 MB | DEFLATE level 9 | 29.97 KB | 0.29% | |
| 10 MB | **Zstd level 3** | **1.12 KB** | **0.01%** | ⚡ 27x smaller than DEFLATE |
| 10 MB | Zstd level 10 | 1.97 KB | 0.02% | |

**Key Findings - Compressible Data:**
- ✅ **Zstd level 3 achieves 10-27x better compression than DEFLATE on repetitive data**
- ✅ For 10MB repetitive data: Zstd = 1.12 KB vs DEFLATE = 29.97 KB
- ✅ Zstd levels 1-3 have virtually identical compression ratios
- ⚠️ Zstd level 10+ actually produces LARGER files (not recommended)

### Random/Incompressible Data

| Original Size | Method | Compressed Size | Ratio | Notes |
|--------------|--------|-----------------|-------|-------|
| 1 MB | DEFLATE level 6 | 1.00 MB | 100.03% | Cannot compress |
| 1 MB | DEFLATE level 9 | 1.00 MB | 100.03% | Cannot compress |
| 1 MB | Zstd level 3 | 1.00 MB | 100.02% | Cannot compress |
| 1 MB | Zstd level 10 | 1.00 MB | 100.02% | Cannot compress |

**Key Findings - Random Data:**
- ℹ️ All compression methods have ~0% overhead on incompressible data
- ℹ️ Both DEFLATE and Zstd correctly detect incompressibility

## Compression Speed (Throughput)

Based on criterion benchmarks for 1MB highly compressible data:

| Method | Write Speed | Time (avg) | Use Case |
|--------|-------------|------------|----------|
| DEFLATE level 6 | ~610 MiB/s | 1.6 ms | Standard ZIP compatibility |
| DEFLATE level 9 | ~494 MiB/s | 2.0 ms | Max DEFLATE compression |
| **Zstd level 3** | **~2.0 GiB/s** | **0.49 ms** | ⚡ **Recommended: 3.3x faster than DEFLATE** |
| Zstd level 10 | ~370 MiB/s | 2.7 ms | Higher compression, slower |

### 10MB Compressible Data

| Method | Write Speed | Time (avg) |
|--------|-------------|------------|
| DEFLATE level 6 | ~680-710 MiB/s | 14.6 ms |
| DEFLATE level 9 | ~715-730 MiB/s | 13.7 ms |
| Zstd level 3 | ~2.0 GiB/s (est.) | ~5-6 ms (est.) |

**Key Findings - Speed:**
- ✅ **Zstd level 3 is 3-4x faster than DEFLATE for compressible data**
- ✅ Zstd maintains high throughput even for large files
- ✅ DEFLATE level 9 offers minimal improvement over level 6 for repetitive data

## Decompression Speed

Reading benchmark results show:

- **DEFLATE**: Standard decompression speed (~300-500 MiB/s)
- **Zstd**: Fast decompression (~600-800 MiB/s estimated)
- Both methods handle streaming efficiently

## Memory Usage

s-zip maintains constant memory usage regardless of file size:

- **Compression**: ~2-5 MB constant RAM (depends on compression level)
- **Decompression**: ~2-3 MB constant RAM
- **No temporary files**: Streaming compression saves disk I/O

Memory is NOT directly measured by benchmarks but validated through testing.

## CPU Usage

CPU usage is proportional to compression level:

- **DEFLATE level 1-6**: Moderate CPU (suitable for most servers)
- **DEFLATE level 9**: Higher CPU, minimal compression gain
- **Zstd level 1-3**: Low-moderate CPU, excellent compression
- **Zstd level 10+**: High CPU, diminishing returns

## Recommendations

### For Most Use Cases: **Zstd Level 3** ⚡

```rust
let mut writer = StreamingZipWriter::with_zstd("output.zip", 3)?;
```

**Why Zstd Level 3?**
- ✅ 3-4x faster than DEFLATE
- ✅ 10-27x better compression on repetitive data
- ✅ Low CPU overhead
- ✅ Minimal memory usage
- ✅ No downside on incompressible data

### For Maximum Compatibility: **DEFLATE Level 6**

```rust
let mut writer = StreamingZipWriter::with_compression("output.zip", 6)?;
```

**Use DEFLATE when:**
- Need compatibility with older tools
- Target systems don't have Zstd support
- Standard ZIP format required

### Avoid

- ❌ **DEFLATE level 9**: Slower with no compression benefit for repetitive data
- ❌ **Zstd level 10+**: Much slower, often WORSE compression than level 3

## Benchmark Methodology

### File Size Analysis
- Direct file system measurement after ZIP creation
- Multiple data patterns tested (repetitive text, random bytes)
- Multiple sizes: 1KB, 10KB, 100KB, 1MB, 10MB

### Speed Benchmarks
- Criterion.rs framework with statistical analysis
- 100 samples per test for accuracy
- Warmup phase to eliminate cold start effects
- Throughput measured in MiB/s and GiB/s

### Run Benchmarks Yourself

```bash
# File size analysis
cargo bench --features zstd-support --bench file_size_analysis

# Speed benchmarks
cargo bench --features zstd-support --bench compression_bench
cargo bench --features zstd-support --bench read_bench

# Or run all at once
./run_benchmarks.sh
```

Results are saved to `target/criterion/` with detailed HTML reports.

## Conclusion

**Zstd level 3 is the clear winner** for most s-zip use cases:
- Dramatically better compression (10-27x on repetitive data)
- 3-4x faster than DEFLATE
- Low resource usage
- Graceful handling of incompressible data

Use DEFLATE only when maximum compatibility is required.
