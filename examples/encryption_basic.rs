//! Basic AES-256 encryption example
//!
//! This example demonstrates how to create a password-protected ZIP file
//! using AES-256 encryption.
//!
//! Run with: cargo run --example encryption_basic --features encryption

use s_zip::{Result, StreamingZipWriter};

fn main() -> Result<()> {
    println!("Creating encrypted ZIP file...\n");

    // Create a new ZIP writer
    let mut writer = StreamingZipWriter::new("encrypted_example.zip")?;

    // Set password for AES-256 encryption
    writer.set_password("my_super_secret_password_123");

    // Add first encrypted file
    println!("Adding confidential_data.txt (encrypted)");
    writer.start_entry("confidential_data.txt")?;
    writer.write_data(b"This is highly confidential information!")?;

    // Add second encrypted file
    println!("Adding secrets.txt (encrypted)");
    writer.start_entry("secrets.txt")?;
    writer.write_data(b"Secret key: 1234-5678-90AB-CDEF")?;

    // Clear password for subsequent entries (unencrypted)
    writer.clear_password();

    // Add unencrypted file
    println!("Adding readme.txt (not encrypted)");
    writer.start_entry("readme.txt")?;
    writer.write_data(b"This file is not encrypted and can be read by anyone.")?;

    // Finish the ZIP file
    writer.finish()?;

    println!("\n✓ Successfully created encrypted_example.zip");
    println!("  - confidential_data.txt (AES-256 encrypted)");
    println!("  - secrets.txt (AES-256 encrypted)");
    println!("  - readme.txt (not encrypted)");
    println!("\nPassword: my_super_secret_password_123");
    println!("\nTry extracting with:");
    println!("  7z x encrypted_example.zip");
    println!("  unzip encrypted_example.zip  (if supports WinZip AES)");

    Ok(())
}
