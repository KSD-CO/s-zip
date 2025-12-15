//! Basic usage example for s-zip

use s_zip::{StreamingZipReader, StreamingZipWriter};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== s-zip Basic Example ===\n");

    // Create a ZIP file
    println!("Creating test.zip...");
    let mut writer = StreamingZipWriter::new("test.zip")?;

    writer.start_entry("hello.txt")?;
    writer.write_data(b"Hello, s-zip!")?;

    writer.start_entry("folder/nested.txt")?;
    writer.write_data(b"This is a nested file.")?;

    writer.start_entry("data.txt")?;
    writer.write_data(b"Line 1\nLine 2\nLine 3\n")?;

    writer.finish()?;
    println!("✓ Created test.zip\n");

    // Read the ZIP file
    println!("Reading test.zip...");
    let mut reader = StreamingZipReader::open("test.zip")?;

    println!("Entries in ZIP:");
    for entry in reader.entries() {
        println!("  - {} ({} bytes)", entry.name, entry.uncompressed_size);
    }
    println!();

    // Read specific file
    println!("Reading hello.txt:");
    let data = reader.read_entry_by_name("hello.txt")?;
    println!("  Content: {}", String::from_utf8_lossy(&data));
    println!();

    println!("Reading data.txt:");
    let data = reader.read_entry_by_name("data.txt")?;
    println!("  Content:\n{}", String::from_utf8_lossy(&data));

    println!("✓ All done!");

    Ok(())
}
