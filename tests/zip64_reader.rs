use std::fs::File;
use std::io::{Seek, Write};
use tempfile::tempdir;

// This test crafts a minimal ZIP64 archive on disk with one entry by hand.
// It writes a local file header, compressed data (stored), central directory with ZIP64 extra field,
// ZIP64 EOCD record and locator, and classic EOCD with placeholders. Then we open it with StreamingZipReader.

#[test]
fn read_zip64_crafted() {
    use s_zip::StreamingZipReader;

    // Build a small ZIP64 archive in memory
    // We'll create one entry named "a.txt" with content "hello" and then craft ZIP64 structures

    let dir = tempdir().unwrap();
    let path = dir.path().join("zip64_test.zip");
    let mut f = File::create(&path).unwrap();

    // Local file header for a.txt (no sizes, using data descriptor)
    // local file header signature
    f.write_all(&[0x50, 0x4b, 0x03, 0x04]).unwrap();
    // version needed
    f.write_all(&[20, 0]).unwrap();
    // flags (bit 3 set)
    f.write_all(&[8, 0]).unwrap();
    // compression method (0 = stored)
    f.write_all(&[0, 0]).unwrap();
    // mod time/date
    f.write_all(&[0, 0, 0, 0]).unwrap();
    // crc placeholder
    f.write_all(&0u32.to_le_bytes()).unwrap();
    // compressed size placeholder
    f.write_all(&0xFFFFFFFFu32.to_le_bytes()).unwrap();
    // uncompressed size placeholder
    f.write_all(&0xFFFFFFFFu32.to_le_bytes()).unwrap();
    // name length
    f.write_all(&(5u16).to_le_bytes()).unwrap();
    // extra length
    f.write_all(&0u16.to_le_bytes()).unwrap();
    // name
    f.write_all(b"a.txt").unwrap();

    // file data (stored)
    let data = b"hello";
    let data_offset = f.stream_position().unwrap();
    f.write_all(data).unwrap();

    // data descriptor (ZIP64 style: write 64-bit sizes)
    let crc = crc32fast::hash(data);
    f.write_all(&[0x50, 0x4b, 0x07, 0x08]).unwrap();
    f.write_all(&crc.to_le_bytes()).unwrap();
    // compressed size (64)
    f.write_all(&(data.len() as u64).to_le_bytes()).unwrap();
    // uncompressed size (64)
    f.write_all(&(data.len() as u64).to_le_bytes()).unwrap();

    // central directory start
    let cd_start = f.stream_position().unwrap();

    // central dir header
    f.write_all(&[0x50, 0x4b, 0x01, 0x02]).unwrap();
    f.write_all(&[20, 0]).unwrap(); // version made by
    f.write_all(&[20, 0]).unwrap(); // version needed
    f.write_all(&[8, 0]).unwrap(); // flags
    f.write_all(&[0, 0]).unwrap(); // compression method
    f.write_all(&[0, 0, 0, 0]).unwrap(); // mod time/date
    f.write_all(&crc.to_le_bytes()).unwrap();
    // compressed size placeholder (0xFFFFFFFF)
    f.write_all(&0xFFFFFFFFu32.to_le_bytes()).unwrap();
    f.write_all(&0xFFFFFFFFu32.to_le_bytes()).unwrap();
    f.write_all(&(5u16).to_le_bytes()).unwrap(); // name len
                                                 // extra len (we'll include ZIP64 extra)
                                                 // header(2)+len(2)+data(24) => 28
    f.write_all(&(28u16).to_le_bytes()).unwrap();
    f.write_all(&0u16.to_le_bytes()).unwrap(); // comment len
    f.write_all(&0u16.to_le_bytes()).unwrap(); // disk start
    f.write_all(&0u16.to_le_bytes()).unwrap(); // internal attrs
    f.write_all(&0u32.to_le_bytes()).unwrap(); // external attrs
                                               // relative offset placeholder
    f.write_all(&0xFFFFFFFFu32.to_le_bytes()).unwrap();
    // name
    f.write_all(b"a.txt").unwrap();
    // extra field: ZIP64 (ID 0x0001): uncompressed (8), compressed (8), offset (8)
    f.write_all(&0x0001u16.to_le_bytes()).unwrap();
    f.write_all(&(24u16).to_le_bytes()).unwrap();
    // uncompressed size
    f.write_all(&(data.len() as u64).to_le_bytes()).unwrap();
    // compressed size
    f.write_all(&(data.len() as u64).to_le_bytes()).unwrap();
    // relative header offset
    f.write_all(&(data_offset - 30).to_le_bytes()).unwrap(); // roughly local header pos

    let cd_end = f.stream_position().unwrap();
    let cd_size = cd_end - cd_start;

    // Write ZIP64 EOCD record
    let zip64_eocd_start = f.stream_position().unwrap();
    f.write_all(&[0x50, 0x4b, 0x06, 0x06]).unwrap(); // zip64 eocd sig
                                                     // size of zip64 eocd record
    f.write_all(&(44u64).to_le_bytes()).unwrap();
    f.write_all(&[20, 0]).unwrap(); // version made by
    f.write_all(&[20, 0]).unwrap(); // version needed
    f.write_all(&0u32.to_le_bytes()).unwrap(); // disk number
    f.write_all(&0u32.to_le_bytes()).unwrap(); // disk start
    f.write_all(&(1u64).to_le_bytes()).unwrap(); // entries on disk
    f.write_all(&(1u64).to_le_bytes()).unwrap(); // total entries
    f.write_all(&cd_size.to_le_bytes()).unwrap(); // central dir size
    f.write_all(&cd_start.to_le_bytes()).unwrap(); // central dir offset

    // ZIP64 EOCD locator
    f.write_all(&[0x50, 0x4b, 0x06, 0x07]).unwrap();
    f.write_all(&0u32.to_le_bytes()).unwrap(); // disk with zip64 eocd
    f.write_all(&zip64_eocd_start.to_le_bytes()).unwrap(); // relative offset of zip64 eocd
    f.write_all(&0u32.to_le_bytes()).unwrap(); // total disks

    // classic EOCD with placeholders
    f.write_all(&[0x50, 0x4b, 0x05, 0x06]).unwrap();
    f.write_all(&0u16.to_le_bytes()).unwrap(); // disk
    f.write_all(&0u16.to_le_bytes()).unwrap(); // disk with cd
    f.write_all(&0xFFFFu16.to_le_bytes()).unwrap(); // entries on disk
    f.write_all(&0xFFFFu16.to_le_bytes()).unwrap(); // total entries
    f.write_all(&0xFFFFFFFFu32.to_le_bytes()).unwrap(); // cd size
    f.write_all(&0xFFFFFFFFu32.to_le_bytes()).unwrap(); // cd offset
    f.write_all(&0u16.to_le_bytes()).unwrap(); // comment len

    f.flush().unwrap();

    // Now try to open with StreamingZipReader
    let reader = StreamingZipReader::open(&path).expect("should open crafted zip64");
    let entries = reader.entries();
    assert_eq!(entries.len(), 1);
    let e = &entries[0];
    assert_eq!(e.name, "a.txt");
    assert_eq!(e.uncompressed_size, data.len() as u64);
}
