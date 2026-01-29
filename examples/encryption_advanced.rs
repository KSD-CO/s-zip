//! Advanced AES encryption example with multiple passwords
//!
//! This example shows how to use different passwords for different files
//! and mix encrypted/unencrypted files in the same archive.
//!
//! Run with: cargo run --example encryption_advanced --features encryption

#[cfg(not(feature = "encryption"))]
fn main() {
    eprintln!("This example requires the 'encryption' feature.");
    eprintln!("Run with: cargo run --example encryption_advanced --features encryption");
    std::process::exit(1);
}

#[cfg(feature = "encryption")]
fn main() -> s_zip::Result<()> {
    run_example()
}

#[cfg(feature = "encryption")]
fn run_example() -> s_zip::Result<()> {
    use s_zip::StreamingZipWriter;

    println!("Creating ZIP with multiple passwords and mixed encryption...\n");

    // Create a new ZIP writer
    let mut writer = StreamingZipWriter::new("mixed_encryption.zip")?;

    // Add files with first password (AES-256)
    println!("1. Adding files with password 'password123'");
    writer.set_password("password123");

    writer.start_entry("financial_report.txt")?;
    writer.write_data(b"Q4 2024 Revenue: $1.5M\nProfit Margin: 23%")?;

    writer.start_entry("employee_salaries.txt")?;
    writer.write_data(b"John Doe: $95,000\nJane Smith: $105,000\nBob Johnson: $87,000")?;

    // Change to a different password for highly sensitive data
    println!("2. Adding file with password 'ultra_secret_xyz'");
    writer.set_password("ultra_secret_xyz");
    writer.start_entry("master_passwords.txt")?;
    writer.write_data(b"Database: admin/d8$kL3#mP9\nAPI Key: sk-proj-abc123xyz")?;

    // Change to another password for legal documents
    println!("3. Adding file with password 'legal_2024'");
    writer.set_password("legal_2024");
    writer.start_entry("contracts/client_agreement.txt")?;
    writer.write_data(b"CONFIDENTIAL - Client Agreement\nParties: Company A & Company B")?;

    // Add unencrypted public files
    println!("4. Adding public unencrypted files");
    writer.clear_password();

    writer.start_entry("README.txt")?;
    writer.write_data(
        b"This archive contains encrypted files.\n\n\
        Different files require different passwords:\n\
        - Financial documents: password123\n\
        - Security credentials: ultra_secret_xyz\n\
        - Legal documents: legal_2024\n\n\
        All encrypted files use AES-256 encryption.",
    )?;

    writer.start_entry("public_notice.txt")?;
    writer.write_data(b"This file is public and not encrypted.")?;

    // Finish the ZIP file
    writer.finish()?;

    println!("\n✅ Successfully created mixed_encryption.zip");
    println!("\n📁 Archive contents:");
    println!("  ┌─ financial_report.txt        [🔒 AES-256 | password123]");
    println!("  ├─ employee_salaries.txt       [🔒 AES-256 | password123]");
    println!("  ├─ master_passwords.txt        [🔐 AES-256 | ultra_secret_xyz]");
    println!("  ├─ contracts/");
    println!("  │  └─ client_agreement.txt     [🔒 AES-256 | legal_2024]");
    println!("  ├─ README.txt                  [📄 Not encrypted]");
    println!("  └─ public_notice.txt           [📄 Not encrypted]");

    println!("\n🔐 Security Features:");
    println!("  • AES-256-CTR encryption");
    println!("  • PBKDF2-HMAC-SHA1 key derivation (1000 iterations)");
    println!("  • HMAC-SHA1 authentication");
    println!("  • WinZip AE-2 format (compatible with 7-Zip, WinZip, etc.)");

    Ok(())
}
