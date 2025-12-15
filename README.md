# s-zip

[![Crates.io](https://img.shields.io/crates/v/s-zip.svg)](https://crates.io/crates/s-zip)
[![Documentation](https://docs.rs/s-zip/badge.svg)](https://docs.rs/s-zip)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

High-performance streaming ZIP library for Rust - Read/write ZIP files with minimal memory footprint.

## Features

- 🚀 **Streaming Read**: Extract files from ZIP archives without loading entire archive into memory
- ✍️ **Streaming Write**: Create ZIP files with on-the-fly compression, no temp files needed
- 💾 **Low Memory**: Constant memory usage regardless of ZIP file size
- ⚡ **Fast**: Optimized for performance with minimal allocations
- 🎯 **Simple API**: Easy to use, intuitive interface
- 📦 **No Dependencies**: Only uses `flate2` and `crc32fast`

## Quick Start

Add this to your `Cargo.toml`:

```toml
[dependencies]
s-zip = "0.1.1"
```

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

## Why s-zip?

Most ZIP libraries in Rust either:
- Load entire files into memory (high memory usage)
- Require temp files for compression (slow, disk I/O)
- Have complex APIs with many features you don't need

**s-zip** focuses on:
- Streaming operations (read/write on-the-fly)
- Minimal memory footprint (constant memory usage)
- Simple, easy-to-use API
- Good performance for common use cases

Perfect for:
- Processing large ZIP archives
- Creating ZIP files from streaming data
- Memory-constrained environments
- High-performance data pipelines

## Performance

- **Memory**: ~2-5 MB constant usage regardless of ZIP size
- **Speed**: Comparable to `zip` crate for common operations
- **No temp files**: Direct streaming compression saves disk I/O

## Limitations

- Only supports DEFLATE compression (most common)
- No encryption support
- Write operations require `finish()` call

## Changelog

- v0.1.1: Added ZIP64 read/write support for large archives (>4GB) and improved compatibility with external unzip tools.

## License

MIT License - see [LICENSE](LICENSE) file for details.

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

## Related Projects

- [excelstream](https://github.com/KSD-CO/excelstream) - High-performance Excel library using s-zip
- [zip](https://crates.io/crates/zip) - Full-featured ZIP library

## Author

Ton That Vu - [@KSD-CO](https://github.com/KSD-CO)
