//! Smoke test for the `s-zip` binary (requires the `cli` feature).

#![cfg(feature = "cli")]

use std::process::Command;

fn s_zip(args: &[&std::ffi::OsStr]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_s-zip"))
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn test_pack_list_test_unpack() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("src");
    std::fs::create_dir_all(src.join("sub")).unwrap();
    std::fs::write(src.join("a.txt"), b"hello").unwrap();
    std::fs::write(src.join("sub/b.bin"), vec![3u8; 50_000]).unwrap();
    let archive = dir.path().join("out.zip");
    let dest = dir.path().join("x");

    let out = s_zip(&[
        "pack".as_ref(),
        archive.as_ref(),
        src.as_ref(),
        "--comment".as_ref(),
        "hi".as_ref(),
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let out = s_zip(&["list".as_ref(), archive.as_ref()]);
    let listing = String::from_utf8_lossy(&out.stdout);
    assert!(
        listing.contains("src/a.txt") && listing.contains("src/sub/b.bin"),
        "{listing}"
    );
    assert!(listing.contains("Comment: hi"), "{listing}");

    assert!(s_zip(&["test".as_ref(), archive.as_ref()]).status.success());

    let out = s_zip(&[
        "unpack".as_ref(),
        archive.as_ref(),
        "-d".as_ref(),
        dest.as_ref(),
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(std::fs::read(dest.join("src/a.txt")).unwrap(), b"hello");
    assert_eq!(
        std::fs::read(dest.join("src/sub/b.bin")).unwrap(),
        vec![3u8; 50_000]
    );
}

#[test]
fn test_wrong_password_fails() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("secret.txt");
    std::fs::write(&file, b"top secret").unwrap();
    let archive = dir.path().join("enc.zip");

    let out = s_zip(&[
        "pack".as_ref(),
        archive.as_ref(),
        file.as_ref(),
        "-p".as_ref(),
        "pw".as_ref(),
    ]);
    assert!(out.status.success());
    assert!(s_zip(&[
        "test".as_ref(),
        archive.as_ref(),
        "-p".as_ref(),
        "pw".as_ref()
    ])
    .status
    .success());
    assert!(!s_zip(&[
        "test".as_ref(),
        archive.as_ref(),
        "-p".as_ref(),
        "nope".as_ref()
    ])
    .status
    .success());
}
