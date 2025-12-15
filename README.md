# s-zip

[![Crates.io](https://img.shields.io/crates/v/s-zip.svg)](https://crates.io/crates/s-zip)
[![Documentation](https://docs.rs/s-zip/badge.svg)](https://docs.rs/s-zip)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

# s-zip
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

- Streaming ZIP writer (no full buffering)
- Streaming ZIP reader
- Predictable memory usage
- Rust safety guarantees
- Backend-friendly API

## Non-goals

- Not a CLI replacement for zip/unzip
- Not focused on desktop or interactive usage
- Not optimized for small files convenience

## Typical Use Cases

- Generating large ZIP exports on the server
- Packaging reports or datasets
- Data pipelines and batch jobs
- Infrastructure tools that require ZIP as an intermediate format

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

## License

MIT License - see [LICENSE](LICENSE) file for details.

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.


## Author

Ton That Vu - [@KSD-CO](https://github.com/KSD-CO)
