# s-zip TODOS

Items deferred from plan review. Each item includes motivation, context, and enough
detail to pick up without re-reading the full review.

---

## P0 — Security ✅ COMPLETED

### [P0-1] ✅ Remove debug eprintln in AesEncryptor::new()
**Resolved in:** commit after v0.11.0
**What was done:** Removed `eprintln!` statements from `AesEncryptor::new()` that printed password, salt, and derived keys to stderr.

### [P0-2] ✅ Fix AES-CTR keystream reuse on multi-chunk writes
**Resolved in:** commit after v0.11.0
**What was done:** Added `byte_offset: u64` field to both `AesEncryptor` and `AesDecryptor`. `encrypt()` and `decrypt()` now advance the CTR block counter by `byte_offset / 16` so successive calls on the same entry use non-overlapping keystream segments. Same fix applied preemptively to `AesDecryptor` to support future streaming decrypt [P2-3]. Added 3 new tests: `test_multi_chunk_encrypt_decrypt_roundtrip`, `test_ctr_keystreams_differ_across_chunks`, `test_single_chunk_still_works_after_offset_fix`.

---

## P1 — Correctness & Reliability ✅ COMPLETED

### [P1-1] ✅ Return SZipError::IncorrectPassword from AesDecryptor::new()
**Resolved in:** v0.11.2
**What was done:** Changed `AesDecryptor::new()` to return `SZipError::IncorrectPassword` instead of `SZipError::InvalidFormat("Incorrect password")`.

### [P1-2] ✅ Change generate_salt() to return Result<Vec<u8>>
**Resolved in:** v0.11.2
**What was done:** `generate_salt()` now returns `Result<Vec<u8>>`. `getrandom` failure maps to `SZipError::EncryptionError`. `.expect()` panic removed. `AesEncryptor::new()` propagates with `?`.

### [P1-3] ✅ Add size cap to prevent OOM on malicious compressed_size
**Resolved in:** v0.11.2
**What was done:** Added `MAX_ENTRY_ALLOC = 2 GiB` constant to both `reader.rs` and `async_reader.rs`. Guard before `vec![0u8; data_size as usize]` returns `InvalidFormat` with message directing to `read_entry_streaming()`.

### [P1-4] ✅ Return explicit error for encrypted entries in read_entry_streaming()
**Resolved in:** v0.11.2
**What was done:** Sync reader checks `entry.is_encrypted` (feature-gated) and returns `SZipError::EncryptionError`. `async_reader::ZipEntry` gained `is_encrypted: bool` field; central dir parser now reads flags u16 instead of skipping. Async streaming also guards with `#[cfg(feature = "encryption")]`.

### [P1-5] ✅ Change ParallelConfig::with_max_concurrent() panics to Result
**Resolved in:** v0.11.2
**What was done:** `with_max_concurrent()` returns `Result<Self>` instead of `assert!` panic. Tests updated from `#[should_panic]` to `is_err()`. Example callsites updated with `.unwrap()`. **Breaking change** (noted in CHANGELOG/README).

### [P1-6] ✅ Add ZipEntry::safe_path() for path traversal protection
**Resolved in:** v0.11.2
**What was done:** Added `pub fn safe_path(&self) -> PathBuf` to both `reader::ZipEntry` and `async_reader::ZipEntry`. Uses `Path::components().filter(Component::Normal)` to strip `..`, leading `/`/`\`, and Windows drive prefixes.

### [P1-7] ✅ Fix write_entries_parallel() to use ZIP64 for entries >4GB
**Resolved in:** v0.11.2
**What was done:** `write_entries_parallel()` now detects when `compressed_size > u32::MAX || uncompressed_size > u32::MAX` and writes ZIP64 local headers (`version_needed=45`, `0xFFFFFFFF` placeholders, extra field ID `0x0001` with 64-bit sizes).

---

## P2 — Quality & Features

### [P2-1] Extract shared format.rs module (DRY refactor)
**What:** Create `src/format.rs` with: `CrcCounter`, `CompressedBuffer`, `parse_zip64_extra_field()`, `find_eocd_sync()`, `find_eocd_async()`, local header builder, central directory entry builder. Both `writer.rs`/`async_writer.rs` and `reader.rs`/`async_reader.rs` import from here.
**Why:** ~600 lines of code are currently duplicated between sync and async modules. Any bug fix must be applied twice. This is a maintenance liability.
**Effort:** L
**Priority:** P2
**Note:** Be careful about the `AsyncWrite` impl on `CompressedBuffer` — the sync version only implements `Write`. The shared module can provide the data struct; each module adds its trait impl.

### [P2-2] Add encryption support to async reader
**File:** `src/async_reader.rs`
**What:** Port the AES decryption path from `src/reader.rs:read_entry()` to `async_reader.rs:read_entry()`. Add `password: Option<String>` field to `GenericAsyncZipReader`.
**Why:** `AsyncStreamingZipReader` currently has no encryption support. Any async user trying to read an encrypted ZIP gets either garbled data or no path at all.
**Effort:** M
**Priority:** P2
**Depends on:** P0-2 (fix CTR bug first).

### [P2-3] Implement encryption in streaming reader (sync + async)
**What:** For `read_entry_streaming()` with an encrypted entry: decrypt the stream on-the-fly using a wrapper `DecryptingReader<R>` that applies CTR decryption. HMAC verification is tricky in streaming mode (need to read to end first).
**Why:** Users with large encrypted files can't use streaming without loading into memory.
**Effort:** L
**Priority:** P2

### [P2-4] Add proptest-based fuzz tests for ZIP parsing
**What:** Add `proptest` dev-dependency. Write property tests for `find_eocd` and `read_central_directory` that feed arbitrary byte slices and assert: function returns `Ok` or `Err`, never panics.
**Why:** ZIP parsing code is complex and manually written. Adversarial inputs (corrupt headers, wrong signatures, overflow values) are not tested. A future `cargo fuzz` migration is also worthwhile.
**Effort:** M
**Priority:** P2

### [P2-5] Implement CompressionMethod::Stored in async writer
**File:** `src/async_writer.rs:512-516`
**What:** `CompressionMethod::Stored` currently returns `InvalidFormat("Stored method not yet implemented")`. Implement a pass-through compressor for async (mirror of sync `StoredCompressor`).
**Why:** Feature parity between sync and async writers. Users who want no compression in async mode hit an error.
**Effort:** S
**Priority:** P2

### [P2-6] CRC32 verification on read_entry()
**File:** `src/reader.rs:read_entry()`, `src/async_reader.rs:read_entry()`
**What:** After decompression, compute CRC32 of the result and compare against `entry.crc32` stored in the central directory. Return `SZipError::InvalidFormat("CRC32 mismatch")` if they differ.
**Why:** Bit-rot or partial downloads go silently undetected. CRC32 data is already stored.
**Effort:** S
**Priority:** P2

### [P2-7] Add file metadata support (mtime, Unix permissions)
**What:** Add `EntryOptions` struct: `{ mtime: Option<SystemTime>, unix_mode: Option<u32> }`. Add `start_entry_with_options(name: &str, options: EntryOptions)`. Write MS-DOS time in local header from mtime. Write Unix extra field (0x5455/0x7875) for permissions.
**Why:** All entries currently have zero timestamps and no permissions, which breaks backup/archive use cases and causes "file from the future" warnings in some tools.
**Effort:** M
**Priority:** P2

### [P2-8] Stream file through compressor in parallel.rs (not read all into memory)
**File:** `src/parallel.rs:112-129`
**What:** Instead of `tokio::fs::read(&path)`, open the file with `tokio::fs::File::open()` and stream through `DeflateEncoder` using a BufReader. Use a streaming CRC32 hasher alongside.
**Why:** Currently reads entire file into memory before compressing. With 4 concurrent large files, this multiplies RAM usage by 4x.
**Effort:** S
**Priority:** P2

---

## P3 — Future Vision

### [P3-1] Seekless ZIP writer (true streaming to any AsyncWrite sink)
**What:** New `SeeklessZipWriter<W: AsyncWrite + Unpin>` that pre-compresses each entry to memory, then writes local header (with known sizes) + compressed data + central directory. No `Seek` required.
**Why:** Current `AsyncStreamingZipWriter` requires `AsyncSeek`, preventing use with HTTP response bodies, pipes, and stdio. This is the most significant architectural gap vs true streaming.
**Context:** The tradeoff is memory: pre-compressing means holding a full entry in RAM. For small entries this is fine. For large entries, a "streaming data descriptor" approach (write header with zeros, write data, write data descriptor at end) works but some tools don't support reading data descriptor mode.
**Effort:** XL
**Priority:** P3

### [P3-2] Implement AES-128 and AES-192 in AesStrength
**File:** `src/encryption.rs`
**What:** Add `Aes128` and `Aes192` variants to `AesStrength`. Wire up the different key/salt sizes in `AesEncryptor`/`AesDecryptor`.
**Why:** `set_encryption_strength()` exists in the public API but only `Aes256` works. The API promises flexibility it doesn't deliver.
**Effort:** S
**Priority:** P3

### [P3-3] Add optional `tracing` feature gate
**What:** Add `tracing = { version = "0.1", optional = true }` feature. Add `tracing::trace!` calls at: entry start/finish, compression start/finish, encryption init, flush events, and any error paths.
**Why:** Users in production can't diagnose "why did my ZIP come out wrong?" without adding their own debug builds. Structured traces allow post-hoc reconstruction.
**Effort:** M
**Priority:** P3

### [P3-4] Return ZipStats from finish()
**What:** Change `finish()` to return `Result<(W, ZipStats)>` where `ZipStats` contains: `entry_count: usize`, `total_uncompressed_bytes: u64`, `total_compressed_bytes: u64`, `compression_ratio: f32`, `encrypted: bool`. Data is already tracked in `entries` vec — this is purely additive.
**Why:** Users want to log/report "created ZIP with N files, X MB → Y MB (Z% compression)." Currently requires tracking this manually outside the library.
**Effort:** S
**Priority:** P3
**Note:** This is a breaking API change (return type change). Consider adding `finish_with_stats()` as additive alternative.

### [P3-5] Add entry_count() and bytes_written() accessors to writer
**What:** Add `pub fn entry_count(&self) -> usize` and `pub fn bytes_written(&self) -> u64` to `StreamingZipWriter` and `AsyncStreamingZipWriter`.
**Why:** Useful for progress bars and logging during write without tracking state externally.
**Effort:** XS
**Priority:** P3

### [P3-6] add_entry() one-liner convenience method
**What:** Add `pub fn add_entry(&mut self, name: &str, data: &[u8]) -> Result<()>` that calls `start_entry()` + `write_data()` + returns. For the common case of writing pre-loaded data.
**Why:** The two-step start_entry/write_data pattern is verbose for simple use cases. Users would write `writer.add_entry("readme.txt", b"Hello")` instead of 2 calls.
**Effort:** XS
**Priority:** P3

### [P3-7] Parallel extraction (read_entries_parallel)
**What:** Add `read_entries_parallel(names: Vec<String>, config: ParallelConfig) -> Result<Vec<(String, Vec<u8>)>>` to `AsyncStreamingZipReader`. Uses same semaphore pattern as write.
**Why:** Mirror of write_entries_parallel. High value for server workloads extracting many entries.
**Effort:** M
**Priority:** P3
