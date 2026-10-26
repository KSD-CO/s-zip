//! Reading legacy archives: ZipCrypto (Info-ZIP `zip -P`) and bzip2 / LZMA / XZ.

use std::io::{Read, Write};
use std::path::Path;
use std::process::Command;

fn have(tool: &str, arg: &str) -> bool {
    Command::new(tool).arg(arg).output().is_ok()
}

/// Build a minimal ZIP holding one already-compressed entry.
#[allow(dead_code)]
fn build_zip(name: &str, method: u16, flags: u16, compressed: &[u8], plain: &[u8]) -> Vec<u8> {
    let crc = crc32fast::hash(plain);
    let mut out = Vec::new();
    let header = |out: &mut Vec<u8>, central: bool| {
        out.extend_from_slice(if central {
            &[0x50, 0x4b, 1, 2]
        } else {
            &[0x50, 0x4b, 3, 4]
        });
        if central {
            out.extend_from_slice(&63u16.to_le_bytes()); // version made by
        }
        out.extend_from_slice(&63u16.to_le_bytes()); // version needed
        out.extend_from_slice(&flags.to_le_bytes());
        out.extend_from_slice(&method.to_le_bytes());
        out.extend_from_slice(&[0, 0, 0x21, 0]); // time/date
        out.extend_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&(compressed.len() as u32).to_le_bytes());
        out.extend_from_slice(&(plain.len() as u32).to_le_bytes());
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // extra len
        if central {
            out.extend_from_slice(&[0u8; 14]); // comment len, disk, int/ext attrs, offset 0
        }
        out.extend_from_slice(name.as_bytes());
    };
    header(&mut out, false);
    out.extend_from_slice(compressed);
    let cd_offset = out.len();
    header(&mut out, true);
    let cd_size = out.len() - cd_offset;
    out.extend_from_slice(&[0x50, 0x4b, 5, 6, 0, 0, 0, 0, 1, 0, 1, 0]);
    out.extend_from_slice(&(cd_size as u32).to_le_bytes());
    out.extend_from_slice(&(cd_offset as u32).to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

fn plain_data() -> Vec<u8> {
    (0..200_000u32)
        .flat_map(|i| (i % 251).to_le_bytes())
        .collect()
}

fn write_tmp(dir: &Path, bytes: &[u8]) -> std::path::PathBuf {
    let path = dir.join("archive.zip");
    std::fs::write(&path, bytes).unwrap();
    path
}

/// Read `name` with every reader API and check the content.
fn check_all_readers(path: &Path, name: &str, expected: &[u8], password: Option<&str>) {
    let mut reader = s_zip::StreamingZipReader::open(path).unwrap();
    #[cfg(feature = "encryption")]
    if let Some(pw) = password {
        reader.set_password(pw);
    }
    assert_eq!(
        reader.read_entry_by_name(name).unwrap(),
        expected,
        "sync read_entry"
    );
    let mut streamed = Vec::new();
    reader
        .read_entry_streaming_by_name(name)
        .unwrap()
        .read_to_end(&mut streamed)
        .unwrap();
    assert_eq!(streamed, expected, "sync streaming");

    #[cfg(feature = "async")]
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(async {
            use tokio::io::AsyncReadExt;
            let mut reader = s_zip::AsyncStreamingZipReader::open(path).await.unwrap();
            #[cfg(feature = "encryption")]
            if let Some(pw) = password {
                reader.set_password(pw);
            }
            assert_eq!(
                reader.read_entry_by_name(name).await.unwrap(),
                expected,
                "async read_entry"
            );
            let mut streamed = Vec::new();
            reader
                .read_entry_streaming_by_name(name)
                .await
                .unwrap()
                .read_to_end(&mut streamed)
                .await
                .unwrap();
            assert_eq!(streamed, expected, "async streaming");
        });
    let _ = password;
}

// ── ZipCrypto ────────────────────────────────────────────────────────────────

#[cfg(feature = "encryption")]
#[test]
fn test_zipcrypto_info_zip() {
    if !have("zip", "-v") {
        eprintln!("zip not installed; skipping");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("secret.txt"), plain_data()).unwrap();

    // To a file: sizes and CRC in the local header (check byte = CRC high byte)
    let status = Command::new("zip")
        .current_dir(dir.path())
        .args(["-q", "-P", "s3cret", "file.zip", "secret.txt"])
        .status()
        .unwrap();
    assert!(status.success());
    check_all_readers(
        &dir.path().join("file.zip"),
        "secret.txt",
        &plain_data(),
        Some("s3cret"),
    );

    // To stdout: data descriptor (check byte = mod time high byte)
    let output = Command::new("zip")
        .current_dir(dir.path())
        .args(["-q", "-P", "s3cret", "-", "secret.txt"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let path = dir.path().join("stdout.zip");
    std::fs::write(&path, output.stdout).unwrap();
    check_all_readers(&path, "secret.txt", &plain_data(), Some("s3cret"));

    // Wrong password
    let mut reader = s_zip::StreamingZipReader::open(dir.path().join("file.zip")).unwrap();
    reader.set_password("wrong");
    let err = reader.read_entry_by_name("secret.txt").unwrap_err();
    assert!(
        matches!(
            err,
            s_zip::SZipError::IncorrectPassword | s_zip::SZipError::InvalidFormat(_)
        ),
        "{err:?}"
    );
}

// ── bzip2 / LZMA / XZ ────────────────────────────────────────────────────────

#[cfg(feature = "legacy-codecs")]
#[test]
fn test_bzip2_entry() {
    let plain = plain_data();
    let mut enc = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::default());
    enc.write_all(&plain).unwrap();
    let zip = build_zip("data.bin", 12, 0, &enc.finish().unwrap(), &plain);
    let dir = tempfile::tempdir().unwrap();
    check_all_readers(&write_tmp(dir.path(), &zip), "data.bin", &plain, None);
}

#[cfg(feature = "legacy-codecs")]
#[test]
fn test_xz_entry() {
    let plain = plain_data();
    let mut enc = liblzma::write::XzEncoder::new(Vec::new(), 6);
    enc.write_all(&plain).unwrap();
    let zip = build_zip("data.bin", 95, 0, &enc.finish().unwrap(), &plain);
    let dir = tempfile::tempdir().unwrap();
    check_all_readers(&write_tmp(dir.path(), &zip), "data.bin", &plain, None);
}

/// LZMA as stored in ZIP: 2-byte version, 2-byte props size, 5-byte props, raw stream.
#[cfg(feature = "legacy-codecs")]
fn zip_lzma(plain: &[u8]) -> Vec<u8> {
    let opts = liblzma::stream::LzmaOptions::new_preset(6).unwrap();
    let stream = liblzma::stream::Stream::new_lzma_encoder(&opts).unwrap();
    let mut enc = liblzma::write::XzEncoder::new_stream(Vec::new(), stream);
    enc.write_all(plain).unwrap();
    // .lzma alone: props(5) + size(8, unknown, so an EOS marker is written) + data
    let alone = enc.finish().unwrap();
    let mut out = vec![16, 2, 5, 0];
    out.extend_from_slice(&alone[..5]);
    out.extend_from_slice(&alone[13..]);
    out
}

#[cfg(feature = "legacy-codecs")]
#[test]
fn test_lzma_entry() {
    let plain = plain_data();
    // Flag bit 1: stream has an end-of-stream marker
    let zip = build_zip("data.bin", 14, 0x02, &zip_lzma(&plain), &plain);
    let dir = tempfile::tempdir().unwrap();
    check_all_readers(&write_tmp(dir.path(), &zip), "data.bin", &plain, None);
}

/// Cross-check against archives written by Python's zipfile (skipped without python3).
#[cfg(feature = "legacy-codecs")]
#[test]
fn test_python_zipfile_bzip2_and_lzma() {
    if !have("python3", "--version") {
        eprintln!("python3 not installed; skipping");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("py.zip");
    let script = r#"
import sys, zipfile
data = bytes(i % 251 for i in range(300000))
with zipfile.ZipFile(sys.argv[1], "w") as z:
    z.writestr(zipfile.ZipInfo("b.bin"), data, compress_type=zipfile.ZIP_BZIP2)
    z.writestr(zipfile.ZipInfo("l.bin"), data, compress_type=zipfile.ZIP_LZMA)
"#;
    let status = Command::new("python3")
        .args(["-I", "-c", script])
        .arg(&path)
        .status()
        .unwrap();
    assert!(status.success());
    let expected: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
    check_all_readers(&path, "b.bin", &expected, None);
    check_all_readers(&path, "l.bin", &expected, None);
}
