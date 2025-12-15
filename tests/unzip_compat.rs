use std::process::Command;
use tempfile::tempdir;

// This test writes a ZIP using the library and then calls `unzip -t` to verify compatibility.
// If `unzip` is not present on the system, the test will be skipped.

#[test]
fn unzip_compatibility() {
    use s_zip::StreamingZipWriter;

    // Check if `unzip` exists by trying to run `unzip -v`.
    let check = Command::new("unzip").arg("-v").output();
    if check.is_err() {
        eprintln!("skipping test: `unzip` not found");
        return;
    }

    let dir = tempdir().unwrap();
    let zip_path = dir.path().join("compat.zip");

    // Create zip
    {
        let mut writer = StreamingZipWriter::new(&zip_path).unwrap();
        writer.start_entry("hello.txt").unwrap();
        writer.write_data(b"hello from test").unwrap();
        writer.start_entry("big.bin").unwrap();
        // write a moderate amount of data to ensure non-trivial archive
        for _ in 0..1024 {
            writer.write_data(&vec![0u8; 1024]).unwrap();
        }
        writer.finish().unwrap();
    }

    // Run `unzip -t` to test archive integrity
    let output = Command::new("unzip")
        .arg("-t")
        .arg(&zip_path)
        .output()
        .expect("failed to run unzip");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "unzip reported failure: {} {}",
        stdout,
        stderr
    );
}
