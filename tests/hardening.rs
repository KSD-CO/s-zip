//! Integration tests for v0.14.1 hardening:
//! - UTF-8 file name flag (general purpose bit 11)

use s_zip::{StreamingZipReader, StreamingZipWriter};
use tempfile::NamedTempFile;

// ── UTF-8 file names ─────────────────────────────────────────────────────────

#[test]
fn test_utf8_flag_set_and_name_roundtrip() {
    let tmp = NamedTempFile::new().unwrap();
    let name = "thư mục/Báo cáo tháng 10.txt";
    {
        let mut writer = StreamingZipWriter::new(tmp.path()).unwrap();
        writer.add_entry(name, b"xin chao").unwrap();
        writer.finish().unwrap();
    }

    let bytes = std::fs::read(tmp.path()).unwrap();
    // Local header general purpose flag at offset 6
    let flags = u16::from_le_bytes([bytes[6], bytes[7]]);
    assert_ne!(flags & (1 << 11), 0, "UTF-8 flag must be set");

    let mut reader = StreamingZipReader::open(tmp.path()).unwrap();
    assert_eq!(reader.entries()[0].name, name);
    assert_eq!(reader.read_entry_by_name(name).unwrap(), b"xin chao");
}
