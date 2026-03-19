//! Proptest-based fuzz tests for ZIP parsing functions.
//!
//! These property tests feed arbitrary byte slices into the ZIP parsing
//! functions exported from `s_zip::format` and assert that:
//!
//! 1. No panics occur (the parser must handle any input gracefully).
//! 2. Return values are within documented bounds.
//!
//! The tests use `proptest` to generate random byte sequences and verify
//! invariants hold across all inputs.

use proptest::prelude::*;
use s_zip::format::{find_eocd_in_buffer, find_zip64_eocd_offset, parse_zip64_extra_field};

proptest! {
    /// `find_eocd_in_buffer` must never panic on arbitrary input.
    ///
    /// When found, the returned offset must be >= `search_start` and
    /// representable as a file position (i.e. the signature is actually
    /// present in the buffer at the reported relative position).
    #[test]
    fn prop_find_eocd_never_panics(
        buf in proptest::collection::vec(any::<u8>(), 0..4096),
        search_start in 0u64..1_000_000u64,
    ) {
        // Must not panic
        let result = find_eocd_in_buffer(&buf, search_start);

        // If found, the signature must actually be present at that relative offset
        if let Some(abs_offset) = result {
            assert!(abs_offset >= search_start, "offset must be >= search_start");
            let rel = (abs_offset - search_start) as usize;
            // The buffer at that position must contain the EOCD signature
            assert!(rel + 3 < buf.len(), "signature position must be within buffer");
            assert_eq!(buf[rel],     0x50);
            assert_eq!(buf[rel + 1], 0x4b);
            assert_eq!(buf[rel + 2], 0x05);
            assert_eq!(buf[rel + 3], 0x06);
        }
    }

    /// `find_eocd_in_buffer` must return `None` on an all-zero buffer
    /// (no EOCD signature present).
    #[test]
    fn prop_find_eocd_none_on_zeroes(
        len in 0usize..512,
        search_start in 0u64..1000u64,
    ) {
        let buf = vec![0u8; len];
        let result = find_eocd_in_buffer(&buf, search_start);
        assert!(result.is_none(), "all-zero buffer should not contain EOCD");
    }

    /// `find_zip64_eocd_offset` must never panic on arbitrary input.
    ///
    /// When found, the returned value is a `u64` (any value is valid as
    /// it represents a file offset to be sought to).
    #[test]
    fn prop_find_zip64_eocd_offset_never_panics(
        buf in proptest::collection::vec(any::<u8>(), 0..4096),
    ) {
        // Must not panic; return value can be anything (file offset)
        let _result = find_zip64_eocd_offset(&buf);
    }

    /// `parse_zip64_extra_field` must never panic on arbitrary input.
    ///
    /// The returned sizes/offset must equal the inputs when no ZIP64
    /// extra field is found (pass-through behaviour).
    #[test]
    fn prop_parse_zip64_extra_never_panics(
        buf in proptest::collection::vec(any::<u8>(), 0..512),
        compressed_32 in any::<u32>(),
        uncompressed_32 in any::<u32>(),
        offset_32 in any::<u32>(),
    ) {
        let c32 = compressed_32 as u64;
        let u32_ = uncompressed_32 as u64;
        let o32 = offset_32 as u64;

        // Must not panic
        let (u, c, o) = parse_zip64_extra_field(&buf, c32, u32_, o32);

        // Returned values must be either the originals or a parsed 64-bit value.
        // We cannot check the exact value without parsing ourselves, but we can
        // assert that if no placeholder was present, the original is returned.
        if c32 != 0xFFFFFFFF {
            assert_eq!(c, c32, "non-placeholder compressed_size must be unchanged");
        }
        if u32_ != 0xFFFFFFFF {
            assert_eq!(u, u32_, "non-placeholder uncompressed_size must be unchanged");
        }
        if o32 != 0xFFFFFFFF {
            assert_eq!(o, o32, "non-placeholder offset must be unchanged");
        }
    }

    /// Round-trip: a buffer containing only the EOCD signature at a known
    /// position must be found correctly.
    #[test]
    fn prop_find_eocd_roundtrip(
        prefix_len in 0usize..200,
        suffix_len in 0usize..200,
        search_start in 0u64..1000u64,
    ) {
        // Build a buffer with the EOCD signature at `prefix_len`
        let mut buf = vec![0u8; prefix_len + 4 + suffix_len];
        buf[prefix_len]     = 0x50;
        buf[prefix_len + 1] = 0x4b;
        buf[prefix_len + 2] = 0x05;
        buf[prefix_len + 3] = 0x06;

        let result = find_eocd_in_buffer(&buf, search_start);
        // The signature IS in the buffer, so it must be found
        assert!(result.is_some(), "EOCD signature must be found");
        let abs_offset = result.unwrap();
        assert_eq!(
            abs_offset,
            search_start + prefix_len as u64,
            "found offset must point to the EOCD signature"
        );
    }

    /// `parse_zip64_extra_field` with a well-formed ZIP64 extra replaces
    /// the placeholder 0xFFFFFFFF values correctly.
    #[test]
    fn prop_parse_zip64_extra_replaces_placeholders(
        unc in any::<u64>(),
        com in any::<u64>(),
        off in any::<u64>(),
    ) {
        // Build a minimal ZIP64 extra field with all three values
        let mut extra = Vec::new();
        extra.extend_from_slice(&0x0001u16.to_le_bytes()); // tag
        extra.extend_from_slice(&24u16.to_le_bytes());      // data_len = 3 * 8
        extra.extend_from_slice(&unc.to_le_bytes());
        extra.extend_from_slice(&com.to_le_bytes());
        extra.extend_from_slice(&off.to_le_bytes());

        let (u, c, o) = parse_zip64_extra_field(&extra, 0xFFFFFFFF, 0xFFFFFFFF, 0xFFFFFFFF);
        assert_eq!(u, unc, "uncompressed_size mismatch");
        assert_eq!(c, com, "compressed_size mismatch");
        assert_eq!(o, off, "offset mismatch");
    }
}
