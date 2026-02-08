//! Encryption roundtrip example - create and read encrypted ZIP
//!
//! This example demonstrates the full roundtrip: create an encrypted ZIP,
//! then read it back and decrypt the contents.
//!
//! Run with: cargo run --example encryption_roundtrip --features encryption

use s_zip::{Result, StreamingZipReader, StreamingZipWriter};

fn main() -> Result<()> {
    println!("🔐 Encryption Roundtrip Test\n");

    let zip_path = "roundtrip_test.zip";
    let password = "test_password_12345";

    // Step 1: Create encrypted ZIP
    println!("Step 1: Creating encrypted ZIP...");
    {
        let mut writer = StreamingZipWriter::new(zip_path)?;
        writer.set_password(password);

        writer.start_entry("secret1.txt")?;
        writer.write_data(b"This is the first secret message!")?;

        writer.start_entry("secret2.txt")?;
        writer.write_data(b"This is the second secret message with more data!")?;

        writer.start_entry("folder/secret3.txt")?;
        writer.write_data(b"Nested secret file in a folder!")?;

        writer.finish()?;
    }
    println!("✅ Created encrypted ZIP with 3 files\n");

    // Step 2: Read encrypted ZIP
    println!("Step 2: Reading encrypted ZIP...");
    let mut reader = StreamingZipReader::open(zip_path)?;
    reader.set_password(password);

    println!("📁 Archive contents:");
    for entry in reader.entries() {
        println!("  - {}: {} bytes", entry.name, entry.uncompressed_size);
        #[cfg(feature = "encryption")]
        if entry.is_encrypted {
            println!("    🔒 Encrypted");
        }
    }

    println!("\nStep 3: Decrypting and verifying contents...");

    // Read and verify first file
    let data1 = reader.read_entry_by_name("secret1.txt")?;
    let text1 = String::from_utf8_lossy(&data1);
    println!("✅ secret1.txt: \"{}\"", text1);
    assert_eq!(text1, "This is the first secret message!");

    // Read and verify second file
    let data2 = reader.read_entry_by_name("secret2.txt")?;
    let text2 = String::from_utf8_lossy(&data2);
    println!("✅ secret2.txt: \"{}\"", text2);
    assert_eq!(text2, "This is the second secret message with more data!");

    // Read and verify third file
    let data3 = reader.read_entry_by_name("folder/secret3.txt")?;
    let text3 = String::from_utf8_lossy(&data3);
    println!("✅ folder/secret3.txt: \"{}\"", text3);
    assert_eq!(data3, b"Nested secret file in a folder!");

    println!("\n🎉 Roundtrip test PASSED!");
    println!("   - Encryption: ✅");
    println!("   - Decryption: ✅");
    println!("   - Password validation: ✅");
    println!("   - HMAC authentication: ✅");
    println!("   - Data integrity: ✅");

    // Test wrong password
    println!("\nStep 4: Testing wrong password...");
    let mut reader_wrong = StreamingZipReader::open(zip_path)?;
    reader_wrong.set_password("wrong_password");

    match reader_wrong.read_entry_by_name("secret1.txt") {
        Ok(_) => {
            println!("❌ ERROR: Should have failed with wrong password!");
            std::process::exit(1);
        }
        Err(e) => {
            println!("✅ Correctly rejected wrong password: {}", e);
        }
    }

    // Test without password
    println!("\nStep 5: Testing without password...");
    let mut reader_no_pw = StreamingZipReader::open(zip_path)?;

    match reader_no_pw.read_entry_by_name("secret1.txt") {
        Ok(_) => {
            println!("❌ ERROR: Should have failed without password!");
            std::process::exit(1);
        }
        Err(e) => {
            println!("✅ Correctly rejected missing password: {}", e);
        }
    }

    println!("\n✨ All security checks PASSED!");

    // Cleanup
    std::fs::remove_file(zip_path)?;
    println!("\n🧹 Cleaned up test file");

    Ok(())
}
