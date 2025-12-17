//! Example demonstrating arbitrary writer usage
//!
//! This shows how to write ZIP files to any writer (not just files),
//! such as in-memory buffers, network streams, etc.
//!
//! The `.finish()` method returns the writer, allowing you to:
//! - Extract the Vec<u8> from a Cursor to get the ZIP bytes
//! - Continue using the writer for other purposes
//! - Save in-memory ZIPs to disk or send over network
//!
//! ⚠️ IMPORTANT: When using Vec<u8> or Cursor<Vec<u8>>, the ENTIRE compressed
//! ZIP file will be stored in RAM. Only use this for small archives (<100MB).
//! For large files, use StreamingZipWriter::new(path) to write to disk instead.

use s_zip::{Result, StreamingZipWriter};
use std::io::{Cursor, Seek, SeekFrom};

fn main() -> Result<()> {
    // Example 1: Write ZIP to in-memory buffer (Vec<u8>)
    // ⚠️ WARNING: Entire ZIP will be in RAM - only for small files!
    println!("Example 1: Writing ZIP to in-memory buffer...");
    let buffer = Vec::new();
    let cursor = Cursor::new(buffer);

    let mut zip = StreamingZipWriter::from_writer(cursor)?;

    zip.start_entry("hello.txt")?;
    zip.write_data(b"Hello from in-memory ZIP!")?;

    zip.start_entry("data.txt")?;
    zip.write_data(b"Some data in the second file.")?;

    // Get the cursor back after finishing
    let cursor = zip.finish()?;
    let zip_bytes = cursor.into_inner();
    println!(
        "✓ Successfully created in-memory ZIP ({} bytes)",
        zip_bytes.len()
    );

    // Example 2: Write ZIP with custom compression level
    println!("\nExample 2: Writing with custom compression level...");
    let buffer2 = Vec::new();
    let cursor2 = Cursor::new(buffer2);

    let mut zip2 = StreamingZipWriter::from_writer_with_compression(cursor2, 9)?; // Max compression

    zip2.start_entry("compressed.txt")?;
    let large_data = "Hello World! ".repeat(1000);
    zip2.write_data(large_data.as_bytes())?;

    let cursor2 = zip2.finish()?;
    let zip_bytes2 = cursor2.into_inner();
    println!(
        "✓ Successfully created highly compressed ZIP ({} bytes)",
        zip_bytes2.len()
    );

    // Example 3: Demonstrate that it works with seekable writers
    println!("\nExample 3: Using Cursor for random access...");
    let mut buffer3 = Vec::new();
    buffer3.extend_from_slice(b"PREFIX_"); // Add some prefix

    let mut cursor3 = Cursor::new(buffer3);
    cursor3.seek(SeekFrom::End(0))?; // Seek to end

    let mut zip3 = StreamingZipWriter::from_writer(cursor3)?;
    zip3.start_entry("after_prefix.txt")?;
    zip3.write_data(b"This ZIP starts after the prefix")?;

    let cursor3 = zip3.finish()?;
    let final_buffer = cursor3.into_inner();
    println!(
        "✓ Successfully created ZIP with custom position ({} bytes total, {} prefix)",
        final_buffer.len(),
        "PREFIX_".len()
    );

    // Example 4: Get the ZIP bytes and save to file
    println!("\nExample 4: Creating in-memory ZIP and saving to file...");
    let buffer4 = Vec::new();
    let cursor4 = Cursor::new(buffer4);

    let mut zip4 = StreamingZipWriter::from_writer(cursor4)?;
    zip4.start_entry("output.txt")?;
    zip4.write_data(b"This was created in memory, then saved to disk!")?;

    // Get the writer back and extract the bytes
    let cursor4 = zip4.finish()?;
    let zip_data = cursor4.into_inner();

    // Now we can do whatever we want with the bytes
    std::fs::write("/tmp/example_output.zip", &zip_data)?;
    println!(
        "✓ Successfully saved in-memory ZIP to /tmp/example_output.zip ({} bytes)",
        zip_data.len()
    );

    println!("\n✓ All examples completed successfully!");
    println!("\n⚠️  CRITICAL - Memory Usage by Writer Type:");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("  ✅ File (StreamingZipWriter::new):  ~2-5 MB constant");
    println!("  ✅ Network streams (TCP, pipes):    ~2-5 MB constant");
    println!("  ⚠️  Vec<u8>/Cursor:                  ENTIRE ZIP IN RAM!");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("\n⚠️  WARNING for Vec<u8>/Cursor:");
    println!("   While the compressor buffer is only ~2-5MB, the final ZIP");
    println!("   accumulates in the Vec. A 1GB file will use ~1GB RAM!");
    println!("\n✅ Recommended approach:");
    println!("   - Large files (>100MB):  Use StreamingZipWriter::new(path)");
    println!("   - Network transfer:      Use network streams");
    println!("   - Small temp files:      Vec<u8>/Cursor is fine (<100MB)");

    Ok(())
}
