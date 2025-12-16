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
- **Streaming ZIP reader** with minimal memory footprint
- **ZIP64 support** for files >4GB
- **Multiple compression methods**: DEFLATE, Zstd (optional)
- **Predictable memory usage**: ~2-5 MB constant
- **High performance**: Zstd 3x faster than DEFLATE with 11-27x better compression
- **Rust safety guarantees**
- **Backend-friendly API**

## Non-goals

- Not a CLI replacement for zip/unzip
- Not focused on desktop or interactive usage
- Not optimized for small files convenience

## Typical Use Cases

- Generating large ZIP exports on the server
- Packaging reports or datasets
- Data pipelines and batch jobs
- Infrastructure tools that require ZIP as an intermediate format

## Performance Highlights

Based on comprehensive benchmarks (see [BENCHMARK_RESULTS.md](BENCHMARK_RESULTS.md)):

| Metric | DEFLATE level 6 | **Zstd level 3** | Improvement |
|--------|-----------------|------------------|-------------|
| **Speed** (1MB) | 610 MiB/s | **2.0 GiB/s** | **3.3x faster** ⚡ |
| **File Size** (1MB compressible) | 3.16 KB | **281 bytes** | **11x smaller** 🗜️ |
| **File Size** (10MB compressible) | 29.97 KB | **1.12 KB** | **27x smaller** 🗜️ |
| **Memory Usage** | 2-5 MB constant | 2-5 MB constant | Same ✓ |
| **CPU Usage** | Moderate | Low-Moderate | Better ✓ |

**Key Benefits:**
- ✅ No temp files - Direct streaming saves disk I/O
- ✅ ZIP64 support for files >4GB
- ✅ Zstd compression: faster + smaller than DEFLATE
- ✅ Constant memory usage regardless of archive size

## Quick Start

Add this to your `Cargo.toml`:

```toml
[dependencies]
s-zip = "0.2"

# Optional: Enable Zstd compression support
# s-zip = { version = "0.2", features = ["zstd-support"] }
```

### Optional Features

- **`zstd-support`**: Enables Zstd compression (method 93) for reading and writing ZIP files with better compression ratios. This adds the `zstd` crate as a dependency.

### Reading a ZIP file

```rust
use s_zip::StreamingZipReader;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut reader = StreamingZipReader::open("archive.zip")?;

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

    // Add second file
    writer.start_entry("folder/file2.txt")?;
    writer.write_data(b"Another file in a folder")?;

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

## Supported Compression Methods

| Method | Description | Default | Feature Flag | Best For |
|--------|-------------|---------|--------------|----------|
| DEFLATE (8) | Standard ZIP compression | ✓ | Always available | Text, source code, JSON, XML, CSV |
| Stored (0) | No compression | - | Always available | Already compressed files (JPG, PNG, MP4, PDF) |
| Zstd (93) | Modern compression algorithm | - | `zstd-support` | All text/data files, logs, databases |

### Compression Method Selection Guide

**Use DEFLATE (default)** when:
- ✅ Maximum compatibility required (all ZIP tools support it)
- ✅ Working with: text files, source code, JSON, XML, CSV, HTML
- ✅ Small to medium files (<100MB)
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
```

Benchmarks measure:
- **Compression speed**: Write throughput for different compression methods and levels
- **Decompression speed**: Read throughput for various compressed formats
- **Data patterns**: Highly compressible text, random data, and mixed workloads
- **File sizes**: From 1KB to 10MB to test scaling characteristics
- **Multiple entries**: Performance with 100+ files in a single archive

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

## License

MIT License - see [LICENSE](LICENSE) file for details.

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.


## Author

Ton That Vu - [@KSD-CO](https://github.com/KSD-CO)
